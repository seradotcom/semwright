"""Additional preflight for the existing GLB exporter; this is not an exporter.

Conservative closure: unsupported native references are rejected rather than silently
claiming that Blender's use_selection is the complete export boundary.
"""
from pathlib import Path
from .validation import CommandError

SAFE_MODIFIERS = {"BEVEL", "MIRROR", "SUBSURF", "ARRAY", "ARMATURE", "BOOLEAN", "WEIGHTED_NORMAL", "TRIANGULATE"}
SAFE_CONSTRAINTS = {"COPY_LOCATION", "COPY_ROTATION", "COPY_SCALE", "COPY_TRANSFORMS", "TRACK_TO",
                    "DAMPED_TRACK", "LOCKED_TRACK", "LIMIT_LOCATION", "LIMIT_ROTATION", "LIMIT_SCALE"}
SAFE_SHADERS = {"ShaderNodeBsdfPrincipled", "ShaderNodeOutputMaterial", "ShaderNodeTexImage",
                "ShaderNodeNormalMap", "ShaderNodeBump", "ShaderNodeTexCoord", "ShaderNodeUVMap",
                "ShaderNodeMapping", "ShaderNodeMath", "ShaderNodeVectorMath", "ShaderNodeMixRGB",
                "ShaderNodeMix", "ShaderNodeRGB", "ShaderNodeValue", "ShaderNodeValToRGB",
                "ShaderNodeSeparateColor", "ShaderNodeCombineColor", "ShaderNodeSeparateXYZ",
                "ShaderNodeCombineXYZ", "NodeReroute"}


def require(ok, message):
    if not ok:
        raise CommandError("PolicyDenied", message)


def inactive(block):
    require(block.library is None and block.override_library is None, "linked/override dependency needs an explicit trusted export plan")
    animation = getattr(block, "animation_data", None)
    require(animation is None or len(animation.drivers) == 0, "active driver dependency excluded from export")


def inspect_export_closure(bpy, workspace, collection, animations):
    inactive(collection)
    objects = list(collection.all_objects)
    require(0 < len(objects) <= 2048, "export object budget")
    selected = {obj.as_pointer() for obj in objects}
    materials = {}; actions = {}; images = {}; vertex_count = 0
    for obj in objects:
        inactive(obj)
        require(not obj.is_instancer and obj.instance_collection is None, "collection/vertex instancer excluded from export")
        require(obj.type in {"MESH", "ARMATURE", "EMPTY"}, "unsupported object type in export closure")
        require(obj.parent is None or obj.parent.as_pointer() in selected, "parent outside export closure")
        if obj.data:
            inactive(obj.data)
        if obj.type == "MESH":
            vertex_count += len(obj.data.vertices)
            require(vertex_count <= 1_048_576, "export source geometry budget")
            require(len(obj.data.materials) <= 64, "export material slot budget")
            for material in obj.data.materials:
                if material: materials[material.as_pointer()] = material
        require(len(obj.modifiers) <= 64 and len(obj.constraints) <= 64, "export modifier/constraint budget")
        for modifier in obj.modifiers:
            require(modifier.type in SAFE_MODIFIERS, "modifier dependencies have no export closure method")
            if modifier.type == "ARMATURE":
                require(modifier.object is None or modifier.object.as_pointer() in selected, "armature outside export closure")
            elif modifier.type == "MIRROR":
                require(modifier.mirror_object is None or modifier.mirror_object.as_pointer() in selected, "mirror target outside export closure")
            elif modifier.type == "ARRAY":
                for dependency in [modifier.offset_object, modifier.start_cap, modifier.end_cap]:
                    require(dependency is None or dependency.as_pointer() in selected, "array target outside export closure")
            elif modifier.type == "BOOLEAN":
                require(
                    modifier.operand_type == "OBJECT"
                    and modifier.solver == "EXACT"
                    and modifier.object is not None
                    and modifier.object.as_pointer() in selected,
                    "boolean target/solver outside managed export closure",
                )
        owners = [obj]
        if obj.type == "ARMATURE":
            require(len(obj.pose.bones) <= 512, "export bone budget")
            owners.extend(list(obj.pose.bones))
        for owner in owners:
            for constraint in owner.constraints:
                require(constraint.type in SAFE_CONSTRAINTS, "constraint dependencies have no export closure method")
                target = getattr(constraint, "target", None)
                require(target is None or target.as_pointer() in selected, "constraint target outside export closure")
        animation = obj.animation_data
        if animation:
            require(len(animation.nla_tracks) <= 16, "NLA export track budget")
            if animation.action:
                inactive(animation.action)
                actions[animation.action.as_pointer()] = animation.action
            total_strips = 0
            for track in animation.nla_tracks:
                require(len(track.strips) <= 16, "NLA export strip budget")
                for strip in track.strips:
                    total_strips += 1
                    require(total_strips <= 64, "NLA export total strip budget")
                    require(strip.action is not None, "NLA strip is missing its Action")
                    inactive(strip.action)
                    actions[strip.action.as_pointer()] = strip.action
    require(len(materials) <= 256, "export distinct material budget")
    for material in materials.values():
        inactive(material)
        if not material.use_nodes:
            continue
        tree = material.node_tree
        inactive(tree)
        require(len(tree.nodes) <= 128 and len(tree.links) <= 256, "export shader graph budget")
        for node in tree.nodes:
            require(node.bl_idname in SAFE_SHADERS, "shader/group has no closed typed export method")
            if node.bl_idname == "ShaderNodeTexImage" and node.image:
                image = node.image; inactive(image)
                require(image.source in {"FILE", "GENERATED"}, "texture source is not a bounded still image")
                if image.packed_file:
                    require(image.packed_file.size <= 67_108_864, "packed image budget")
                elif image.source == "FILE":
                    # Workspace.path checks every path component with lstat, not resolve-and-trust.
                    absolute = Path(bpy.path.abspath(image.filepath))
                    try: relative = absolute.relative_to(workspace.root).as_posix()
                    except ValueError as error: raise CommandError("PolicyDenied", "texture outside granted workspace") from error
                    require(absolute.suffix.lower() in {".png", ".jpg", ".jpeg", ".exr", ".tif", ".tiff", ".tga", ".bmp"}, "texture codec outside closed export scope")
                    path = Path(workspace.path(relative, absolute.suffix.lower(), existing=True))
                    require(path.stat().st_size <= 67_108_864, "external image budget")
                images[image.as_pointer()] = image
    require(len(images) <= 64, "export texture count budget")
    if animations:
        # ACTIONS mode may inspect globally available actions. Until a more selective pinned
        # exporter contract is verified, do not let unrelated actions enter effective membership.
        require(len(bpy.data.actions) <= 1024, "global action preflight budget")
        require(all(action.as_pointer() in actions for action in bpy.data.actions), "unselected action may expand ACTIONS export membership")
    return {"objects": sorted(o.name for o in objects), "materials": sorted(m.name for m in materials.values()),
            "actions": sorted(a.name for a in actions.values()), "images": sorted(i.name for i in images.values()),
            "method": "bounded-glb-dependency-preflight-v1", "scope": "declared native references only"}
