@tool
extends RefCounted

const MAX_BONES := 512

static func inspect(ctx, args: Dictionary) -> Dictionary:
    var skeleton = ctx._resolve_node(str(args.get("target", "")))
    if not (skeleton is Skeleton3D): return ctx._error("not_found", "Skeleton3D not found")
    var bones: Array = []
    var count = min(skeleton.get_bone_count(), MAX_BONES)
    for i in count:
        var rotation: Quaternion = skeleton.get_bone_pose_rotation(i)
        bones.append({
            "index":i,
            "name":skeleton.get_bone_name(i),
            "parent":skeleton.get_bone_parent(i),
            "enabled":skeleton.is_bone_enabled(i),
            "position":_v3(skeleton.get_bone_pose_position(i)),
            "rotation":[rotation.x,rotation.y,rotation.z,rotation.w],
            "scale":_v3(skeleton.get_bone_pose_scale(i)),
            "children":Array(skeleton.get_bone_children(i)),
        })
    return {"stamp":ctx._stamp(),"data":{
        "target":str(args.get("target", "")),
        "bone_count":skeleton.get_bone_count(),
        "bones":bones,
        "truncated":skeleton.get_bone_count() > MAX_BONES,
    }}

static func bone_add(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var skeleton = ctx._resolve_node(str(args.get("target", "")))
    if not (skeleton is Skeleton3D): return ctx._error("not_found", "Skeleton3D not found")
    var name = str(args.get("name", ""))
    if name.is_empty() or name.length() > 96 or skeleton.find_bone(name) >= 0:
        return ctx._error("conflict", "invalid or duplicate bone name")
    if skeleton.get_bone_count() >= MAX_BONES: return ctx._error("invalid_argument", "bone limit reached")
    var parent = int(args.get("parent", -1))
    if parent >= skeleton.get_bone_count(): return ctx._error("invalid_argument", "bone parent is out of range")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", "")),name], "dry-run")
    var index = skeleton.add_bone(name)
    if index < 0: return ctx._error("backend_failed", "failed to add bone")
    if parent >= 0: skeleton.set_bone_parent(index,parent)
    if args.has("position"):
        var position = _array_v3(args["position"])
        if position == null: return ctx._error("invalid_argument", "position must be a 3-number array")
        skeleton.set_bone_pose_position(index,position)
    if args.has("rotation"):
        var rotation = _array_quat(args["rotation"])
        if rotation == null: return ctx._error("invalid_argument", "rotation must be a quaternion array")
        skeleton.set_bone_pose_rotation(index,rotation.normalized())
    if args.has("scale"):
        var scale = _array_v3(args["scale"])
        if scale == null: return ctx._error("invalid_argument", "scale must be a 3-number array")
        skeleton.set_bone_pose_scale(index,scale)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", "")),"bone:%d" % index], "Add skeleton bone")

static func bone_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var skeleton = ctx._resolve_node(str(args.get("target", "")))
    if not (skeleton is Skeleton3D): return ctx._error("not_found", "Skeleton3D not found")
    var index = _bone_index(skeleton,args)
    if index < 0: return ctx._error("not_found", "bone not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", "")),"bone:%d" % index], "dry-run")
    if args.has("name"):
        var name = str(args["name"])
        var existing = skeleton.find_bone(name)
        if name.is_empty() or (existing >= 0 and existing != index): return ctx._error("conflict", "invalid or duplicate bone name")
        skeleton.set_bone_name(index,name)
    if args.has("parent"):
        var parent = int(args["parent"])
        if parent >= index: return ctx._error("invalid_argument", "bone parent must precede child")
        skeleton.set_bone_parent(index,parent)
    if args.has("enabled"): skeleton.set_bone_enabled(index,bool(args["enabled"]))
    if args.has("position"):
        var position = _array_v3(args["position"])
        if position == null: return ctx._error("invalid_argument", "position must be a 3-number array")
        skeleton.set_bone_pose_position(index,position)
    if args.has("rotation"):
        var rotation = _array_quat(args["rotation"])
        if rotation == null: return ctx._error("invalid_argument", "rotation must be a quaternion array")
        skeleton.set_bone_pose_rotation(index,rotation.normalized())
    if args.has("scale"):
        var scale = _array_v3(args["scale"])
        if scale == null: return ctx._error("invalid_argument", "scale must be a 3-number array")
        skeleton.set_bone_pose_scale(index,scale)
    if bool(args.get("reset_pose", false)): skeleton.reset_bone_pose(index)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", "")),"bone:%d" % index], "Configure skeleton bone")

static func attachment_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var attachment = ctx._resolve_node(str(args.get("target", "")))
    if not (attachment is BoneAttachment3D): return ctx._error("not_found", "BoneAttachment3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("bone_name"): attachment.bone_name = str(args["bone_name"])
    if args.has("bone_idx"): attachment.bone_idx = int(args["bone_idx"])
    if args.has("override_pose"): attachment.override_pose = bool(args["override_pose"])
    if args.has("use_external_skeleton"): attachment.use_external_skeleton = bool(args["use_external_skeleton"])
    if args.has("external_skeleton"): attachment.external_skeleton = NodePath(str(args["external_skeleton"]))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure bone attachment")

static func _bone_index(skeleton: Skeleton3D, args: Dictionary) -> int:
    if args.has("index"): return int(args["index"])
    if args.has("bone"): return skeleton.find_bone(str(args["bone"]))
    return -1

static func _array_v3(value):
    if not (value is Array) or value.size() != 3: return null
    return Vector3(float(value[0]),float(value[1]),float(value[2]))

static func _array_quat(value):
    if not (value is Array) or value.size() != 4: return null
    var q := Quaternion(float(value[0]),float(value[1]),float(value[2]),float(value[3]))
    if q.length_squared() <= 0.000001: return null
    return q

static func _v3(value: Vector3) -> Array:
    return [value.x,value.y,value.z]
