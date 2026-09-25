@tool
extends RefCounted

const MAX_CELLS := 4096

static func inspect(ctx, args: Dictionary) -> Dictionary:
    var layer = ctx._resolve_node(str(args.get("target", "")))
    if not (layer is TileMapLayer):
        return ctx._error("not_found", "TileMapLayer not found")
    var cells: Array = []
    for coords in layer.get_used_cells():
        if cells.size() >= MAX_CELLS:
            break
        cells.append({
            "coords":[coords.x, coords.y],
            "source_id":layer.get_cell_source_id(coords),
            "atlas_coords":[layer.get_cell_atlas_coords(coords).x, layer.get_cell_atlas_coords(coords).y],
            "alternative":layer.get_cell_alternative_tile(coords),
        })
    var rect: Rect2i = layer.get_used_rect()
    var tile_set_path := ""
    if layer.tile_set != null:
        tile_set_path = layer.tile_set.resource_path
    return {"stamp":ctx._stamp(),"data":{
        "target":str(args.get("target", "")),
        "tile_set":tile_set_path,
        "used_rect":[rect.position.x,rect.position.y,rect.size.x,rect.size.y],
        "cells":cells,
        "truncated":layer.get_used_cells().size() > MAX_CELLS,
    }}

static func cell_set(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var layer = ctx._resolve_node(str(args.get("target", "")))
    if not (layer is TileMapLayer): return ctx._error("not_found", "TileMapLayer not found")
    var coords = _v2i(args.get("coords"))
    var atlas = _v2i(args.get("atlas_coords"))
    if coords == null or atlas == null: return ctx._error("invalid_argument", "tile coordinates must be integer pairs")
    if abs(coords.x) > 32767 or abs(coords.y) > 32767:
        return ctx._error("invalid_argument", "TileMapLayer coordinates exceed serialized range")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    layer.set_cell(coords, int(args.get("source_id", -1)), atlas, int(args.get("alternative", 0)))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", "")), "cell:%d,%d" % [coords.x,coords.y]], "Set tile cell")

static func cell_erase(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var layer = ctx._resolve_node(str(args.get("target", "")))
    var coords = _v2i(args.get("coords"))
    if not (layer is TileMapLayer) or coords == null: return ctx._error("invalid_argument", "invalid TileMapLayer cell")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    layer.erase_cell(coords)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", "")), "cell:%d,%d" % [coords.x,coords.y]], "Erase tile cell")

static func clear(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var layer = ctx._resolve_node(str(args.get("target", "")))
    if not (layer is TileMapLayer): return ctx._error("not_found", "TileMapLayer not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    layer.clear()
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Clear tile map")

static func tileset_inspect(ctx, args: Dictionary) -> Dictionary:
    var tile_set = _tileset(ctx, str(args.get("path", "")))
    if tile_set == null: return ctx._error("not_found", "TileSet not found")
    var ids: Array = []
    for i in tile_set.get_source_count():
        ids.append(tile_set.get_source_id(i))
    return {"stamp":ctx._stamp(),"data":{
        "path":tile_set.resource_path,
        "tile_size":[tile_set.tile_size.x,tile_set.tile_size.y],
        "tile_shape":tile_set.tile_shape,
        "tile_layout":tile_set.tile_layout,
        "tile_offset_axis":tile_set.tile_offset_axis,
        "uv_clipping":tile_set.uv_clipping,
        "source_ids":ids,
        "physics_layers":tile_set.get_physics_layers_count(),
        "navigation_layers":tile_set.get_navigation_layers_count(),
    }}

static func tileset_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var tile_set = _tileset(ctx, str(args.get("path", "")))
    if tile_set == null: return ctx._error("not_found", "TileSet not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [tile_set.resource_path], "dry-run")
    if args.has("tile_size"):
        var size = _v2i(args["tile_size"])
        if size == null or size.x <= 0 or size.y <= 0: return ctx._error("invalid_argument", "tile_size must be positive")
        tile_set.tile_size = size
    if args.has("tile_shape"): tile_set.tile_shape = int(args["tile_shape"])
    if args.has("tile_layout"): tile_set.tile_layout = int(args["tile_layout"])
    if args.has("tile_offset_axis"): tile_set.tile_offset_axis = int(args["tile_offset_axis"])
    if args.has("uv_clipping"): tile_set.uv_clipping = bool(args["uv_clipping"])
    if ResourceSaver.save(tile_set, tile_set.resource_path) != OK:
        return ctx._error("backend_failed", "failed to save TileSet")
    ctx._revision += 1
    return ctx._mutation_result(true, [tile_set.resource_path], "Configure TileSet")

static func atlas_create(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var tile_set = _tileset(ctx, str(args.get("path", "")))
    var texture_path = str(args.get("texture", ""))
    if tile_set == null or not ctx._safe_res(texture_path) or not ResourceLoader.exists(texture_path):
        return ctx._error("not_found", "TileSet or atlas texture not found")
    var texture = ResourceLoader.load(texture_path, "Texture2D", ResourceLoader.CACHE_MODE_REUSE)
    if not (texture is Texture2D): return ctx._error("invalid_argument", "atlas texture must be Texture2D")
    var region = _v2i(args.get("region_size"))
    if region == null or region.x <= 0 or region.y <= 0: return ctx._error("invalid_argument", "region_size must be positive")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [tile_set.resource_path,texture_path], "dry-run")
    var source := TileSetAtlasSource.new()
    source.texture = texture
    source.texture_region_size = region
    if args.has("margins"):
        var margins = _v2i(args["margins"])
        if margins == null: return ctx._error("invalid_argument", "margins must be an integer pair")
        source.margins = margins
    if args.has("separation"):
        var separation = _v2i(args["separation"])
        if separation == null: return ctx._error("invalid_argument", "separation must be an integer pair")
        source.separation = separation
    var source_id = tile_set.add_source(source, int(args.get("source_id", -1)))
    if source_id < 0: return ctx._error("conflict", "failed to add TileSet atlas source")
    if ResourceSaver.save(tile_set, tile_set.resource_path) != OK:
        return ctx._error("backend_failed", "failed to save TileSet")
    ctx._revision += 1
    return ctx._mutation_result(true, [tile_set.resource_path,"source:%d" % source_id], "Create TileSet atlas source")

static func tile_create(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var tile_set = _tileset(ctx, str(args.get("path", "")))
    if tile_set == null: return ctx._error("not_found", "TileSet not found")
    var source = tile_set.get_source(int(args.get("source_id", -1)))
    if not (source is TileSetAtlasSource): return ctx._error("not_found", "TileSet atlas source not found")
    var coords = _v2i(args.get("atlas_coords"))
    var size = _v2i(args.get("size", [1,1]))
    if coords == null or size == null or size.x <= 0 or size.y <= 0:
        return ctx._error("invalid_argument", "invalid atlas tile coordinates or size")
    if source.has_tile(coords): return ctx._error("conflict", "atlas tile already exists")
    if not source.has_room_for_tile(coords, size, 1, Vector2i.ZERO, 1):
        return ctx._error("conflict", "atlas tile does not fit")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [tile_set.resource_path], "dry-run")
    source.create_tile(coords, size)
    if ResourceSaver.save(tile_set, tile_set.resource_path) != OK:
        return ctx._error("backend_failed", "failed to save TileSet")
    ctx._revision += 1
    return ctx._mutation_result(true, [tile_set.resource_path,"tile:%d,%d" % [coords.x,coords.y]], "Create atlas tile")

static func _tileset(ctx, path: String) -> TileSet:
    if not ctx._safe_res(path) or not ResourceLoader.exists(path):
        return null
    return ResourceLoader.load(path, "TileSet", ResourceLoader.CACHE_MODE_REPLACE) as TileSet

static func _v2i(value):
    if not (value is Array) or value.size() != 2:
        return null
    return Vector2i(int(value[0]), int(value[1]))
