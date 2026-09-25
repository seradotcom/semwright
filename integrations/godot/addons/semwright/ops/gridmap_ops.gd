@tool
extends RefCounted

const MAX_CELLS := 4096
const MAX_ITEMS := 512

static func inspect(ctx, args: Dictionary) -> Dictionary:
    var grid = ctx._resolve_node(str(args.get("target", "")))
    if not (grid is GridMap):
        return ctx._error("not_found", "GridMap not found")
    var cells: Array = []
    var used = grid.get_used_cells()
    for pos in used:
        if cells.size() >= MAX_CELLS:
            break
        cells.append({
            "position": [pos.x, pos.y, pos.z],
            "item": grid.get_cell_item(pos),
            "orientation": grid.get_cell_item_orientation(pos),
        })
    return {"stamp":ctx._stamp(),"data":{
        "target":str(args.get("target", "")),
        "mesh_library":"" if grid.mesh_library == null else grid.mesh_library.resource_path,
        "cell_size":[grid.cell_size.x,grid.cell_size.y,grid.cell_size.z],
        "cell_octant_size":grid.cell_octant_size,
        "cell_scale":grid.cell_scale,
        "cell_center_x":grid.cell_center_x,
        "cell_center_y":grid.cell_center_y,
        "cell_center_z":grid.cell_center_z,
        "bake_navigation":grid.bake_navigation,
        "collision_layer":grid.collision_layer,
        "collision_mask":grid.collision_mask,
        "cells":cells,
        "truncated":used.size() > MAX_CELLS,
    }}

static func configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var grid = ctx._resolve_node(str(args.get("target", "")))
    if not (grid is GridMap): return ctx._error("not_found", "GridMap not found")
    var library: MeshLibrary
    if args.has("mesh_library"):
        var path := str(args["mesh_library"])
        if path.is_empty():
            library = null
        elif not ctx._safe_res(path) or not ResourceLoader.exists(path):
            return ctx._error("not_found", "MeshLibrary resource not found")
        else:
            library = ResourceLoader.load(path, "MeshLibrary", ResourceLoader.CACHE_MODE_REUSE) as MeshLibrary
            if library == null: return ctx._error("invalid_argument", "mesh_library must be a MeshLibrary")
    var size = args.get("cell_size")
    if size != null and (not (size is Array) or size.size() != 3):
        return ctx._error("invalid_argument", "cell_size must be a 3D vector")
    if size != null and (float(size[0]) <= 0.0 or float(size[1]) <= 0.0 or float(size[2]) <= 0.0):
        return ctx._error("invalid_argument", "cell_size components must be positive")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("mesh_library"): grid.mesh_library = library
    if size != null:
        grid.cell_size = Vector3(float(size[0]), float(size[1]), float(size[2]))
    if args.has("cell_octant_size"): grid.cell_octant_size = int(args["cell_octant_size"])
    if args.has("cell_scale"): grid.cell_scale = float(args["cell_scale"])
    if args.has("cell_center_x"): grid.cell_center_x = bool(args["cell_center_x"])
    if args.has("cell_center_y"): grid.cell_center_y = bool(args["cell_center_y"])
    if args.has("cell_center_z"): grid.cell_center_z = bool(args["cell_center_z"])
    if args.has("bake_navigation"): grid.bake_navigation = bool(args["bake_navigation"])
    if args.has("collision_layer"): grid.collision_layer = int(args["collision_layer"])
    if args.has("collision_mask"): grid.collision_mask = int(args["collision_mask"])
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure GridMap")

static func cell_set(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var grid = ctx._resolve_node(str(args.get("target", "")))
    var pos = _v3i(args.get("position"))
    if not (grid is GridMap) or pos == null:
        return ctx._error("invalid_argument", "invalid GridMap cell target")
    var item := int(args.get("item", -1))
    var orientation := int(args.get("orientation", 0))
    if orientation < 0 or orientation > 23:
        return ctx._error("invalid_argument", "GridMap orientation must be 0..23")
    if item >= 0 and (grid.mesh_library == null or not grid.mesh_library.get_item_list().has(item)):
        return ctx._error("not_found", "GridMap MeshLibrary item not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    grid.set_cell_item(pos, item, orientation)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", "")), "cell:%d,%d,%d" % [pos.x,pos.y,pos.z]], "Set GridMap cell")

static func cell_erase(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var grid = ctx._resolve_node(str(args.get("target", "")))
    var pos = _v3i(args.get("position"))
    if not (grid is GridMap) or pos == null:
        return ctx._error("invalid_argument", "invalid GridMap cell target")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", "")), "cell:%d,%d,%d" % [pos.x,pos.y,pos.z]], "dry-run")
    grid.set_cell_item(pos, GridMap.INVALID_CELL_ITEM, 0)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", "")), "cell:%d,%d,%d" % [pos.x,pos.y,pos.z]], "Erase GridMap cell")

static func clear(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var grid = ctx._resolve_node(str(args.get("target", "")))
    if not (grid is GridMap): return ctx._error("not_found", "GridMap not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    grid.clear()
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Clear GridMap")

static func library_inspect(ctx, args: Dictionary) -> Dictionary:
    var library = _library(ctx, str(args.get("path", "")))
    if library == null: return ctx._error("not_found", "MeshLibrary not found")
    var rows: Array = []
    var ids = library.get_item_list()
    for id in ids:
        if rows.size() >= MAX_ITEMS: break
        var mesh: Mesh = library.get_item_mesh(id)
        var nav: NavigationMesh = library.get_item_navigation_mesh(id)
        rows.append({
            "id":id,
            "name":library.get_item_name(id),
            "mesh":"" if mesh == null else mesh.resource_path,
            "navigation_mesh":"" if nav == null else nav.resource_path,
            "navigation_layers":library.get_item_navigation_layers(id),
            "shape_count":library.get_item_shapes(id).size() / 2,
        })
    return {"stamp":ctx._stamp(),"data":{
        "path":library.resource_path,
        "items":rows,
        "truncated":ids.size() > MAX_ITEMS,
    }}

static func item_create(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var library = _library(ctx, str(args.get("path", "")))
    if library == null: return ctx._error("not_found", "MeshLibrary not found")
    var id := int(args.get("id", -1))
    if id < 0: id = library.get_last_unused_item_id()
    if library.get_item_list().has(id): return ctx._error("conflict", "MeshLibrary item already exists")
    var resolved = _item_resources(ctx, args)
    if resolved.has("_error"): return resolved
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [library.resource_path, "item:%d" % id], "dry-run")
    library.create_item(id)
    _apply_item(library, id, args, resolved)
    if ResourceSaver.save(library, library.resource_path) != OK:
        return ctx._error("backend_failed", "failed to save MeshLibrary")
    ctx._revision += 1
    return ctx._mutation_result(true, [library.resource_path, "item:%d" % id], "Create MeshLibrary item")

static func item_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var library = _library(ctx, str(args.get("path", "")))
    var id := int(args.get("id", -1))
    if library == null or not library.get_item_list().has(id):
        return ctx._error("not_found", "MeshLibrary item not found")
    var resolved = _item_resources(ctx, args)
    if resolved.has("_error"): return resolved
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [library.resource_path, "item:%d" % id], "dry-run")
    _apply_item(library, id, args, resolved)
    if ResourceSaver.save(library, library.resource_path) != OK:
        return ctx._error("backend_failed", "failed to save MeshLibrary")
    ctx._revision += 1
    return ctx._mutation_result(true, [library.resource_path, "item:%d" % id], "Configure MeshLibrary item")

static func item_remove(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var library = _library(ctx, str(args.get("path", "")))
    var id := int(args.get("id", -1))
    if library == null or not library.get_item_list().has(id):
        return ctx._error("not_found", "MeshLibrary item not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [library.resource_path, "item:%d" % id], "dry-run")
    library.remove_item(id)
    if ResourceSaver.save(library, library.resource_path) != OK:
        return ctx._error("backend_failed", "failed to save MeshLibrary")
    ctx._revision += 1
    return ctx._mutation_result(true, [library.resource_path, "item:%d" % id], "Remove MeshLibrary item")

static func _item_resources(ctx, args: Dictionary) -> Dictionary:
    var out := {"mesh": null, "navigation_mesh": null}
    if args.has("mesh") and not str(args["mesh"]).is_empty():
        var path := str(args["mesh"])
        if not ctx._safe_res(path) or not ResourceLoader.exists(path):
            return ctx._error("not_found", "Mesh resource not found")
        out["mesh"] = ResourceLoader.load(path, "Mesh", ResourceLoader.CACHE_MODE_REUSE) as Mesh
        if out["mesh"] == null: return ctx._error("invalid_argument", "mesh must be a Mesh")
    if args.has("navigation_mesh") and not str(args["navigation_mesh"]).is_empty():
        var nav_path := str(args["navigation_mesh"])
        if not ctx._safe_res(nav_path) or not ResourceLoader.exists(nav_path):
            return ctx._error("not_found", "NavigationMesh resource not found")
        out["navigation_mesh"] = ResourceLoader.load(nav_path, "NavigationMesh", ResourceLoader.CACHE_MODE_REUSE) as NavigationMesh
        if out["navigation_mesh"] == null:
            return ctx._error("invalid_argument", "navigation_mesh must be a NavigationMesh")
    return out

static func _apply_item(library: MeshLibrary, id: int, args: Dictionary, resolved: Dictionary) -> void:
    if args.has("name"): library.set_item_name(id, str(args["name"]))
    if args.has("mesh"): library.set_item_mesh(id, resolved["mesh"])
    if args.has("navigation_mesh"): library.set_item_navigation_mesh(id, resolved["navigation_mesh"])
    if args.has("navigation_layers"): library.set_item_navigation_layers(id, int(args["navigation_layers"]))

static func _library(ctx, path: String) -> MeshLibrary:
    if not ctx._safe_res(path) or not ResourceLoader.exists(path): return null
    return ResourceLoader.load(path, "MeshLibrary", ResourceLoader.CACHE_MODE_REPLACE) as MeshLibrary

static func _v3i(value):
    if not (value is Array) or value.size() != 3: return null
    return Vector3i(int(value[0]), int(value[1]), int(value[2]))

