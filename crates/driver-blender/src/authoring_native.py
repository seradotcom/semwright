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

from .validation import CommandError

ISLAND = "sw_authoring_island"
ENTITY = "sw_authoring_entity"
MATERIAL = "sw_authoring_material"
MARKER = "sw_authoring_observed_v1"


def check(ok, message, code="InvalidArgument"):
    if not ok:
        raise CommandError(code, message)


def local_id(value):
    check(isinstance(value, str) and 1 <= len(value) <= 64 and
          all(c.isascii() and (c.isalnum() or c in "_-") for c in value), "invalid authoring ID")
    return value


def digest(value):
    # Native projection algorithm is named separately from A's semwright-json-v1.
    body = json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode()
    check(len(body) <= 16_777_216, "native projection budget", "Unsupported")
    return hashlib.sha256(body).hexdigest()


def vec(value, count=3, maximum=1_000_000.0):
    check(isinstance(value, (list, tuple)) and len(value) == count, "vector shape")
    check(all(not isinstance(v, bool) and isinstance(v, (int, float)) and math.isfinite(v) and abs(v) <= maximum for v in value), "finite vector bounds")
    return list(value)


def close(a, b, tolerance=1e-5):
    return len(a) == len(b) and all(abs(x-y) <= tolerance * max(1.0, abs(x), abs(y)) for x, y in zip(a, b))


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
                 "modifiers": [], "constraints": [], "action": None, "drivers": 0}
        for modifier in obj.modifiers:
            settings = {"name": modifier.name, "type": modifier.type,
                        "show_viewport": modifier.show_viewport, "show_render": modifier.show_render}
            if modifier.type == "BEVEL": settings.update(width=modifier.width, segments=modifier.segments)
            elif modifier.type == "MIRROR": settings.update(axes=list(modifier.use_axis))
            elif modifier.type == "SUBSURF": settings.update(levels=modifier.levels, render_levels=modifier.render_levels)
            elif modifier.type == "ARRAY": settings.update(count=modifier.count, offset=list(modifier.constant_offset_displace), relative=modifier.use_relative_offset)
            elif modifier.type == "ARMATURE": settings.update(target=modifier.object.get(ENTITY) if modifier.object else None)
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
            value["drivers"] = len(obj.animation_data.drivers)
            check(len(obj.animation_data.nla_tracks) == 0, "NLA readback not covered in this increment", "Unsupported")
            value["action"] = self._action(obj.animation_data.action)
        if obj.data:
            value["data_name"] = obj.data.name
            value["data_library"] = obj.data.library is not None
        if obj.type == "MESH":
            mesh = obj.data
            check(len(mesh.polygons) <= 262_144 and len(mesh.loops) <= 1_048_576 and len(mesh.uv_layers) <= 8, "mesh readback bound", "Unsupported")
            check(len(obj.vertex_groups) <= 64, "weight group readback budget", "Unsupported")
            value["geometry_digest"] = digest({"vertices": [list(v.co) for v in mesh.vertices],
                "faces": [[list(p.vertices), p.material_index, p.use_smooth] for p in mesh.polygons],
                "uv": [[[float(x) for x in loop.uv] for loop in layer.data] for layer in mesh.uv_layers],
                "weights": [[[group.group, group.weight] for group in v.groups] for v in mesh.vertices],
                "groups": [group.name for group in obj.vertex_groups]})
            value.update(vertices=len(mesh.vertices), polygons=len(mesh.polygons), uv_layers=len(mesh.uv_layers), data_users=mesh.users)
            value["materials"] = [self._material(m) for m in mesh.materials]
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
        fingerprint = digest({"schema": "blender-source-projection-v1", "items": rows,
                              "scene_units": scene.unit_settings.scale_length,
                              "fps": scene.render.fps, "fps_base": scene.render.fps_base, "frame": scene.frame_current})
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
            check(not obj.animation_data or (len(obj.animation_data.drivers) == 0 and len(obj.animation_data.nla_tracks) == 0), "active drivers/NLA excluded", "PolicyDenied")
            if obj.animation_data and obj.animation_data.action:
                action = obj.animation_data.action
                check(action.library is None and action.get(ISLAND) == island, "external action dependency", "PolicyDenied")
            if obj.data:
                check(obj.data.library is None and obj.data.override_library is None, "linked data excluded", "PolicyDenied")
                if obj.type == "MESH":
                    check(all(o.as_pointer() in pointers for o in self.bpy.data.objects if o.data == obj.data), "shared datablock has an external user", "PolicyDenied")
                    for m in obj.data.materials:
                        check(m and m.get(ISLAND) == island and m.library is None, "foreign material dependency", "PolicyDenied")
                        check(m.use_nodes and all(n.bl_idname in {"ShaderNodeBsdfPrincipled", "ShaderNodeOutputMaterial"} for n in m.node_tree.nodes), "material graph outside managed PBR profile", "PolicyDenied")
                        check(not m.animation_data or len(m.animation_data.drivers) == 0, "material driver excluded", "PolicyDenied")
            for modifier in obj.modifiers:
                check(modifier.type in {"BEVEL", "MIRROR", "SUBSURF", "ARRAY", "ARMATURE"}, "unmanaged modifier", "PolicyDenied")
                if modifier.type == "ARMATURE": check(modifier.object and modifier.object.as_pointer() in pointers, "external armature", "PolicyDenied")
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
        elif kind == "material":
            self.island(island)
            spec = operation["material"]
            mat = data.materials.new(self._name(island, spec["id"], data.materials))
            mat[ISLAND] = island; mat[MATERIAL] = spec["id"]
            mat.diffuse_color = vec(spec["base_color"], 4)
            mat.roughness = spec["roughness"]; mat.metallic = spec["metallic"]; mat.use_nodes = True
            node = mat.node_tree.nodes.get("Principled BSDF")
            check(node is not None, "pinned Principled shader missing", "Unsupported")
            node.inputs["Base Color"].default_value = mat.diffuse_color
            node.inputs["Roughness"].default_value = mat.roughness
            node.inputs["Metallic"].default_value = mat.metallic
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
        elif kind == "mesh_instance":
            source = self.entity(island, shape["source"])
            check(source.type == "MESH" and not spec["materials"] and not spec["modifiers"], "shared instance writes are excluded")
            native = source.data
        elif kind == "armature": native = data.armatures.new(self._name(island, spec["id"], data.armatures))
        elif kind == "camera":
            native = data.cameras.new(self._name(island, spec["id"], data.cameras))
            native.lens = shape["lens_mm"]; native.clip_start = shape["clip_start"]*units; native.clip_end = shape["clip_end"]*units
        elif kind == "area_light":
            native = data.lights.new(self._name(island, spec["id"], data.lights), "AREA")
            native.energy = shape["energy_watts"]; native.size = shape["size"]*units; native.color = vec(shape["color"])
        else: check(kind == "empty", "unsupported native entity")
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
            types = {"bevel":"BEVEL", "mirror":"MIRROR", "subdivision":"SUBSURF", "array":"ARRAY"}
            check(kind in types, "modifier allowlist")
            native_mod = obj.modifiers.new("SW_modifier_"+str(i), types[kind])
            if kind == "bevel": native_mod.width = modifier["width"]*units; native_mod.segments = modifier["segments"]
            elif kind == "mirror": native_mod.use_axis = modifier["axes"]
            elif kind == "subdivision": native_mod.levels = modifier["levels"]; native_mod.render_levels = modifier["levels"]
            elif kind == "array":
                native_mod.count = modifier["count"]; native_mod.use_relative_offset = False; native_mod.use_constant_offset = True
                native_mod.constant_offset_displace = [v*units for v in modifier["offset"]]

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
        for channel in animation["channels"]: channels.setdefault(channel["entity"], []).append(channel)
        for entity, rows in channels.items():
            obj = self.entity(island, entity)
            check(not obj.animation_data or obj.animation_data.action is None, "existing action binding", "Conflict")
            action = self.bpy.data.actions.new(self._name(island, entity, self.bpy.data.actions))
            action[ISLAND] = island
            slot = action.slots.new(id_type="OBJECT", name=obj.name)
            strip = action.layers.new(animation["id"]).strips.new(type="KEYFRAME")
            bag = strip.channelbag(slot, ensure=True)
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
        for obj in objects:
            observed = obj.evaluated_get(depsgraph) if depsgraph is not None else obj
            row = {"entity": obj[ENTITY], "matrix_world": [list(r) for r in observed.matrix_world],
                   "method": "evaluated-depsgraph" if evaluated else "source-rna",
                   "frame": self.bpy.context.scene.frame_current,
                   "mesh_self_intersections": {"verdict":"UNKNOWN", "reason":"no narrow-phase self-intersection method in this increment"}}
            if obj.type == "MESH":
                mesh = observed.to_mesh() if evaluated else observed.data
                try:
                    check(len(mesh.vertices) <= 262_144 and len(mesh.polygons) <= 262_144, "evaluated geometry exceeds budget", "Unsupported")
                    coords = [observed.matrix_world @ v.co for v in mesh.vertices]
                    row["bounds_world_meters"] = [[min(p[i] for p in coords), max(p[i] for p in coords)] for i in range(3)] if coords else None
                    row["vertices"] = len(mesh.vertices); row["polygons"] = len(mesh.polygons)
                    row["triangles_fan_count"] = sum(max(0, len(p.vertices)-2) for p in mesh.polygons)
                finally:
                    if evaluated: observed.to_mesh_clear()
            rows.append(row)
        return {"island": island, "native_session": self.session, "method_version": 1,
                "coverage": "single_frame" if evaluated else "source_only", "items": rows, "total":len(rows)}

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
