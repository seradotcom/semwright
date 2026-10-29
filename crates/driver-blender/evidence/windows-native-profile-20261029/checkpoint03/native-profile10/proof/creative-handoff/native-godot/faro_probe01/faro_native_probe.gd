extends SceneTree
## Supporting native probe only; no saved scene/camera edit and no broker claim.
const PREFIX := "res://../.local/runtime/faro_native_r22_01"
const NEW := {"SW_dc08abe78a2a_integrated_limestone":640,"SW_dc08abe78a2a_coastal_toe":192,"SW_dc08abe78a2a_low_shelf":192}
const OLD := ["GEO01_HARBOR_COAST_faro_integrated_limestone_stone_0","GEO01_HARBOR_COAST_faro_crown_stone_0","GEO01_HARBOR_COAST_faro_footing_cap_stone_0","GEO01_HARBOR_COAST_faro_integrated_limestone_base_stone_dark_0","GEO01_HARBOR_COAST_faro_integrated_limestone_toe_stone_0","GEO01_HARBOR_COAST_faro_rock_mass_stone_0","GEO01_HARBOR_COAST_faro_rock_mass_stone_light_0"]
var done := false
var writable := false
var errors: Array[String] = []
var report := {"native_physics":true,"godot_broker_used":false,"art_approved":false,"bridge_traversability":"NOT_ASSERTED_BY_RAYS"}
func _initialize() -> void:
	root.size = Vector2i(1920,1080)
	root.content_scale_size = Vector2i(1920,1080)
	call_deferred("_run")
func _run() -> void:
	if FileAccess.file_exists(PREFIX+".json") or FileAccess.file_exists(PREFIX+".png"):
		_finish(2,"Preserve earlier probe")
		return
	writable = true
	create_timer(30.0,true).timeout.connect(func(): _finish(90,"Probe deadline"))
	if change_scene_to_file("res://main.tscn") != OK:
		_finish(2,"Scene load failed")
		return
	await process_frame
	await process_frame
	if done: return
	var review := current_scene
	if review == null or not review.get("asset_ready"):
		_finish(2,"Admitted scene unavailable")
		return
	report["source"] = review.call("_source_receipt")
	var all: Array[Node] = []
	_collect(review,all)
	var meshes: Array[MeshInstance3D] = []
	var found: Dictionary = {}
	for node in all:
		if str(node.name) in OLD: errors.append("Retired node remains: "+str(node.get_path()))
		if NEW.has(str(node.name)):
			found[str(node.name)] = int(found.get(str(node.name),0))+1
			if node is MeshInstance3D: meshes.append(node)
	for label in NEW:
		if int(found.get(label,0)) != 1: errors.append("New mesh identity absent or ambiguous: "+label)
	if meshes.size() != 3 or not errors.is_empty():
		_finish(3,"Replacement identity failed")
		return
	var bodies: Array[CollisionObject3D] = []
	var rows: Array = []
	var triangle_total := 0
	for node in meshes:
		var triangles := 0
		for i in node.mesh.get_surface_count():
			if node.mesh.surface_get_primitive_type(i) != Mesh.PRIMITIVE_TRIANGLES: errors.append("Nontriangle primitive")
			var indices: int = node.mesh.surface_get_array_index_len(i)
			triangles += int((indices if indices > 0 else node.mesh.surface_get_array_len(i))/3)
		triangle_total += triangles
		if triangles != int(NEW[str(node.name)]): errors.append("Unexpected triangles")
		var bounds: AABB = node.global_transform*node.mesh.get_aabb()
		var end := bounds.position+bounds.size
		if bounds.position.x < -66.001 or end.x > -48.749 or bounds.position.y < -1.251 or end.y > 10.211:
			errors.append("World bounds outside rock constraint")
		var children: Array[StaticBody3D] = []
		for child in node.get_children():
			if child is StaticBody3D: children.append(child)
		if children.size() != 1:
			_finish(4,"Expected one new collider per mesh")
			return
		var body := children[0]
		if body.get_child_count() != 1 or not body.get_child(0) is CollisionShape3D:
			_finish(4,"Unexpected collision shape")
			return
		var shape_node := body.get_child(0) as CollisionShape3D
		if shape_node.disabled or not shape_node.shape is ConcavePolygonShape3D:
			_finish(4,"Expected enabled exact concave collider")
			return
		var collision_triangles := int((shape_node.shape as ConcavePolygonShape3D).get_faces().size()/3)
		if collision_triangles != triangles: errors.append("Collider triangle mismatch")
		bodies.append(body)
		rows.append({"name":node.name,"triangles":triangles,"bounds_min":_v(bounds.position),"bounds_max":_v(end),"collider_path":str(body.get_path()),"collision_triangles":collision_triangles})
	report["meshes"] = rows
	report["triangle_total"] = triangle_total
	report["old_meshes_absent"] = errors.is_empty()
	await physics_frame
	await physics_frame
	if done: return
	var exclude: Array[RID] = []
	for node in all:
		if node is CollisionObject3D and not bodies.has(node): exclude.append(node.get_rid())
	var space := meshes[0].get_world_3d().direct_space_state
	var bearing: Array = []
	for i in range(13):
		var angle := TAU*float(i)/12.0
		var radius := 0.0 if i == 12 else 3.25
		var x := -56.0+radius*cos(angle)
		var z := 7.0+radius*sin(angle)
		var hit := _ray(space,Vector3(x,12,z),Vector3(x,9,z),exclude)
		var row := _hit(hit)
		row["sample_xz"] = [x,z]
		if hit.is_empty() or not bodies.has(hit.get("collider")) or absf(float(hit.get("position",Vector3.ZERO).y)-10.17)>0.02:
			errors.append("Bearing support failed: "+str(i))
		bearing.append(row)
	report["bearing_rays"] = bearing
	var bridge: Array = []
	for z: float in [6.3,7.0,7.7]:
		var a := _ray(space,Vector3(-50.5,14,z),Vector3(-50.5,6,z),[])
		var b := _ray(space,Vector3(-51.2,14,z),Vector3(-51.2,6,z),[])
		bridge.append({"z":z,"deck":_hit(a),"rock":_hit(b),"delta":float(b.position.y-a.position.y) if not a.is_empty() and not b.is_empty() else null})
	report["bridge_samples"] = bridge
	var arch := _ray(space,Vector3(-37,3,0),Vector3(-37,3,14),[])
	report["arch_ray"] = {"from":[-37,3,0],"to":[-37,3,14],"clear":arch.is_empty(),"hit":_hit(arch)}
	if not arch.is_empty(): errors.append("Arch opening blocked")
	if DisplayServer.get_name() == "headless":
		_finish(5,"Visible renderer required")
		return
	var camera: Camera3D = review.get("camera")
	var prior := camera.transform
	var prior_fov := camera.fov
	camera.position = Vector3(-75,18,40)
	camera.look_at(Vector3(-49,12,7),Vector3.UP)
	camera.fov = 48
	review.set("motion",Vector3.ZERO)
	report["supporting_camera"] = {"eye":[-75,18,40],"target":[-49,12,7],"fov":48,"human_camera_edit":false}
	await create_timer(2.0).timeout
	await RenderingServer.frame_post_draw
	if done: return
	var img := root.get_texture().get_image()
	if img == null or img.is_empty() or img.get_size() != Vector2i(1920,1080):
		_finish(5,"Visible pixels unavailable")
		return
	if FileAccess.file_exists(PREFIX+".png") or img.save_png(PREFIX+".png") != OK:
		_finish(5,"Image destination unavailable")
		return
	report["image"] = {"path":PREFIX+".png","sha256":FileAccess.get_sha256(PREFIX+".png"),"width":1920,"height":1080}
	camera.transform = prior
	camera.fov = prior_fov
	report["runtime_camera_restored"] = camera.transform == prior and camera.fov == prior_fov
	_finish(0 if errors.is_empty() else 6,"Native supporting probe complete")
func _collect(node: Node,out: Array[Node]) -> void:
	out.append(node)
	for child in node.get_children(): _collect(child,out)
func _v(value: Vector3) -> Array: return [value.x,value.y,value.z]
func _ray(space: PhysicsDirectSpaceState3D,start: Vector3,end: Vector3,exclude: Array[RID]) -> Dictionary:
	var q := PhysicsRayQueryParameters3D.create(start,end,1)
	q.exclude = exclude
	q.collide_with_areas = false
	q.hit_back_faces = true
	return space.intersect_ray(q)
func _hit(hit: Dictionary) -> Dictionary:
	if hit.is_empty(): return {"hit":false}
	return {"hit":true,"position":_v(hit.position),"normal":_v(hit.normal),"collider_path":str(hit.collider.get_path())}
func _finish(code: int,message: String) -> void:
	if done: return
	done = true
	report["status"] = "PASS" if code == 0 else "FAIL"
	report["message"] = message
	report["errors"] = errors
	if writable and not FileAccess.file_exists(PREFIX+".json"):
		var f := FileAccess.open(PREFIX+".json",FileAccess.WRITE)
		if f != null:
			f.store_string(JSON.stringify(report,"\t")+"\n")
			f.flush()
			if f.get_error() != OK: code = 91
			f.close()
		else: code = 91
	elif code == 0: code = 91
	print("FARO_NATIVE_PROBE "+JSON.stringify({"code":code,"errors":errors}))
	quit(code)
