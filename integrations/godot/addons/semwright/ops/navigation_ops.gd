@tool
extends RefCounted

static func region_inspect(ctx, args: Dictionary) -> Dictionary:
    var region = ctx._resolve_node(str(args.get("target", "")))
    if region is NavigationRegion3D:
        var mesh_path := ""
        if region.navigation_mesh != null:
            mesh_path = region.navigation_mesh.resource_path
        var bounds: AABB = region.get_bounds()
        return {"stamp":ctx._stamp(),"data":{
            "target":str(args.get("target", "")),
            "dimension":"3d",
            "enabled":region.enabled,
            "navigation_layers":region.navigation_layers,
            "enter_cost":region.enter_cost,
            "travel_cost":region.travel_cost,
            "use_edge_connections":region.use_edge_connections,
            "navigation_resource":mesh_path,
            "navigation_mesh":mesh_path,
            "baking":region.is_baking(),
            "bounds":{"position":_v3(bounds.position),"size":_v3(bounds.size)},
        }}
    if region is NavigationRegion2D:
        var polygon_path := ""
        if region.navigation_polygon != null:
            polygon_path = region.navigation_polygon.resource_path
        var bounds2: Rect2 = region.get_bounds()
        return {"stamp":ctx._stamp(),"data":{
            "target":str(args.get("target", "")),
            "dimension":"2d",
            "enabled":region.enabled,
            "navigation_layers":region.navigation_layers,
            "enter_cost":region.enter_cost,
            "travel_cost":region.travel_cost,
            "use_edge_connections":region.use_edge_connections,
            "navigation_resource":polygon_path,
            "navigation_polygon":polygon_path,
            "baking":region.is_baking(),
            "bounds":{"position":_v2(bounds2.position),"size":_v2(bounds2.size)},
        }}
    return ctx._error("not_found", "NavigationRegion2D or NavigationRegion3D not found")

static func region_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var region = ctx._resolve_node(str(args.get("target", "")))
    if not (region is NavigationRegion2D) and not (region is NavigationRegion3D):
        return ctx._error("not_found", "NavigationRegion2D or NavigationRegion3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("enabled"): region.enabled = bool(args["enabled"])
    if args.has("navigation_layers"): region.navigation_layers = int(args["navigation_layers"])
    if args.has("enter_cost"): region.enter_cost = float(args["enter_cost"])
    if args.has("travel_cost"): region.travel_cost = float(args["travel_cost"])
    if args.has("use_edge_connections"): region.use_edge_connections = bool(args["use_edge_connections"])
    if region is NavigationRegion3D:
        if args.has("navigation_polygon"):
            return ctx._error("invalid_argument", "NavigationRegion3D does not use navigation_polygon")
        if args.has("navigation_mesh"):
            var path3 := str(args["navigation_mesh"])
            if path3.is_empty():
                region.navigation_mesh = null
            elif ctx._safe_res(path3) and ResourceLoader.exists(path3):
                var mesh = ResourceLoader.load(path3, "NavigationMesh", ResourceLoader.CACHE_MODE_REUSE)
                if not (mesh is NavigationMesh): return ctx._error("invalid_argument", "resource is not NavigationMesh")
                region.navigation_mesh = mesh
            else:
                return ctx._error("not_found", "NavigationMesh resource not found")
    else:
        if args.has("navigation_mesh"):
            return ctx._error("invalid_argument", "NavigationRegion2D does not use navigation_mesh")
        if args.has("navigation_polygon"):
            var path2 := str(args["navigation_polygon"])
            if path2.is_empty():
                region.navigation_polygon = null
            elif ctx._safe_res(path2) and ResourceLoader.exists(path2):
                var polygon = ResourceLoader.load(path2, "NavigationPolygon", ResourceLoader.CACHE_MODE_REUSE)
                if not (polygon is NavigationPolygon): return ctx._error("invalid_argument", "resource is not NavigationPolygon")
                region.navigation_polygon = polygon
            else:
                return ctx._error("not_found", "NavigationPolygon resource not found")
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure navigation region")

static func region_bake(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var region = ctx._resolve_node(str(args.get("target", "")))
    if not (region is NavigationRegion2D) and not (region is NavigationRegion3D):
        return ctx._error("not_found", "NavigationRegion2D or NavigationRegion3D not found")
    if region.is_baking(): return ctx._error("conflict", "navigation region is already baking")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if region is NavigationRegion3D:
        if region.navigation_mesh == null:
            region.navigation_mesh = NavigationMesh.new()
        region.bake_navigation_mesh(false)
    else:
        if region.navigation_polygon == null:
            region.navigation_polygon = NavigationPolygon.new()
        region.bake_navigation_polygon(false)
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Bake navigation region")

static func agent_inspect(ctx, args: Dictionary) -> Dictionary:
    var agent = ctx._resolve_node(str(args.get("target", "")))
    if agent is NavigationAgent3D:
        return {"stamp":ctx._stamp(),"data":_agent_data_3d(agent, str(args.get("target", "")))}
    if agent is NavigationAgent2D:
        return {"stamp":ctx._stamp(),"data":_agent_data_2d(agent, str(args.get("target", "")))}
    return ctx._error("not_found", "NavigationAgent2D or NavigationAgent3D not found")

static func agent_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var agent = ctx._resolve_node(str(args.get("target", "")))
    if not (agent is NavigationAgent2D) and not (agent is NavigationAgent3D):
        return ctx._error("not_found", "NavigationAgent2D or NavigationAgent3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("navigation_layers"): agent.navigation_layers = int(args["navigation_layers"])
    if args.has("target_position"):
        if agent is NavigationAgent3D:
            var target3 = _array_v3(args["target_position"])
            if target3 == null: return ctx._error("invalid_argument", "3D target_position must contain three numbers")
            agent.target_position = target3
        else:
            var target2 = _array_v2(args["target_position"])
            if target2 == null: return ctx._error("invalid_argument", "2D target_position must contain two numbers")
            agent.target_position = target2
    for key in ["path_desired_distance","target_desired_distance","path_max_distance","radius","max_speed","avoidance_priority","neighbor_distance","time_horizon_agents","time_horizon_obstacles"]:
        if args.has(key): agent.set(key, float(args[key]))
    if args.has("max_neighbors"): agent.max_neighbors = int(args["max_neighbors"])
    if args.has("avoidance_enabled"): agent.avoidance_enabled = bool(args["avoidance_enabled"])
    if args.has("avoidance_layers"): agent.avoidance_layers = int(args["avoidance_layers"])
    if args.has("avoidance_mask"): agent.avoidance_mask = int(args["avoidance_mask"])
    if agent is NavigationAgent3D:
        if args.has("height"): agent.height = float(args["height"])
        if args.has("use_3d_avoidance"): agent.use_3d_avoidance = bool(args["use_3d_avoidance"])
    elif args.has("height") or args.has("use_3d_avoidance"):
        return ctx._error("invalid_argument", "height/use_3d_avoidance are only valid for NavigationAgent3D")
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure navigation agent")

static func link_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var link = ctx._resolve_node(str(args.get("target", "")))
    if not (link is NavigationLink2D) and not (link is NavigationLink3D):
        return ctx._error("not_found", "NavigationLink2D or NavigationLink3D not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("enabled"): link.enabled = bool(args["enabled"])
    if args.has("bidirectional"): link.bidirectional = bool(args["bidirectional"])
    if args.has("navigation_layers"): link.navigation_layers = int(args["navigation_layers"])
    if args.has("enter_cost"): link.enter_cost = float(args["enter_cost"])
    if args.has("travel_cost"): link.travel_cost = float(args["travel_cost"])
    if args.has("start_position"):
        if link is NavigationLink3D:
            var start3 = _array_v3(args["start_position"])
            if start3 == null: return ctx._error("invalid_argument", "3D start_position must contain three numbers")
            link.start_position = start3
        else:
            var start2 = _array_v2(args["start_position"])
            if start2 == null: return ctx._error("invalid_argument", "2D start_position must contain two numbers")
            link.start_position = start2
    if args.has("end_position"):
        if link is NavigationLink3D:
            var end3 = _array_v3(args["end_position"])
            if end3 == null: return ctx._error("invalid_argument", "3D end_position must contain three numbers")
            link.end_position = end3
        else:
            var end2 = _array_v2(args["end_position"])
            if end2 == null: return ctx._error("invalid_argument", "2D end_position must contain two numbers")
            link.end_position = end2
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure navigation link")

static func _agent_data_3d(agent: NavigationAgent3D, target: String) -> Dictionary:
    return {
        "target":target,"dimension":"3d","navigation_layers":agent.navigation_layers,
        "target_position":_v3(agent.target_position),"path_desired_distance":agent.path_desired_distance,
        "target_desired_distance":agent.target_desired_distance,"path_max_distance":agent.path_max_distance,
        "radius":agent.radius,"height":agent.height,"max_speed":agent.max_speed,
        "avoidance_enabled":agent.avoidance_enabled,"avoidance_layers":agent.avoidance_layers,
        "avoidance_mask":agent.avoidance_mask,"avoidance_priority":agent.avoidance_priority,
        "neighbor_distance":agent.neighbor_distance,"max_neighbors":agent.max_neighbors,
        "use_3d_avoidance":agent.use_3d_avoidance,
        "time_horizon_agents":agent.time_horizon_agents,"time_horizon_obstacles":agent.time_horizon_obstacles,
    }

static func _agent_data_2d(agent: NavigationAgent2D, target: String) -> Dictionary:
    return {
        "target":target,"dimension":"2d","navigation_layers":agent.navigation_layers,
        "target_position":_v2(agent.target_position),"path_desired_distance":agent.path_desired_distance,
        "target_desired_distance":agent.target_desired_distance,"path_max_distance":agent.path_max_distance,
        "radius":agent.radius,"max_speed":agent.max_speed,
        "avoidance_enabled":agent.avoidance_enabled,"avoidance_layers":agent.avoidance_layers,
        "avoidance_mask":agent.avoidance_mask,"avoidance_priority":agent.avoidance_priority,
        "neighbor_distance":agent.neighbor_distance,"max_neighbors":agent.max_neighbors,
        "time_horizon_agents":agent.time_horizon_agents,"time_horizon_obstacles":agent.time_horizon_obstacles,
    }

static func _array_v2(value):
    if not (value is Array) or value.size() != 2: return null
    return Vector2(float(value[0]),float(value[1]))

static func _array_v3(value):
    if not (value is Array) or value.size() != 3: return null
    return Vector3(float(value[0]),float(value[1]),float(value[2]))

static func _v2(value: Vector2) -> Array:
    return [value.x,value.y]

static func _v3(value: Vector3) -> Array:
    return [value.x,value.y,value.z]
