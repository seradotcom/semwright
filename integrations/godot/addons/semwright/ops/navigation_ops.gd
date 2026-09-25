@tool
extends RefCounted

static func region_inspect(ctx, args: Dictionary) -> Dictionary:
    var region = ctx._resolve_node(str(args.get("target", "")))
    if not (region is NavigationRegion3D):
        return ctx._error("not_found", "NavigationRegion3D not found")
    var mesh_path := ""
    if region.navigation_mesh != null:
        mesh_path = region.navigation_mesh.resource_path
    var bounds: AABB = region.get_bounds()
    return {"stamp":ctx._stamp(),"data":{
        "target":str(args.get("target", "")),
        "enabled":region.enabled,
        "navigation_layers":region.navigation_layers,
        "enter_cost":region.enter_cost,
        "travel_cost":region.travel_cost,
        "use_edge_connections":region.use_edge_connections,
        "navigation_mesh":mesh_path,
        "baking":region.is_baking(),
        "bounds":{
            "position":[bounds.position.x,bounds.position.y,bounds.position.z],
            "size":[bounds.size.x,bounds.size.y,bounds.size.z],
        },
    }}

static func region_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var region = ctx._resolve_node(str(args.get("target", "")))
    if not (region is NavigationRegion3D):
        return ctx._error("not_found", "NavigationRegion3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("enabled"): region.enabled = bool(args["enabled"])
    if args.has("navigation_layers"): region.navigation_layers = int(args["navigation_layers"])
    if args.has("enter_cost"): region.enter_cost = float(args["enter_cost"])
    if args.has("travel_cost"): region.travel_cost = float(args["travel_cost"])
    if args.has("use_edge_connections"): region.use_edge_connections = bool(args["use_edge_connections"])
    if args.has("navigation_mesh"):
        var path = str(args["navigation_mesh"])
        if path.is_empty():
            region.navigation_mesh = null
        elif ctx._safe_res(path) and ResourceLoader.exists(path):
            var mesh = ResourceLoader.load(path, "NavigationMesh", ResourceLoader.CACHE_MODE_REUSE)
            if not (mesh is NavigationMesh): return ctx._error("invalid_argument", "resource is not NavigationMesh")
            region.navigation_mesh = mesh
        else:
            return ctx._error("not_found", "NavigationMesh resource not found")
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure navigation region")

static func region_bake(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var region = ctx._resolve_node(str(args.get("target", "")))
    if not (region is NavigationRegion3D):
        return ctx._error("not_found", "NavigationRegion3D not found")
    if region.is_baking(): return ctx._error("conflict", "navigation region is already baking")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if region.navigation_mesh == null:
        region.navigation_mesh = NavigationMesh.new()
    region.bake_navigation_mesh(false)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Bake navigation mesh")

static func agent_inspect(ctx, args: Dictionary) -> Dictionary:
    var agent = ctx._resolve_node(str(args.get("target", "")))
    if not (agent is NavigationAgent3D):
        return ctx._error("not_found", "NavigationAgent3D not found")
    return {"stamp":ctx._stamp(),"data":{
        "target":str(args.get("target", "")),
        "navigation_layers":agent.navigation_layers,
        "target_position":_v3(agent.target_position),
        "path_desired_distance":agent.path_desired_distance,
        "target_desired_distance":agent.target_desired_distance,
        "path_max_distance":agent.path_max_distance,
        "radius":agent.radius,
        "height":agent.height,
        "max_speed":agent.max_speed,
        "avoidance_enabled":agent.avoidance_enabled,
        "avoidance_layers":agent.avoidance_layers,
        "avoidance_mask":agent.avoidance_mask,
        "avoidance_priority":agent.avoidance_priority,
        "neighbor_distance":agent.neighbor_distance,
        "max_neighbors":agent.max_neighbors,
        "use_3d_avoidance":agent.use_3d_avoidance,
    }}

static func agent_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var agent = ctx._resolve_node(str(args.get("target", "")))
    if not (agent is NavigationAgent3D):
        return ctx._error("not_found", "NavigationAgent3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("navigation_layers"): agent.navigation_layers = int(args["navigation_layers"])
    if args.has("target_position"):
        var target = _array_v3(args["target_position"])
        if target == null: return ctx._error("invalid_argument", "target_position must be a 3-number array")
        agent.target_position = target
    for key in ["path_desired_distance","target_desired_distance","path_max_distance","radius","height","max_speed","avoidance_priority","neighbor_distance"]:
        if args.has(key): agent.set(key, float(args[key]))
    if args.has("max_neighbors"): agent.max_neighbors = int(args["max_neighbors"])
    if args.has("avoidance_enabled"): agent.avoidance_enabled = bool(args["avoidance_enabled"])
    if args.has("avoidance_layers"): agent.avoidance_layers = int(args["avoidance_layers"])
    if args.has("avoidance_mask"): agent.avoidance_mask = int(args["avoidance_mask"])
    if args.has("use_3d_avoidance"): agent.use_3d_avoidance = bool(args["use_3d_avoidance"])
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure navigation agent")

static func link_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var link = ctx._resolve_node(str(args.get("target", "")))
    if not (link is NavigationLink3D):
        return ctx._error("not_found", "NavigationLink3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("enabled"): link.enabled = bool(args["enabled"])
    if args.has("bidirectional"): link.bidirectional = bool(args["bidirectional"])
    if args.has("navigation_layers"): link.navigation_layers = int(args["navigation_layers"])
    if args.has("enter_cost"): link.enter_cost = float(args["enter_cost"])
    if args.has("travel_cost"): link.travel_cost = float(args["travel_cost"])
    if args.has("start_position"):
        var start = _array_v3(args["start_position"])
        if start == null: return ctx._error("invalid_argument", "start_position must be a 3-number array")
        link.start_position = start
    if args.has("end_position"):
        var end = _array_v3(args["end_position"])
        if end == null: return ctx._error("invalid_argument", "end_position must be a 3-number array")
        link.end_position = end
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure navigation link")

static func _array_v3(value):
    if not (value is Array) or value.size() != 3: return null
    return Vector3(float(value[0]),float(value[1]),float(value[2]))

static func _v3(value: Vector3) -> Array:
    return [value.x,value.y,value.z]
