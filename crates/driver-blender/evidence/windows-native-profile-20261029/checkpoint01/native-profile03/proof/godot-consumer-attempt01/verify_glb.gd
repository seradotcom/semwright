extends SceneTree
# Fixed diagnostic consumer. No caller-supplied code, operators or paths.
const INPUT := "res://broker-proof.glb"
const REPORT := "res://evidence/godot-consumer.json"
const CAPTURE := "res://evidence/godot-consumer.png"
const EXPECTED_POSITION := Vector3(3.0, 5.0, -4.0)
const EXPECTED_SCALE := Vector3(1.0, 1.0, 2.0)
var started := 0
var finished := false
var writable := false
var report: Dictionary = {
	"proof": "broker_glb_native_godot_consumer",
	"godot_broker_used": false, "godot_windows_driver_qualified": false,
	"art_review": "NOT_APPLICABLE_DIAGNOSTIC_FIXTURE"
}

func _initialize() -> void:
	started = Time.get_ticks_msec()
	root.title = "Semwright — native Godot GLB proof"
	root.size = Vector2i(1280, 720)
	call_deferred("_run")

func _run() -> void:
	if not DirAccess.dir_exists_absolute(ProjectSettings.globalize_path("res://evidence")):
		_finish(2, "Existing evidence directory required")
		return
	if FileAccess.file_exists(REPORT) or FileAccess.file_exists(CAPTURE):
		_finish(2, "Evidence exists; preserve previous attempt")
		return
	writable = true
	create_timer(20.0).timeout.connect(func(): _finish(90, "In-process deadline"))
	report["godot_version"] = Engine.get_version_info()
	report["os"] = OS.get_name()
	report["display_server"] = DisplayServer.get_name()
	report["rendering_method"] = RenderingServer.get_current_rendering_method()
	report["video_adapter_name"] = RenderingServer.get_video_adapter_name()
	report["video_adapter_vendor"] = RenderingServer.get_video_adapter_vendor()
	report["user_data_directory"] = OS.get_user_data_dir()
	var source_hash := FileAccess.get_sha256(INPUT)
	report["input_sha256"] = source_hash
	if source_hash.length() != 64:
		_finish(3, "Input GLB cannot be hashed")
		return
	var packed := load(INPUT) as PackedScene
	if packed == null:
		_finish(3, "Imported GLB is not a PackedScene")
		return
	var world := Node3D.new()
	root.add_child(world)
	var imported := packed.instantiate()
	world.add_child(imported)
	await process_frame
	if finished:
		return
	var meshes: Array[MeshInstance3D] = []
	_collect(imported, meshes)
	report["mesh_instance_count"] = meshes.size()
	if meshes.size() != 1:
		_finish(4, "Expected one imported MeshInstance3D")
		return
	var mesh_node := meshes[0]
	var mesh := mesh_node.mesh
	if mesh == null or mesh.get_surface_count() == 0:
		_finish(5, "Imported geometry has no surfaces")
		return
	var vertices := 0
	for surface in range(mesh.get_surface_count()):
		var arrays := mesh.surface_get_arrays(surface)
		if arrays.size() > Mesh.ARRAY_VERTEX and typeof(arrays[Mesh.ARRAY_VERTEX]) == TYPE_PACKED_VECTOR3_ARRAY:
			vertices += arrays[Mesh.ARRAY_VERTEX].size()
	var bounds := mesh.get_aabb()
	var position := mesh_node.global_transform.origin
	var scale := mesh_node.global_transform.basis.get_scale()
	report["mesh_path"] = str(imported.get_path_to(mesh_node))
	report["surface_count"] = mesh.get_surface_count()
	report["vertex_count"] = vertices
	report["local_aabb_position"] = _v3(bounds.position)
	report["local_aabb_size"] = _v3(bounds.size)
	report["global_translation"] = _v3(position)
	report["global_scale"] = _v3(scale)
	if vertices < 3 or bounds.size.length_squared() <= 0.0001:
		_finish(5, "Imported geometry is empty")
		return
	if position.distance_to(EXPECTED_POSITION) > 0.0001 or scale.distance_to(EXPECTED_SCALE) > 0.0001:
		_finish(6, "Imported transform differs from broker fixture")
		return
	if not mesh_node.is_visible_in_tree() or DisplayServer.get_name().to_lower() == "headless":
		_finish(7, "Visible native rendering required")
		return
	var environment := Environment.new()
	environment.background_mode = Environment.BG_COLOR
	environment.background_color = Color(0.07, 0.09, 0.13)
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	environment.ambient_light_color = Color(0.72, 0.80, 0.92)
	environment.ambient_light_energy = 0.45
	var world_environment := WorldEnvironment.new()
	world_environment.environment = environment
	world.add_child(world_environment)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-40.0, -35.0, 0.0)
	light.light_energy = 1.4
	world.add_child(light)
	var camera := Camera3D.new()
	camera.position = Vector3(10.0, 11.0, 9.0)
	camera.fov = 40.0
	camera.far = 100.0
	world.add_child(camera)
	camera.look_at(EXPECTED_POSITION, Vector3.UP)
	camera.make_current()
	report["camera_position"] = _v3(camera.global_position)
	report["camera_target"] = _v3(EXPECTED_POSITION)
	for frame in range(8):
		await process_frame
		if finished:
			return
	await RenderingServer.frame_post_draw
	if finished:
		return
	var image := root.get_texture().get_image()
	if image == null or image.is_empty():
		_finish(7, "Empty native viewport image")
		return
	report["capture_size"] = [image.get_width(), image.get_height()]
	if image.get_width() != 1280 or image.get_height() != 720:
		_finish(7, "Capture size is not 1280x720")
		return
	if FileAccess.get_sha256(INPUT) != source_hash:
		_finish(9, "Input changed during verification")
		return
	if FileAccess.file_exists(CAPTURE) or image.save_png(CAPTURE) != OK:
		_finish(8, "Capture could not be saved without overwrite")
		return
	report["capture_sha256"] = FileAccess.get_sha256(CAPTURE)
	_finish(0, "Native import, geometry, transform and visible frame verified")

func _collect(node: Node, result: Array[MeshInstance3D]) -> void:
	if node is MeshInstance3D:
		result.append(node as MeshInstance3D)
	for child in node.get_children():
		_collect(child, result)

func _v3(value: Vector3) -> Array:
	return [value.x, value.y, value.z]

func _finish(code: int, message: String) -> void:
	if finished:
		return
	finished = true
	report["status"] = "PASS" if code == 0 else "FAIL"
	report["exit_code"] = code
	report["message"] = message
	report["elapsed_ms"] = Time.get_ticks_msec() - started
	if writable and not FileAccess.file_exists(REPORT):
		var output := FileAccess.open(REPORT, FileAccess.WRITE)
		if output == null:
			code = 91
		else:
			output.store_string(JSON.stringify(report, "\t") + "\n")
			output.flush()
			if output.get_error() != OK:
				code = 91
			output.close()
	print(JSON.stringify(report))
	quit(code)
