"""Fixed Blender 4.5 native backend for the Rust-validated authoring profile.

No payload selects a Python function, operator, module, expression or RNA data path.
All scene creation below is product code, not a test harness authoring shortcut.
"""
import hashlib
import json
import math
import os
import tempfile
import uuid
from pathlib import Path

from mathutils import Vector, geometry

from .validation import CommandError

ISLAND = "sw_authoring_island"
ENTITY = "sw_authoring_entity"
MATERIAL = "sw_authoring_material"
TEXTURE = "sw_authoring_texture"
MARKER = "sw_authoring_observed_v1"


def check(ok, message, code="InvalidArgument"):
    if not ok:
        raise CommandError(code, message)


def local_id(value):
    check(isinstance(value, str) and 1 <= len(value) <= 64 and
          all(c.isascii() and (c.isalnum() or c in "_-") for c in value), "invalid authoring ID")
    return value


def digest(value):
    # Native geometry/internal digest helper; source projection has its own canonicalizer below.
    body = json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode()
    check(len(body) <= 16_777_216, "native projection budget", "Unsupported")
    return hashlib.sha256(body).hexdigest()


def source_projection_value(value):
    # IEEE signed zero is semantically identical for Blender source state but JSON encodes
    # -0.0 and 0.0 differently. Normalize only exact zeros; do not round or add tolerance.
    if isinstance(value, float):
        return 0.0 if value == 0.0 else value
    if isinstance(value, list):
        return [source_projection_value(item) for item in value]
    if isinstance(value, tuple):
        return [source_projection_value(item) for item in value]
    if isinstance(value, dict):
        return {key: source_projection_value(item) for key, item in value.items()}
    return value


def vec(value, count=3, maximum=1_000_000.0):
    check(isinstance(value, (list, tuple)) and len(value) == count, "vector shape")
    check(all(not isinstance(v, bool) and isinstance(v, (int, float)) and math.isfinite(v) and abs(v) <= maximum for v in value), "finite vector bounds")
    return list(value)


def close(a, b, tolerance=1e-5):
    return len(a) == len(b) and all(abs(x-y) <= tolerance * max(1.0, abs(x), abs(y)) for x, y in zip(a, b))


def managed_mesh_attributes(mesh):
    rows = []
    total = 0
    fields = {"FLOAT": "value", "FLOAT_VECTOR": "vector", "FLOAT_COLOR": "color"}
    for attribute in mesh.attributes:
        if not attribute.name.startswith("SW_attr_"):
            continue
        identity = local_id(attribute.name[len("SW_attr_"):])
        check(attribute.data_type in fields, "managed mesh attribute type changed", "Conflict")
        check(attribute.domain in {"POINT", "FACE", "CORNER"}, "managed mesh attribute domain changed", "Conflict")
        check(len(attribute.data) <= 32768, "managed mesh attribute data budget", "Unsupported")
        total += len(attribute.data)
        check(total <= 65536, "managed mesh attribute total budget", "Unsupported")
        field = fields[attribute.data_type]
        values = []
        for item in attribute.data:
            value = getattr(item, field)
            values.append(float(value) if field == "value" else [float(v) for v in value])
        rows.append({
            "id": identity,
            "name": attribute.name,
            "domain": attribute.domain,
            "data_type": attribute.data_type,
            "values": values,
        })
    return sorted(rows, key=lambda row: row["id"])


def bounds_overlap(a, b, epsilon=1e-9):
    return all(a[i][0] <= b[i][1] + epsilon and b[i][0] <= a[i][1] + epsilon for i in range(3))


def triangle_bounds(triangle):
    return [[min(point[i] for point in triangle), max(point[i] for point in triangle)] for i in range(3)]


def segment_triangle(a, b, triangle, epsilon=1e-9):
    origin = Vector(a)
    direction = Vector(b) - origin
    v0, v1, v2 = (Vector(point) for point in triangle)
    edge1 = v1 - v0
    edge2 = v2 - v0
    h = direction.cross(edge2)
    determinant = edge1.dot(h)
    if abs(determinant) <= epsilon:
        return False
    inverse = 1.0 / determinant
    s = origin - v0
    u = inverse * s.dot(h)
    if u < -epsilon or u > 1.0 + epsilon:
        return False
    q = s.cross(edge1)
    v = inverse * direction.dot(q)
    if v < -epsilon or u + v > 1.0 + epsilon:
        return False
    t = inverse * edge2.dot(q)
    return -epsilon <= t <= 1.0 + epsilon


def triangles_intersect(a, b, epsilon=1e-9):
    va = [Vector(point) for point in a]
    vb = [Vector(point) for point in b]
    normal_a = (va[1] - va[0]).cross(va[2] - va[0])
    normal_b = (vb[1] - vb[0]).cross(vb[2] - vb[0])
    if normal_a.length_squared <= epsilon or normal_b.length_squared <= epsilon:
        return None
    unit_a = normal_a.normalized()
    unit_b = normal_b.normalized()
    parallel = abs(unit_a.dot(unit_b)) >= 1.0 - 1e-8
    coplanar = parallel and abs(unit_a.dot(vb[0] - va[0])) <= 1e-8
    if coplanar:
        axis = max(range(3), key=lambda i: abs(unit_a[i]))
        def project(point):
            return Vector(tuple(point[i] for i in range(3) if i != axis))
        return geometry.intersect_tri_tri_2d(
            *(project(point) for point in [*va, *vb])
        )
    if parallel:
        return False
    for first, second, triangle in [
        (va[0], va[1], vb), (va[1], va[2], vb), (va[2], va[0], vb),
        (vb[0], vb[1], va), (vb[1], vb[2], va), (vb[2], vb[0], va),
    ]:
        if segment_triangle(first, second, triangle, epsilon):
            return True
    return False


def narrow_pair(triangles_a, triangles_b):
    if len(triangles_a) * len(triangles_b) > 2_000_000:
        return None, 0
    tested = 0
    for triangle_a in triangles_a:
        bounds_a = triangle_bounds(triangle_a)
        for triangle_b in triangles_b:
            if not bounds_overlap(bounds_a, triangle_bounds(triangle_b)):
                continue
            tested += 1
            intersection = triangles_intersect(triangle_a, triangle_b)
            if intersection is None:
                return None, tested
            if intersection:
                return True, tested
    return False, tested


class AuthoringRuntime:
    def __init__(self, bpy, commands, semantic):
        self.bpy = bpy
        self.commands = commands
        self.semantic = semantic
        self.session = uuid.uuid4().hex

    def island(self, identity):
        local_id(identity)
        check(len(self.bpy.data.collections) <= 4096, "collection enumeration budget", "Unsupported")
        found = [c for c in self.bpy.data.collections if c.get(ISLAND) == identity]
        check(len(found) == 1, "managed island missing or ambiguous", "Conflict")
        check(found[0].library is None and found[0].override_library is None, "linked island is read-only", "PolicyDenied")
        return found[0]

    def entity(self, island, identity):
        local_id(identity)
        rows = [o for o in self.island(island).all_objects if o.get(ENTITY) == identity and o.get(ISLAND) == island]
        check(len(rows) == 1, "entity native identity missing or ambiguous", "Conflict")
        check(rows[0].library is None and rows[0].override_library is None, "linked object is read-only", "PolicyDenied")
        return rows[0]

    def material(self, island, identity):
        local_id(identity)
        check(len(self.bpy.data.materials) <= 4096, "material enumeration budget", "Unsupported")
        rows = [m for m in self.bpy.data.materials if m.get(ISLAND) == island and m.get(MATERIAL) == identity]
        check(len(rows) == 1, "material native identity missing or ambiguous", "Conflict")
        return rows[0]

    def texture(self, island, identity):
        local_id(identity)
        check(len(self.bpy.data.images) <= 4096, "image enumeration budget", "Unsupported")
        rows = [image for image in self.bpy.data.images if image.get(ISLAND) == island and image.get(TEXTURE) == identity]
        check(len(rows) == 1, "texture native identity missing or ambiguous", "Conflict")
        return rows[0]

    def _name(self, island, alias, root):
        name = "SW_" + local_id(island)[:12] + "_" + local_id(alias)[:40]
        check(root.get(name) is None, "native name collision; no implicit adoption or suffix", "Conflict")
        return name

    def _objects(self, island):
        objects = self.bpy.data.objects if island is None else self.island(island).all_objects
        check(len(objects) <= 512, "complete object enumeration exceeds budget", "Unsupported")
        values = list(objects)
        check(sum(len(o.data.vertices) for o in values if o.type == "MESH") <= 262_144, "source mesh readback budget", "Unsupported")
        return sorted(values, key=lambda o: (str(o.get(ENTITY, "")), o.name))

    def _action(self, action):
        if action is None:
            return None
        check(len(action.layers) <= 8 and len(action.slots) <= 128, "action readback budget", "Unsupported")
        curves = []
        for layer in action.layers:
            check(len(layer.strips) <= 8, "action strip budget", "Unsupported")
            for strip in layer.strips:
                check(hasattr(strip, "channelbags") and len(strip.channelbags) <= 128, "unsupported action strip", "Unsupported")
                for bag in strip.channelbags:
                    check(len(bag.fcurves) <= 384, "F-Curve enumeration budget", "Unsupported")
                    for curve in bag.fcurves:
                        check(len(curve.keyframe_points) <= 512 and len(curve.modifiers) == 0, "F-Curve readback budget/modifier coverage", "Unsupported")
                        curves.append({"path": curve.data_path, "index": curve.array_index,
                                       "keys": [[float(k.co.x), float(k.co.y), k.interpolation] for k in curve.keyframe_points]})
        check(len(curves) <= 384, "total F-Curve budget", "Unsupported")
        return {"name": action.name, "slots": len(action.slots), "curves": curves}

    def _material(self, material):
        if material is None:
            return None
        values = {"id": material.get(MATERIAL), "name": material.name,
                  "color": list(material.diffuse_color), "roughness": material.roughness,
                  "metallic": material.metallic, "library": material.library is not None,
                  "nodes": []}
        if material.use_nodes:
            shader = material.node_tree.nodes.get("Principled BSDF")
            check(shader is not None, "managed Principled shader missing", "Unsupported")
            for socket in ("Base Color", "Alpha", "Emission Color", "Emission Strength"):
                check(shader.inputs.get(socket) is not None, "pinned Principled input missing: " + socket, "Unsupported")
            values["base_color"] = list(shader.inputs["Base Color"].default_value)
            values["opacity"] = float(shader.inputs["Alpha"].default_value)
            values["emission_color"] = list(shader.inputs["Emission Color"].default_value)
            values["emission_strength"] = float(shader.inputs["Emission Strength"].default_value)
            check(len(material.node_tree.nodes) <= 64 and len(material.node_tree.links) <= 128, "node readback budget", "Unsupported")
            for node in material.node_tree.nodes:
                ports = []
                for socket in node.inputs:
                    if hasattr(socket, "default_value"):
                        value = socket.default_value
                        if isinstance(value, (float, int, bool)):
                            ports.append([socket.identifier, value])
                        elif hasattr(value, "__len__") and not isinstance(value, str) and len(value) <= 4:
                            ports.append([socket.identifier, list(value)])
                values["nodes"].append({"name": node.name, "type": node.bl_idname, "inputs": ports})
            values["links"] = sorted([l.from_node.name, l.from_socket.identifier, l.to_node.name, l.to_socket.identifier] for l in material.node_tree.links)
            bindings = []
            expected_inputs = {
                "base_color": "Base Color",
                "roughness": "Roughness",
                "metallic": "Metallic",
                "emission": "Emission Color",
                "opacity": "Alpha",
            }
            for node in material.node_tree.nodes:
                if node.bl_idname != "ShaderNodeTexImage" or node.image is None:
                    continue
                role = node.get("sw_texture_role")
                if not role:
                    continue
                image = node.image
                texture_id = image.get(TEXTURE)
                channel = node.get("sw_texture_channel", "color")
                path = Path(self.bpy.path.abspath(image.filepath))
                try:
                    relative = path.relative_to(self.commands.workspace.root).as_posix()
                    suffix = path.suffix.lower()
                    verified = Path(self.commands.workspace.path(relative, suffix, existing=True))
                    actual_sha256 = hashlib.sha256(verified.read_bytes()).hexdigest()
                except (ValueError, OSError, CommandError):
                    actual_sha256 = None
                topology_valid = False
                links = material.node_tree.links
                same_node = lambda left, right: left.as_pointer() == right.as_pointer()
                if role == "normal":
                    normal_nodes = [
                        candidate
                        for candidate in material.node_tree.nodes
                        if candidate.bl_idname == "ShaderNodeNormalMap"
                        and candidate.get("sw_texture_role") == "normal"
                    ]
                    topology_valid = len(normal_nodes) == 1 and any(
                        same_node(link.from_node, node)
                        and link.from_socket.name == "Color"
                        and same_node(link.to_node, normal_nodes[0])
                        and link.to_socket.name == "Color"
                        for link in links
                    ) and any(
                        same_node(link.from_node, normal_nodes[0])
                        and link.from_socket.name == "Normal"
                        and same_node(link.to_node, shader)
                        and link.to_socket.name == "Normal"
                        for link in links
                    )
                elif role in expected_inputs:
                    target = expected_inputs[role]
                    if channel == "alpha":
                        topology_valid = any(
                            same_node(link.from_node, node)
                            and link.from_socket.name == "Alpha"
                            and same_node(link.to_node, shader)
                            and link.to_socket.name == target
                            for link in links
                        )
                    elif channel == "color":
                        topology_valid = any(
                            same_node(link.from_node, node)
                            and link.from_socket.name == "Color"
                            and same_node(link.to_node, shader)
                            and link.to_socket.name == target
                            for link in links
                        )
                    else:
                        separate = [
                            candidate
                            for candidate in material.node_tree.nodes
                            if candidate.bl_idname == "ShaderNodeSeparateColor"
                            and candidate.get("sw_texture_role") == role
                        ]
                        output = {"red": "Red", "green": "Green", "blue": "Blue"}[channel]
                        topology_valid = len(separate) == 1 and any(
                            same_node(link.from_node, node)
                            and link.from_socket.name == "Color"
                            and same_node(link.to_node, separate[0])
                            and link.to_socket.name == "Color"
                            for link in links
                        ) and any(
                            same_node(link.from_node, separate[0])
                            and link.from_socket.name == output
                            and same_node(link.to_node, shader)
                            and link.to_socket.name == target
                            for link in links
                        )
                strength = None
                if role == "normal" and 'normal_nodes' in locals() and len(normal_nodes) == 1:
                    strength = float(normal_nodes[0].inputs["Strength"].default_value)
                bindings.append({
                    "role": role,
                    "texture": texture_id,
                    "channel": channel,
                    "colorspace": image.colorspace_settings.name,
                    "sha256": actual_sha256,
                    "topology_valid": topology_valid,
                    "strength": strength,
                })
            values["texture_bindings"] = sorted(bindings, key=lambda item: item["role"])
        return values

    def _row(self, obj):
        check(len(obj.modifiers) <= 16 and len(obj.constraints) <= 16, "modifier/constraint readback budget", "Unsupported")
        value = {"entity": obj.get(ENTITY), "name": obj.name, "type": obj.type,
                 "native_island": obj.get(ISLAND), "library": obj.library is not None,
                 "translation": list(obj.location), "rotation": list(obj.rotation_euler), "scale": list(obj.scale),
                 "parent": obj.parent.get(ENTITY) if obj.parent else None,
                 "parent_name": obj.parent.name if obj.parent else None,
                 "parent_type": obj.parent_type, "parent_bone": obj.parent_bone,
                 "parent_inverse": [list(row) for row in obj.matrix_parent_inverse],
                 "collections": sorted(c.name for c in obj.users_collection),
                 "hidden_render": obj.hide_render, "hidden_viewport": obj.hide_viewport,
                 "modifiers": [], "constraints": [], "action": None, "actions": [],
                 "nla_tracks": [], "drivers": 0}
        for modifier in obj.modifiers:
            settings = {"name": modifier.name, "type": modifier.type,
                        "show_viewport": modifier.show_viewport, "show_render": modifier.show_render}
            if modifier.type == "BEVEL": settings.update(width=modifier.width, segments=modifier.segments)
            elif modifier.type == "MIRROR": settings.update(axes=list(modifier.use_axis))
            elif modifier.type == "SUBSURF": settings.update(levels=modifier.levels, render_levels=modifier.render_levels)
            elif modifier.type == "ARRAY": settings.update(count=modifier.count, offset=list(modifier.constant_offset_displace), relative=modifier.use_relative_offset)
            elif modifier.type == "ARMATURE": settings.update(target=modifier.object.get(ENTITY) if modifier.object else None)
            elif modifier.type == "BOOLEAN":
                settings.update(
                    target=modifier.object.get(ENTITY) if modifier.object else None,
                    operation=modifier.operation,
                    solver=modifier.solver,
                )
            else: settings["coverage"] = "UNKNOWN"
            value["modifiers"].append(settings)
        for constraint in obj.constraints:
            check(constraint.type in {"COPY_LOCATION", "TRACK_TO"}, "constraint readback unsupported", "Unsupported")
            settings = {"type": constraint.type, "target": constraint.target.get(ENTITY) if constraint.target else None,
                        "target_name": constraint.target.name if constraint.target else None,
                        "influence": constraint.influence, "mute": constraint.mute,
                        "owner_space": constraint.owner_space, "target_space": constraint.target_space}
            if constraint.type == "COPY_LOCATION": settings["offset"] = constraint.use_offset
            else: settings.update(track_axis=constraint.track_axis, up_axis=constraint.up_axis)
            value["constraints"].append(settings)
        if obj.animation_data:
            animation_data = obj.animation_data
            value["drivers"] = len(animation_data.drivers)
            check(len(animation_data.nla_tracks) <= 16, "NLA track readback budget", "Unsupported")
            actions = {}
            if animation_data.action is not None:
                actions[animation_data.action.as_pointer()] = animation_data.action
                value["action"] = self._action(animation_data.action)
            nla_tracks = []
            total_strips = 0
            for track in animation_data.nla_tracks:
                check(len(track.strips) <= 16, "NLA strip readback budget", "Unsupported")
                strips = []
                for strip in track.strips:
                    total_strips += 1
                    check(total_strips <= 64, "total NLA strip readback budget", "Unsupported")
                    check(strip.action is not None, "NLA strip missing Action", "Unsupported")
                    check(strip.action.get(ISLAND) == obj.get(ISLAND), "NLA strip references unmanaged Action", "PolicyDenied")
                    actions[strip.action.as_pointer()] = strip.action
                    strips.append({
                        "name": strip.name,
                        "action": strip.action.name,
                        "frame_start": float(strip.frame_start),
                        "frame_end": float(strip.frame_end),
                        "action_frame_start": float(strip.action_frame_start),
                        "action_frame_end": float(strip.action_frame_end),
                        "repeat": float(strip.repeat),
                        "scale": float(strip.scale),
                        "influence": float(strip.influence),
                    })
                nla_tracks.append({"name": track.name, "muted": bool(track.mute), "strips": strips})
            value["nla_tracks"] = nla_tracks
            value["actions"] = [self._action(actions[key]) for key in sorted(actions)]
        if obj.data:
            value["data_name"] = obj.data.name
            value["data_library"] = obj.data.library is not None
        if obj.type == "MESH":
            mesh = obj.data
            check(len(mesh.polygons) <= 262_144 and len(mesh.loops) <= 1_048_576 and len(mesh.uv_layers) <= 8, "mesh readback bound", "Unsupported")
            check(len(obj.vertex_groups) <= 64, "weight group readback budget", "Unsupported")
            attributes = managed_mesh_attributes(mesh)
            normals = [list(p.normal) for p in mesh.polygons]
            value["geometry_digest"] = digest(source_projection_value({
                "vertices": [list(v.co) for v in mesh.vertices],
                "faces": [[list(p.vertices), p.material_index, p.use_smooth] for p in mesh.polygons],
                "uv": [[[float(x) for x in loop.uv] for loop in layer.data] for layer in mesh.uv_layers],
                "weights": [[[group.group, group.weight] for group in v.groups] for v in mesh.vertices],
                "groups": [group.name for group in obj.vertex_groups],
                "attributes": attributes,
            }))
            value["normal_digest"] = digest(source_projection_value(normals))
            value["normal_method"] = "source-polygon-normal-v1"
            value["smooth_polygons"] = sum(1 for polygon in mesh.polygons if polygon.use_smooth)
            value["attributes"] = attributes
            value.update(vertices=len(mesh.vertices), polygons=len(mesh.polygons), uv_layers=len(mesh.uv_layers), data_users=mesh.users)
            value["materials"] = [self._material(m) for m in mesh.materials]
        elif obj.type == "CURVE":
            curve = obj.data
            check(len(curve.splines) == 1, "managed curve requires one spline", "Unsupported")
            spline = curve.splines[0]
            check(spline.type == "POLY" and len(spline.points) <= 256, "managed curve spline coverage", "Unsupported")
            value["curve"] = {
                "points": [list(point.co[:3]) for point in spline.points],
                "cyclic": bool(spline.use_cyclic_u),
                "extrude": float(curve.extrude),
                "bevel_depth": float(curve.bevel_depth),
                "bevel_resolution": int(curve.bevel_resolution),
            }
            value["data_users"] = curve.users
            value["materials"] = [self._material(material) for material in curve.materials]
        elif obj.type == "ARMATURE":
            check(len(obj.data.bones) <= 64, "bone enumeration budget", "Unsupported")
            value["bones"] = [{"id": b.name, "head": list(b.head_local), "tail": list(b.tail_local),
                               "parent": b.parent.name if b.parent else None, "deform": b.use_deform} for b in obj.data.bones]
        elif obj.type == "CAMERA":
            value["camera"] = {"lens_mm": obj.data.lens, "clip_start": obj.data.clip_start, "clip_end": obj.data.clip_end}
        elif obj.type == "LIGHT":
            value["light"] = {"type": obj.data.type, "energy_watts": obj.data.energy, "size": getattr(obj.data, "size", None), "color": list(obj.data.color)}
        return value

    def snapshot(self, island=None):
        rows = [self._row(obj) for obj in self._objects(island)]
        scene = self.bpy.context.scene
        fingerprint = digest(source_projection_value({
                              "schema": "blender-source-projection-v2", "items": rows,
                              "scene_units": scene.unit_settings.scale_length,
                              "fps": scene.render.fps, "fps_base": scene.render.fps_base,
                              "frame": scene.frame_current
                          }))
        marker = self.island(island).get(MARKER) if island else None
        return {"native_session": self.session, "island": island, "fingerprint": fingerprint,
                "drift": bool(island and marker != fingerprint), "total": len(rows), "items": rows,
                "source_only": True, "exhaustive": True}

    def _closed(self, island):
        collection = self.island(island)
        objects = self._objects(island)
        pointers = {o.as_pointer() for o in objects}
        check(objects and len(objects) <= 128, "managed object budget")
        for obj in objects:
            check(obj.get(ISLAND) == island and obj.get(ENTITY), "unmanaged collection member", "PolicyDenied")
            check(obj.library is None and obj.override_library is None and not obj.is_instancer, "linked/override/instancer scope", "PolicyDenied")
            check(obj.parent is None or obj.parent.as_pointer() in pointers, "parent outside scope", "PolicyDenied")
            check(all(c is collection for c in obj.users_collection), "object linked into another collection", "PolicyDenied")
            if obj.animation_data:
                animation_data = obj.animation_data
                check(len(animation_data.drivers) == 0, "active animation drivers excluded", "PolicyDenied")
                check(len(animation_data.nla_tracks) <= 16, "NLA track closure budget", "Unsupported")
                managed_actions = []
                if animation_data.action is not None:
                    managed_actions.append(animation_data.action)
                total_strips = 0
                for track in animation_data.nla_tracks:
                    check(len(track.strips) <= 16, "NLA strip closure budget", "Unsupported")
                    for strip in track.strips:
                        total_strips += 1
                        check(total_strips <= 64, "total NLA strip closure budget", "Unsupported")
                        check(strip.action is not None, "NLA strip missing Action", "PolicyDenied")
                        managed_actions.append(strip.action)
                for action in managed_actions:
                    check(
                        action.library is None and action.get(ISLAND) == island,
                        "external action dependency",
                        "PolicyDenied",
                    )
            if obj.data:
                check(obj.data.library is None and obj.data.override_library is None, "linked data excluded", "PolicyDenied")
                if obj.type == "MESH":
                    check(all(o.as_pointer() in pointers for o in self.bpy.data.objects if o.data == obj.data), "shared datablock has an external user", "PolicyDenied")
                    for m in obj.data.materials:
                        check(m and m.get(ISLAND) == island and m.library is None, "foreign material dependency", "PolicyDenied")
                        check(m.use_nodes and all(n.bl_idname in {"ShaderNodeBsdfPrincipled", "ShaderNodeOutputMaterial", "ShaderNodeTexImage", "ShaderNodeNormalMap", "ShaderNodeSeparateColor"} for n in m.node_tree.nodes), "material graph outside managed PBR profile", "PolicyDenied")
                        for node in m.node_tree.nodes:
                            if node.bl_idname == "ShaderNodeTexImage":
                                check(node.image is not None and node.image.get(ISLAND) == island and node.image.get(TEXTURE), "foreign image dependency", "PolicyDenied")
                                image_path = Path(self.bpy.path.abspath(node.image.filepath))
                                try:
                                    relative = image_path.relative_to(self.commands.workspace.root).as_posix()
                                except ValueError as error:
                                    raise CommandError("PolicyDenied", "texture escaped workspace") from error
                                verified = Path(self.commands.workspace.path(relative, image_path.suffix.lower(), existing=True))
                                check(verified.stat().st_size <= 33_554_432, "texture byte budget", "Unsupported")
                                check(hashlib.sha256(verified.read_bytes()).hexdigest() == node.image.get("sw_sha256"), "texture changed after load", "StaleReference")
                        check(not m.animation_data or len(m.animation_data.drivers) == 0, "material driver excluded", "PolicyDenied")
            if obj.type == "CURVE":
                for material in obj.data.materials:
                    check(material and material.get(ISLAND) == island and material.library is None, "foreign curve material dependency", "PolicyDenied")
                    check(material.use_nodes and all(node.bl_idname in {"ShaderNodeBsdfPrincipled", "ShaderNodeOutputMaterial", "ShaderNodeTexImage", "ShaderNodeNormalMap", "ShaderNodeSeparateColor"} for node in material.node_tree.nodes), "curve material graph outside managed PBR profile", "PolicyDenied")
                    for node in material.node_tree.nodes:
                        if node.bl_idname == "ShaderNodeTexImage":
                            check(node.image is not None and node.image.get(ISLAND) == island and node.image.get(TEXTURE), "foreign curve image dependency", "PolicyDenied")
                            image_path = Path(self.bpy.path.abspath(node.image.filepath))
                            try:
                                relative = image_path.relative_to(self.commands.workspace.root).as_posix()
                            except ValueError as error:
                                raise CommandError("PolicyDenied", "curve texture escaped workspace") from error
                            verified = Path(self.commands.workspace.path(relative, image_path.suffix.lower(), existing=True))
                            check(verified.stat().st_size <= 33_554_432, "curve texture byte budget", "Unsupported")
                            check(hashlib.sha256(verified.read_bytes()).hexdigest() == node.image.get("sw_sha256"), "curve texture changed after load", "StaleReference")
            for modifier in obj.modifiers:
                check(modifier.type in {"BEVEL", "MIRROR", "SUBSURF", "ARRAY", "ARMATURE", "BOOLEAN"}, "unmanaged modifier", "PolicyDenied")
                if modifier.type == "ARMATURE": check(modifier.object and modifier.object.as_pointer() in pointers, "external armature", "PolicyDenied")
                if modifier.type == "BOOLEAN":
                    check(modifier.object and modifier.object.as_pointer() in pointers, "external boolean target", "PolicyDenied")
                    check(modifier.operand_type == "OBJECT" and modifier.solver == "EXACT", "boolean contract drift", "PolicyDenied")
                if modifier.type == "MIRROR": check(modifier.mirror_object is None, "external mirror reference", "PolicyDenied")
                if modifier.type == "ARRAY": check(not modifier.use_object_offset and not modifier.start_cap and not modifier.end_cap, "external array dependency", "PolicyDenied")
            for constraint in obj.constraints:
                check(constraint.type in {"COPY_LOCATION", "TRACK_TO"} and constraint.target and constraint.target.as_pointer() in pointers, "external constraint target", "PolicyDenied")
        return collection, objects

    def begin(self, expected, island, allow_drift=False):
        check(isinstance(allow_drift, bool), "allow_drift must be boolean")
        check(abs(self.bpy.context.scene.unit_settings.scale_length - 1.0) <= 1e-9, "explicit meter-coordinate profile requires scene unit scale 1", "Unsupported")
        observed = self.snapshot(island)
        check(observed["fingerprint"] == expected, "plan base changed", "StaleReference")
        if island:
            check(allow_drift or not observed["drift"], "manual edit drift", "StaleReference")
            self._closed(island)
        return observed

    def _transform(self, obj, value, units):
        check(math.isfinite(units) and 0.0001 <= units <= 100, "unit scale")
        obj.rotation_mode = "XYZ"
        obj.location = [x * units for x in vec(value["translation"])]
        obj.rotation_euler = vec(value["rotation"])
        scale = vec(value["scale"])
        check(all(abs(v) >= 0.0001 for v in scale), "singular scale")
        obj.scale = scale

    def apply(self, operation):
        kind = operation["kind"]
        island = local_id(operation["island"])
        data = self.bpy.data
        if kind == "collection":
            name = self._name(island, operation["name"], data.collections)
            check(not any(c.get(ISLAND) == island for c in data.collections), "island identity already exists", "Conflict")
            collection = data.collections.new(name)
            collection[ISLAND] = island
            collection["sw_display_name"] = operation["name"]
            self.bpy.context.scene.collection.children.link(collection)
        elif kind == "texture":
            self.island(island)
            spec = operation["texture"]
            suffix = Path(spec["path"]).suffix.lower()
            check(suffix in {".png", ".jpg", ".jpeg", ".exr", ".tif", ".tiff", ".tga", ".bmp"}, "texture codec", "PolicyDenied")
            source = Path(self.commands.workspace.path(spec["path"], suffix, existing=True))
            check(source.stat().st_size <= 33_554_432, "texture byte budget", "Unsupported")
            actual = hashlib.sha256(source.read_bytes()).hexdigest()
            check(actual == spec["sha256"], "texture bytes changed", "StaleReference")
            desired = self._name(island, spec["id"], data.images)
            image = data.images.load(str(source), check_existing=False)
            image.name = desired
            image[ISLAND] = island
            image[TEXTURE] = spec["id"]
            image["sw_sha256"] = actual
            image.colorspace_settings.name = {
                "srgb": "sRGB",
                "non_color": "Non-Color",
            }[spec["color_space"]]
        elif kind == "material":
            self.island(island)
            spec = operation["material"]
            mat = data.materials.new(self._name(island, spec["id"], data.materials))
            mat[ISLAND] = island; mat[MATERIAL] = spec["id"]
            base = vec(spec["base_color"], 4)
            opacity = spec["opacity"]
            mat.diffuse_color = [base[0], base[1], base[2], opacity]
            mat.roughness = spec["roughness"]; mat.metallic = spec["metallic"]; mat.use_nodes = True
            node = mat.node_tree.nodes.get("Principled BSDF")
            check(node is not None, "pinned Principled shader missing", "Unsupported")
            for socket in ("Base Color", "Roughness", "Metallic", "Alpha", "Emission Color", "Emission Strength"):
                check(node.inputs.get(socket) is not None, "pinned Principled input missing: " + socket, "Unsupported")
            node.inputs["Base Color"].default_value = base
            node.inputs["Roughness"].default_value = mat.roughness
            node.inputs["Metallic"].default_value = mat.metallic
            node.inputs["Alpha"].default_value = opacity
            node.inputs["Emission Color"].default_value = vec(spec["emission_color"], 4)
            node.inputs["Emission Strength"].default_value = spec["emission_strength"]
            nodes = mat.node_tree.nodes
            links = mat.node_tree.links

            def texture_node(role, texture_id, channel="color"):
                image = self.texture(island, texture_id)
                tex = nodes.new("ShaderNodeTexImage")
                tex.name = "SW_tex_" + role
                tex.image = image
                tex["sw_texture_role"] = role
                tex["sw_texture_channel"] = channel
                return tex

            def scalar_socket(role, binding):
                tex = texture_node(role, binding["texture"], binding["channel"])
                channel = binding["channel"]
                if channel == "alpha":
                    return tex.outputs["Alpha"]
                check(channel in {"red", "green", "blue"}, "scalar texture channel")
                separate = nodes.new("ShaderNodeSeparateColor")
                separate.name = "SW_separate_" + role
                separate.mode = "RGB"
                separate["sw_texture_role"] = role
                links.new(tex.outputs["Color"], separate.inputs["Color"])
                return separate.outputs[{"red":"Red", "green":"Green", "blue":"Blue"}[channel]]

            for role, field, input_name in [
                ("base_color", "base_color_texture", "Base Color"),
                ("emission", "emission_texture", "Emission Color"),
            ]:
                binding = spec.get(field)
                if binding:
                    tex = texture_node(role, binding["texture"], binding["channel"])
                    links.new(tex.outputs["Color"], node.inputs[input_name])
            for role, field, input_name in [
                ("roughness", "roughness_texture", "Roughness"),
                ("metallic", "metallic_texture", "Metallic"),
                ("opacity", "opacity_texture", "Alpha"),
            ]:
                binding = spec.get(field)
                if binding:
                    links.new(scalar_socket(role, binding), node.inputs[input_name])
            normal = spec.get("normal_texture")
            if normal:
                tex = texture_node("normal", normal["texture"], "color")
                normal_map = nodes.new("ShaderNodeNormalMap")
                normal_map.name = "SW_normal"
                normal_map["sw_texture_role"] = "normal"
                normal_map.inputs["Strength"].default_value = normal["strength"]
                links.new(tex.outputs["Color"], normal_map.inputs["Color"])
                links.new(normal_map.outputs["Normal"], node.inputs["Normal"])
        elif kind == "entity":
            self._create_entity(island, operation["entity"], operation["meters_per_unit"])
        elif kind == "relation":
            self._relation(island, operation["relation"])
        elif kind == "animation":
            self._animate(island, operation["animation"], operation["meters_per_unit"])
        elif kind == "transform":
            self._closed(island)
            obj = self.entity(island, operation["entity"])
            check(not obj.animation_data and not obj.constraints, "animated/constrained transform requires an explicit channel edit", "Unsupported")
            self._transform(obj, operation["transform"], operation["meters_per_unit"])
        else:
            raise CommandError("Unsupported", "native authoring operation is not allowlisted")
        self.semantic.changed()
        return {"applied": kind, "island": island}

    def _create_entity(self, island, spec, units):
        data = self.bpy.data
        collection = self.island(island)
        name = self._name(island, spec["id"], data.objects)
        shape = spec["shape"]; kind = shape["kind"]; native = None
        if kind in {"box", "cylinder", "mesh"}:
            if kind == "box":
                x, y, z = [v * units / 2 for v in vec(shape["size"])]
                vertices = [[-x,-y,-z],[x,-y,-z],[x,y,-z],[-x,y,-z],[-x,-y,z],[x,-y,z],[x,y,z],[-x,y,z]]
                faces = [[3,2,1,0],[4,5,6,7],[0,1,5,4],[1,2,6,5],[2,3,7,6],[3,0,4,7]]
            elif kind == "cylinder":
                n = shape["segments"]; check(isinstance(n, int) and not isinstance(n, bool) and 3 <= n <= 128, "cylinder segments")
                r = shape["radius"] * units; h = shape["depth"] * units / 2
                vertices = [[r*math.cos(2*math.pi*i/n), r*math.sin(2*math.pi*i/n), z] for z in [-h,h] for i in range(n)]
                faces = [list(reversed(range(n))), list(range(n,2*n))] + [[i,(i+1)%n,(i+1)%n+n,i+n] for i in range(n)]
            else:
                vertices = [[v * units for v in vec(p)] for p in shape["vertices"]]
                faces = shape["faces"]
            check(len(vertices) <= 8192 and len(faces) <= 8192, "mesh budget")
            check(all(3 <= len(f) <= 128 and len(set(f)) == len(f) and all(isinstance(i,int) and not isinstance(i,bool) and 0 <= i < len(vertices) for i in f) for f in faces), "mesh topology indices")
            native = data.meshes.new(self._name(island, spec["id"], data.meshes))
            native.from_pydata(vertices, [], faces)
            check(not native.validate(verbose=False, clean_customdata=False), "native topology required repair; rejected", "Conflict")
            native.update()
            if kind == "mesh" and shape.get("uv") is not None:
                uv = shape["uv"]; check(len(uv) == len(native.loops), "UV corner count")
                layer = native.uv_layers.new(name="UVMap")
                for i, value in enumerate(uv): layer.data[i].uv = vec(value, 2)
            for material in spec["materials"]: native.materials.append(self.material(island, material))
            native[ISLAND] = island
        elif kind == "mesh_instance":
            source = self.entity(island, shape["source"])
            check(source.type == "MESH" and not spec["materials"] and not spec["modifiers"], "shared instance writes are excluded")
            native = source.data
        elif kind == "mesh_copy":
            source = self.entity(island, shape["source"])
            check(source.type == "MESH", "mesh_copy source must be a managed mesh")
            desired = self._name(island, spec["id"], data.meshes)
            native = source.data.copy()
            native.name = desired
            native[ISLAND] = island
            if spec["materials"]:
                native.materials.clear()
                for material in spec["materials"]:
                    native.materials.append(self.material(island, material))
        elif kind == "curve":
            native = data.curves.new(self._name(island, spec["id"], data.curves), "CURVE")
            native.dimensions = "3D"
            native.resolution_u = 1
            native.fill_mode = "FULL"
            native.extrude = shape["extrude"] * units
            native.bevel_depth = shape["bevel_depth"] * units
            native.bevel_resolution = shape["bevel_resolution"]
            spline = native.splines.new("POLY")
            spline.points.add(len(shape["points"]) - 1)
            for point, co in zip(spline.points, shape["points"]):
                point.co = (*[value * units for value in vec(co)], 1.0)
            spline.use_cyclic_u = shape["cyclic"]
            for material in spec["materials"]:
                native.materials.append(self.material(island, material))
            native[ISLAND] = island
        elif kind == "armature": native = data.armatures.new(self._name(island, spec["id"], data.armatures))
        elif kind == "camera":
            native = data.cameras.new(self._name(island, spec["id"], data.cameras))
            native.lens = shape["lens_mm"]; native.clip_start = shape["clip_start"]*units; native.clip_end = shape["clip_end"]*units
        elif kind == "area_light":
            native = data.lights.new(self._name(island, spec["id"], data.lights), "AREA")
            native.energy = shape["energy_watts"]; native.size = shape["size"]*units; native.color = vec(shape["color"])
        else: check(kind == "empty", "unsupported native entity")

        if native is not None and getattr(getattr(native, "bl_rna", None), "identifier", "") == "Mesh":
            attributes = spec.get("attributes", [])
            shade_smooth = spec.get("shade_smooth", False)
            check(isinstance(shade_smooth, bool), "shade_smooth must be boolean")
            if kind == "mesh_instance":
                check(not attributes and not shade_smooth, "shared instance mesh writes are excluded")
            else:
                for polygon in native.polygons:
                    polygon.use_smooth = shade_smooth
                type_map = {
                    "float": ("FLOAT", "value"),
                    "vector": ("FLOAT_VECTOR", "vector"),
                    "color": ("FLOAT_COLOR", "color"),
                }
                domain_map = {"point": "POINT", "face": "FACE", "corner": "CORNER"}
                check(len(attributes) <= 16, "mesh attribute count budget")
                total = 0
                for attribute in attributes:
                    identity = local_id(attribute["id"])
                    domain = domain_map.get(attribute["domain"])
                    data_kind = attribute["data"]["kind"]
                    check(domain is not None and data_kind in type_map, "mesh attribute type/domain allowlist")
                    data_type, field = type_map[data_kind]
                    attr_name = "SW_attr_" + identity
                    existing = native.attributes.get(attr_name)
                    if existing is not None:
                        check(kind == "mesh_copy", "managed attribute already exists", "Conflict")
                        native.attributes.remove(existing)
                    created = native.attributes.new(name=attr_name, type=data_type, domain=domain)
                    values = attribute["data"]["values"]
                    check(len(values) == len(created.data), "mesh attribute native cardinality mismatch")
                    total += len(values)
                    check(total <= 65536, "mesh attribute total budget", "Unsupported")
                    for item, value in zip(created.data, values):
                        setattr(item, field, float(value) if field == "value" else value)
                native.update()

        obj = data.objects.new(name, native)
        obj[ISLAND] = island; obj[ENTITY] = spec["id"]; obj["sw_display_name"] = spec["name"]
        collection.objects.link(obj)
        self._transform(obj, spec["transform"], units)
        if kind == "armature":
            def create_bones(edit):
                for bone in shape["bones"]:
                    b = edit.new(local_id(bone["id"]))
                    b.head = [v*units for v in vec(bone["head"])]
                    b.tail = [v*units for v in vec(bone["tail"])]
                for bone in shape["bones"]:
                    if bone["parent"] is not None: edit[bone["id"]].parent = edit[bone["parent"]]
            self.semantic._armature_edit(obj, create_bones)
        for i, modifier in enumerate(spec["modifiers"]):
            kind = modifier["kind"]
            types = {"bevel":"BEVEL", "mirror":"MIRROR", "subdivision":"SUBSURF", "array":"ARRAY", "boolean":"BOOLEAN"}
            check(kind in types, "modifier allowlist")
            native_mod = obj.modifiers.new("SW_modifier_"+str(i), types[kind])
            if kind == "bevel": native_mod.width = modifier["width"]*units; native_mod.segments = modifier["segments"]
            elif kind == "mirror": native_mod.use_axis = modifier["axes"]
            elif kind == "subdivision": native_mod.levels = modifier["levels"]; native_mod.render_levels = modifier["levels"]
            elif kind == "array":
                native_mod.count = modifier["count"]; native_mod.use_relative_offset = False; native_mod.use_constant_offset = True
                native_mod.constant_offset_displace = [v*units for v in modifier["offset"]]
            elif kind == "boolean":
                target = self.entity(island, modifier["target"])
                check(target.type == "MESH" and target is not obj, "boolean target must be a different managed mesh")
                native_mod.operand_type = "OBJECT"
                native_mod.object = target
                native_mod.operation = {"difference":"DIFFERENCE", "union":"UNION", "intersect":"INTERSECT"}[modifier["operation"]]
                native_mod.solver = "EXACT"

    def _relation(self, island, relation):
        kind = relation["kind"]
        if kind in {"parent", "bone_parent"}:
            child = self.entity(island, relation["child"])
            parent = self.entity(island, relation["parent"] if kind == "parent" else relation["armature"])
            check(child.parent is None, "native parent already assigned", "Conflict")
            child.parent = parent
            child.matrix_parent_inverse.identity()
            if kind == "bone_parent":
                check(parent.type == "ARMATURE" and relation["bone"] in parent.data.bones, "bone target missing")
                child.parent_type = "BONE"; child.parent_bone = relation["bone"]
        elif kind in {"follow", "look_at"}:
            subject = self.entity(island, relation["subject"]); target = self.entity(island, relation["target"])
            constraint = subject.constraints.new("COPY_LOCATION" if kind == "follow" else "TRACK_TO")
            constraint.target = target; constraint.owner_space = "WORLD"; constraint.target_space = "WORLD"
            if kind == "follow": constraint.use_offset = relation["offset"]
            else: constraint.track_axis = "TRACK_NEGATIVE_Z"; constraint.up_axis = "UP_Y"
        elif kind == "skin":
            mesh = self.entity(island, relation["mesh"]); rig = self.entity(island, relation["armature"])
            check(mesh.type == "MESH" and rig.type == "ARMATURE", "skin types")
            check(len(mesh.vertex_groups) == 0, "vertex groups already exist", "Conflict")
            groups = {}
            for weight in relation["weights"]:
                bone = local_id(weight["bone"]); check(bone in rig.data.bones, "missing weight bone")
                if bone not in groups: groups[bone] = mesh.vertex_groups.new(name=bone)
                groups[bone].add([weight["vertex"]], weight["weight"], "REPLACE")
            modifier = mesh.modifiers.new("SW_skin", "ARMATURE"); modifier.object = rig
        else: raise CommandError("Unsupported", "relationship not allowlisted")

    def _animate(self, island, animation, units):
        rate = animation["rate"]; scene = self.bpy.context.scene
        requested = rate["num"] / rate["den"]
        check(abs(scene.render.fps / scene.render.fps_base - requested) <= 1e-6,
              "managed islands do not silently change the scene frame rate", "Conflict")
        channels = {}
        for channel in animation["channels"]:
            channels.setdefault(channel["entity"], []).append(channel)
        actions = {}
        for entity, rows in channels.items():
            obj = self.entity(island, entity)
            check(
                not obj.animation_data
                or (obj.animation_data.action is None and len(obj.animation_data.nla_tracks) == 0),
                "existing action/NLA binding",
                "Conflict",
            )
            action = self.bpy.data.actions.new(self._name(island, entity, self.bpy.data.actions))
            action[ISLAND] = island
            slot = action.slots.new(id_type="OBJECT", name=obj.name)
            action_strip = action.layers.new(animation["id"]).strips.new(type="KEYFRAME")
            bag = action_strip.channelbag(slot, ensure=True)
            obj.animation_data_create(); obj.animation_data.action = action; obj.animation_data.action_slot = slot
            for channel in rows:
                property_name = {"translation":"location", "rotation":"rotation_euler", "scale":"scale"}[channel["property"]]
                path = property_name
                if channel["bone"] is not None:
                    bone = local_id(channel["bone"]); check(obj.type == "ARMATURE" and bone in obj.pose.bones, "animation target bone")
                    obj.pose.bones[bone].rotation_mode = "XYZ"
                    path = 'pose.bones["'+bone+'"].'+property_name
                for component in range(3):
                    curve = bag.fcurves.new(path, index=component)
                    curve.keyframe_points.add(len(channel["keys"]))
                    for point, key in zip(curve.keyframe_points, channel["keys"]):
                        value = key["value"][component] * (units if channel["property"] == "translation" else 1)
                        point.co = (key["frame"], value); point.interpolation = "LINEAR"
                    curve.update()
            actions[entity] = (obj, action, slot)

        tracks_by_entity = {}
        for track in animation.get("nla_tracks", []):
            tracks_by_entity.setdefault(track["entity"], []).append(track)
        for entity, track_specs in tracks_by_entity.items():
            obj, action, slot = actions[entity]
            # Once arranged through NLA, the Action is no longer also active.
            obj.animation_data.action = None
            for track_spec in track_specs:
                track = obj.animation_data.nla_tracks.new()
                track.name = "SW_NLA_" + local_id(track_spec["id"])
                for strip_spec in track_spec["strips"]:
                    nla_strip = track.strips.new(
                        "SW_NLA_" + local_id(strip_spec["id"]),
                        int(strip_spec["start_frame"]),
                        action,
                    )
                    nla_strip.action_frame_start = float(strip_spec["action_frame_start"])
                    nla_strip.action_frame_end = float(strip_spec["action_frame_end"])
                    nla_strip.repeat = float(strip_spec["repeat"])
                    nla_strip.scale = float(strip_spec["scale"])
                    nla_strip.influence = float(strip_spec["influence"])
                    if hasattr(nla_strip, "action_slot"):
                        nla_strip.action_slot = slot

    def finish(self, island):
        self._closed(island)
        snapshot = self.snapshot(island)
        self.island(island)[MARKER] = snapshot["fingerprint"]
        snapshot["drift"] = False
        return snapshot

    def measure(self, island, evaluated):
        _, objects = self._closed(island)
        depsgraph = None
        if evaluated:
            self.bpy.context.view_layer.update()
            depsgraph = self.bpy.context.evaluated_depsgraph_get()
        rows = []
        collision_meshes = []
        total_triangles = 0
        for obj in objects:
            observed = obj.evaluated_get(depsgraph) if depsgraph is not None else obj
            row = {"entity": obj[ENTITY], "matrix_world": [list(r) for r in observed.matrix_world],
                   "method": "evaluated-depsgraph" if evaluated else "source-rna",
                   "frame": self.bpy.context.scene.frame_current,
                   "mesh_self_intersections": {
                       "verdict":"UNKNOWN",
                       "reason":"pairwise object collision does not establish self-intersection"
                   }}
            if obj.type == "MESH":
                mesh = observed.to_mesh() if evaluated else observed.data
                try:
                    check(len(mesh.vertices) <= 262_144 and len(mesh.polygons) <= 262_144, "evaluated geometry exceeds budget", "Unsupported")
                    coords = [observed.matrix_world @ v.co for v in mesh.vertices]
                    bounds = [[min(p[i] for p in coords), max(p[i] for p in coords)] for i in range(3)] if coords else None
                    row["bounds_world_meters"] = bounds
                    row["vertices"] = len(mesh.vertices); row["polygons"] = len(mesh.polygons)
                    row["attributes"] = managed_mesh_attributes(mesh)
                    row["smooth_polygons"] = sum(1 for polygon in mesh.polygons if polygon.use_smooth)
                    row["normal_digest"] = digest(source_projection_value(
                        [list(polygon.normal) for polygon in mesh.polygons]
                    ))
                    row["normal_method"] = (
                        "evaluated-polygon-normal-v1" if evaluated
                        else "source-polygon-normal-v1"
                    )
                    mesh.calc_loop_triangles()
                    triangles = [
                        [tuple(observed.matrix_world @ mesh.vertices[index].co) for index in triangle.vertices]
                        for triangle in mesh.loop_triangles
                    ]
                    total_triangles += len(triangles)
                    row["triangles"] = len(triangles)
                    collision_meshes.append({
                        "entity": obj[ENTITY],
                        "bounds": bounds,
                        "triangles": triangles,
                    })
                finally:
                    if evaluated: observed.to_mesh_clear()
            rows.append(row)

        collision = {
            "method": "world-triangle-segment-coplanar-v1",
            "frame": self.bpy.context.scene.frame_current,
            "complete": True,
            "pairs": [],
            "unknown_reason": None,
        }
        if len(collision_meshes) > 64:
            collision["complete"] = False
            collision["unknown_reason"] = "mesh object budget exceeds 64"
        elif total_triangles > 20_000:
            collision["complete"] = False
            collision["unknown_reason"] = "triangle budget exceeds 20000"
        else:
            for i, first in enumerate(collision_meshes):
                for second in collision_meshes[i + 1:]:
                    broad = bool(first["bounds"] and second["bounds"] and bounds_overlap(first["bounds"], second["bounds"]))
                    result = {
                        "a": first["entity"],
                        "b": second["entity"],
                        "broad_phase_aabb": broad,
                        "narrow_phase": "NOT_REQUIRED",
                        "triangle_pairs_tested": 0,
                        "verdict": "SEPARATE",
                    }
                    if broad:
                        intersects, tested = narrow_pair(first["triangles"], second["triangles"])
                        result["triangle_pairs_tested"] = tested
                        if intersects is None:
                            result["narrow_phase"] = "UNKNOWN"
                            result["verdict"] = "UNKNOWN"
                            collision["complete"] = False
                            if collision["unknown_reason"] is None:
                                collision["unknown_reason"] = "narrow-phase pair budget or degenerate triangle"
                        elif intersects:
                            result["narrow_phase"] = "INTERSECT"
                            result["verdict"] = "INTERSECT"
                        else:
                            result["narrow_phase"] = "SEPARATE"
                    collision["pairs"].append(result)
        return {
            "island": island,
            "native_session": self.session,
            "method_version": 2,
            "coverage": "single_frame" if evaluated else "source_only",
            "items": rows,
            "total": len(rows),
            "mesh_pair_intersections": collision,
        }

    def persist(self, island, relative):
        collection, objects = self._closed(island)
        snapshot = self.snapshot(island)
        check(not snapshot["drift"], "external edits need reconciliation before save", "StaleReference")
        destination = self.commands.workspace.path(relative, ".blend")
        check(not os.path.lexists(destination), "save-as destination exists", "Conflict")
        handle, temporary = tempfile.mkstemp(prefix=".semwright-authoring-", suffix=".blend", dir=os.path.dirname(destination))
        os.close(handle)
        try:
            # Blender expands indirect dependencies. _closed rejects external object/data/material
            # references; inventory is returned and fresh-process tests must verify the actual file.
            self.bpy.data.libraries.write(temporary, {collection}, path_remap="NONE", fake_user=False, compress=True)
            size = os.stat(temporary).st_size
            check(0 < size <= 33_554_432, "saved artifact exceeds 32 MiB budget", "Unsupported")
            sha = hashlib.sha256(Path(temporary).read_bytes()).hexdigest()
            self.commands.workspace.path(relative, ".blend")
            os.chmod(temporary, 0o600)
            try: os.link(temporary, destination)
            except FileExistsError as error: raise CommandError("Conflict", "save target appeared; preserved") from error
            return {"path":relative,"sha256":sha,"bytes":size,"island":island,"source_fingerprint":snapshot["fingerprint"],
                    "inventory":{"collection":collection.name,"objects":[o.name for o in objects]},
                    "method":"libraries-write-indirect-closure","fresh_process_verified":False}
        finally:
            if os.path.exists(temporary): os.unlink(temporary)

    def reopen(self, relative, expected_sha, island):
        local_id(island)
        check(len(expected_sha) == 64 and all(c in "0123456789abcdef" for c in expected_sha), "artifact digest")
        check(not any(c.get(ISLAND) == island for c in self.bpy.data.collections), "island is already present; use a fresh process", "Conflict")
        path = self.commands.workspace.path(relative, ".blend", existing=True)
        check(os.stat(path).st_size <= 33_554_432, "input artifact budget", "Unsupported")
        check(hashlib.sha256(Path(path).read_bytes()).hexdigest() == expected_sha, "saved bytes changed", "StaleReference")
        # Executed only inside the owner-created --disable-autoexec Blender process. No UI loads,
        # scripts are enabled, or scene callbacks are supplied by the caller.
        with self.bpy.data.libraries.load(path, link=False) as (source, target):
            names = [n for n in source.collections if n.startswith("SW_"+island[:12]+"_")]
            check(len(names) == 1, "artifact collection identity ambiguous", "Conflict")
            target.collections = names
        collection = self.island(island)
        self._closed(island)
        self.bpy.context.scene.collection.children.link(collection)
        self.semantic.changed()
        # Collection membership affects our projection; the caller compares persisted fields and
        # boot identity explicitly. Reopen itself does not certify semantic equivalence.
        return self.snapshot(island)

    def dispatch(self, command, args):
        operations = {
            "snapshot": lambda: self.snapshot(args.get("island")),
            "begin": lambda: self.begin(
                args["fingerprint"], args.get("island"), args.get("allow_drift", False)
            ),
            "apply": lambda: self.apply(args["operation"]),
            "finish": lambda: self.finish(args["island"]),
            "measure": lambda: self.measure(args["island"], args["evaluated"]),
            "persist": lambda: self.persist(args["island"], args["path"]),
            "reopen": lambda: self.reopen(args["path"], args["sha256"], args["island"]),
        }
        check(command in operations, "private native authoring command is not registered", "Unsupported")
        return operations[command]()
