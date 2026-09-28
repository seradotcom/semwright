@tool
extends RefCounted

# Read-only observations of the edited scene, never the separately running game.
const MAX_SURFACES := 64
const MAX_VERTICES := 250000
const MAX_ELAPSED_MS := 1000
const MAX_RESOURCE_SELECTORS := 8
const MAX_RESOURCE_PROPERTIES := 256

static func observe(ctx, root: Node, node: Node, args: Dictionary) -> Dictionary:
    var include_bounds = args.get("include_vertex_bounds", false)
    if not (include_bounds is bool): return ctx._error("invalid_argument", "include_vertex_bounds must be boolean")
    var data := {"scope":"edited_scene", "owner_path":_relative(root, node.owner),
        "parent_path":null if node == root else _relative(root, node.get_parent())}
    if node is Node3D:
        for name in ["position", "rotation", "scale", "global_position"]:
            var value: Vector3 = node.get(name)
            if not value.is_finite(): return ctx._error("backend_failed", "non-finite Node3D observation")
            data[name] = _vector(value)
        data["rotation_order"] = node.rotation_order
    if node is Light3D:
        # DirectionalLight3D serializes the same native parameter as
        # light_angular_distance; light_size remains an inherited getter.
        var light_size: float = node.light_size
        if not is_finite(light_size): return ctx._error("backend_failed", "non-finite Light3D size observation")
        data["light_size"] = light_size
    if node is AnimationPlayer:
        data["is_playing"] = node.is_playing()
    if node is MeshInstance3D and include_bounds:
        var bounds := vertex_bounds(ctx, node)
        if bounds.has("_error"): return bounds
        data["vertex_bounds"] = bounds
    if args.has("resource_properties"):
        var resources := attached_resources(ctx, node, args["resource_properties"])
        if resources.has("_error"): return resources
        data["resource_properties"] = resources
    return data

static func _relative(root: Node, node: Node):
    if node == null or (node != root and not root.is_ancestor_of(node)): return null
    return "." if node == root else str(root.get_path_to(node))

static func _vector(value: Vector3) -> Array:
    return [value.x, value.y, value.z]

static func vertex_bounds(ctx, node: MeshInstance3D) -> Dictionary:
    var start := Time.get_ticks_msec()
    var mesh: Mesh = node.mesh
    var count := 0
    var surfaces := 0 if mesh == null else mesh.get_surface_count()
    if surfaces > MAX_SURFACES: return ctx._error("resource_exhausted", "mesh surface budget exceeded")
    var minimum := Vector3(INF, INF, INF)
    var maximum := Vector3(-INF, -INF, -INF)
    for surface in range(surfaces):
        if Time.get_ticks_msec() - start > MAX_ELAPSED_MS:
            return ctx._error("resource_exhausted", "mesh readback time budget exceeded")
        # Preflight ArrayMesh before expanding packed arrays. Other Mesh implementations
        # can allocate in their native getter; the deadline is rechecked immediately after it.
        if mesh is ArrayMesh and count + mesh.surface_get_array_len(surface) > MAX_VERTICES:
            return ctx._error("resource_exhausted", "mesh vertex budget exceeded")
        var arrays: Array = mesh.surface_get_arrays(surface)
        if Time.get_ticks_msec() - start > MAX_ELAPSED_MS:
            return ctx._error("resource_exhausted", "mesh readback time budget exceeded")
        if arrays.size() <= Mesh.ARRAY_VERTEX or not (arrays[Mesh.ARRAY_VERTEX] is PackedVector3Array):
            return ctx._error("unsupported", "mesh surface does not expose 3D vertex arrays")
        var vertices: PackedVector3Array = arrays[Mesh.ARRAY_VERTEX]
        if count + vertices.size() > MAX_VERTICES:
            return ctx._error("resource_exhausted", "mesh vertex budget exceeded")
        for vertex in vertices:
            var world: Vector3 = node.global_transform * vertex
            if not world.is_finite(): return ctx._error("backend_failed", "non-finite mesh vertex")
            minimum = minimum.min(world)
            maximum = maximum.max(world)
            count += 1
            if count % 256 == 0 and Time.get_ticks_msec() - start > MAX_ELAPSED_MS:
                return ctx._error("resource_exhausted", "mesh readback time budget exceeded")
    if Time.get_ticks_msec() - start > MAX_ELAPSED_MS:
        return ctx._error("resource_exhausted", "mesh readback time budget exceeded")
    return {"global_min":null if count == 0 else _vector(minimum),
        "global_max":null if count == 0 else _vector(maximum),
        "vertex_count":count,"surface_count":surfaces,"complete":true,
        "space":"global","geometry":"base_mesh_vertices"}

static func attached_resources(ctx, node: Node, selectors) -> Dictionary:
    var started := Time.get_ticks_msec()
    if not (selectors is Array) or selectors.size() > MAX_RESOURCE_SELECTORS:
        return ctx._error("invalid_argument", "resource_properties requires at most 8 direct property names")
    var result := {}
    var stored := {}
    var count := 0
    for property in node.get_property_list():
        if int(property.get("usage", 0)) & PROPERTY_USAGE_STORAGE != 0:
            stored[str(property.get("name", ""))] = property
    for name in selectors:
        if not (name is String) or name.is_empty() or name.length() > 96 or result.has(name):
            return ctx._error("invalid_argument", "invalid/duplicate resource property selector")
        if name == "script" or not stored.has(name) or int(stored[name].get("type", -1)) != TYPE_OBJECT:
            return ctx._error("permission_denied", "selector must be a direct stored Resource property, never script")
        var resource = node.get(name)
        if resource == null:
            var hint := str(stored[name].get("hint_string", ""))
            var klass := str(stored[name].get("class_name", ""))
            if not (ClassDB.class_exists(klass) and ClassDB.is_parent_class(klass, "Resource")) and klass != "Resource" and int(stored[name].get("hint", -1)) != PROPERTY_HINT_RESOURCE_TYPE:
                return ctx._error("invalid_argument", "null selector is not a Resource property: " + hint)
            result[name] = null
            continue
        if not (resource is Resource) or resource is Script:
            return ctx._error("permission_denied", "selected object is not an inspectable Resource")
        var properties := {}
        for property in resource.get_property_list():
            if int(property.get("usage", 0)) & PROPERTY_USAGE_STORAGE == 0: continue
            var property_name := str(property.get("name", ""))
            if property_name == "script": continue
            count += 1
            if count > MAX_RESOURCE_PROPERTIES:
                return ctx._error("resource_exhausted", "attached resource property budget exceeded")
            var encoded = ctx._encode_value(resource.get(property_name))
            if incomplete(encoded): return ctx._error("resource_exhausted", "attached resource value cannot be represented completely")
            properties[property_name] = encoded
            if Time.get_ticks_msec() - started > MAX_ELAPSED_MS:
                return ctx._error("resource_exhausted", "attached resource readback time budget exceeded")
        result[name] = {"class":resource.get_class(),"path":resource.resource_path,"properties":properties}
    if Time.get_ticks_msec() - started > MAX_ELAPSED_MS:
        return ctx._error("resource_exhausted", "attached resource readback time budget exceeded")
    return result

static func incomplete(value) -> bool:
    if value is float and not is_finite(value): return true
    if value is Dictionary:
        if bool(value.get("truncated", false)) or str(value.get("$type", "")) == "Opaque": return true
        for child in value.values():
            if incomplete(child): return true
    elif value is Array:
        for child in value:
            if incomplete(child): return true
    return false
