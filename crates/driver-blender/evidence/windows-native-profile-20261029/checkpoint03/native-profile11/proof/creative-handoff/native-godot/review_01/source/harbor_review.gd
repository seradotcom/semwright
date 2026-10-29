extends Node3D
## Native G2 review surface. Only digest-verified Blender exports are admitted.
const REVIEW_WATER_LAYER := 1 << 19
const BUNDLE := "res://assets/native_bundle.json"
const DEFAULT_SHOTS := [
	{"id":"llegar","title":"Llegar","eye":[15.0,4.3,58.0],"target":[-9.0,9.0,3.0],"fov":60.0},
	{"id":"habitar","title":"Habitar","eye":[32.0,9.0,20.0],"target":[9.0,9.0,-1.0],"fov":55.0},
	{"id":"comprender","title":"Comprender","eye":[68.0,62.0,88.0],"target":[-5.0,6.0,-9.0],"fov":42.0},
	{"id":"detail","title":"Casa del correo","eye":[29.0,9.8,13.5],"target":[18.5,9.1,-0.5],"fov":47.0}
]
var camera := Camera3D.new()
var hud := CanvasLayer.new()
var status_label := Label.new()
var shots: Array = DEFAULT_SHOTS.duplicate(true)
var shot_index := 0
var yaw := 0.0
var pitch := 0.0
var motion := Vector3.ZERO
var frame_samples: Array[float] = []
var source_manifest: Dictionary = {}
var loaded_assets: Array[Dictionary] = []
var asset_ready := false
var elapsed := 0.0
var runtime_dir := ""
var capture_busy := false
var review_environment: Environment
var review_sun: DirectionalLight3D
var wall_frame_samples: Array[float] = []
var previous_frame_usec := 0
var styled_materials: Dictionary = {}
var foliage_audit: Dictionary = {"surfaces":0,"uv_vertices":0,"errors":[]}
var walker: CharacterBody3D
var render_shader_hashes: Dictionary = {}
var performance_sampling := false
var drawn_frame_samples: Array[float] = []
var previous_draw_usec := 0
var drawn_count := 0
var render_window: Dictionary = {}
var capture_deadline_usec := 0
var ocean_material: ShaderMaterial
var planar_viewport: SubViewport
var planar_camera: Camera3D
var planar_dirty := true
var planar_clock := 0.0
var planar_last_transform := Transform3D.IDENTITY
var planar_last_fov := -1.0
var planar_last_aspect := -1.0
var planar_last_keep_aspect := -1
var planar_requests := 0
const VIEWPORT_SAMPLE_LIMIT := 2048
var viewport_sampling := false
var viewport_sample_kind := ""
var viewport_sample_start_us := 0
var viewport_sample_request_start := 0
var viewport_sample_previous_requests := 0
var viewport_sample_dropped := 0
var viewport_samples: Array[Dictionary] = []
var viewport_main_rid := RID()
var viewport_planar_rid := RID()

func _ready() -> void:
	Engine.max_fps = 60
	if "--disable-review-vsync" in OS.get_cmdline_user_args():
		DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	RenderingServer.frame_post_draw.connect(_on_frame_drawn)
	if "--capture-review" in OS.get_cmdline_user_args() or "--capture-motion-review" in OS.get_cmdline_user_args():
		DisplayServer.window_set_flag(DisplayServer.WINDOW_FLAG_ALWAYS_ON_TOP,true)
		DisplayServer.window_move_to_foreground()
	runtime_dir = ProjectSettings.globalize_path("res://../.local/runtime")
	if DirAccess.make_dir_recursive_absolute(runtime_dir) != OK:
		push_error("Review output directory unavailable")
		get_tree().quit(3)
		return
	for shader_name in ["sky","surface","ocean"]:
		render_shader_hashes[shader_name] = FileAccess.get_sha256("res://materials/archipelago_"+shader_name+".gdshader")
	_build_light()
	add_child(camera)
	camera.near = 0.12
	camera.far = 6000.0
	camera.current = true
	_build_hud()
	_load_camera_plan()
	asset_ready = _load_native_bundle()
	if asset_ready:
		_build_ocean()
		walker = load("res://scripts/review_walker.gd").new()
		walker.call("configure",self,camera)
		add_child(walker)
	_set_shot(0)
	if not asset_ready:
		status_label.text = "Native asset handoff pending. Production geometry has not been admitted."
	if "--verify-assets" in OS.get_cmdline_user_args():
		if asset_ready:
			var saved := _write_json("native_import_readback.json", _source_receipt())
			get_tree().quit(0 if saved else 4)
		else:
			get_tree().quit(2)
	elif "--verify-walk" in OS.get_cmdline_user_args() or "--verify-bridge" in OS.get_cmdline_user_args():
		if asset_ready:
			walker.call("run_validation")
		else:
			get_tree().quit(2)
	elif "--light-audit" in OS.get_cmdline_user_args():
		if asset_ready:
			_light_audit_sequence()
		else:
			get_tree().quit(2)
	elif "--capture-motion-review" in OS.get_cmdline_user_args():
		if asset_ready:
			_capture_motion_review()
		else:
			get_tree().quit(2)
	elif "--capture-review" in OS.get_cmdline_user_args():
		if not asset_ready:
			get_tree().quit(2)
		else:
			_capture_review_sequence()

func _capture_review_sequence() -> void:
	for index in range(4):
		_set_shot(index)
		await get_tree().create_timer(3.0).timeout
		frame_samples.clear()
		wall_frame_samples.clear()
		drawn_frame_samples.clear()
		drawn_count = 0
		var started := Time.get_ticks_usec()
		var engine_drawn_before := Engine.get_frames_drawn()
		_begin_viewport_samples("still_8s_renderer_window")
		performance_sampling = true
		await get_tree().create_timer(8.0).timeout
		performance_sampling = false
		var viewport_timing := _end_viewport_samples()
		var seconds := float(Time.get_ticks_usec()-started)/1000000.0
		render_window = {"seconds":seconds,"frame_post_draw_events":drawn_count,"engine_frames_drawn":Engine.get_frames_drawn()-engine_drawn_before,"draw_events_per_second":float(drawn_count)/seconds,"note":"Renderer frame completion callbacks, not display presentation or GPU timestamps"}
		render_window["viewport_render_timing"] = viewport_timing
		if drawn_frame_samples.is_empty() or Time.get_ticks_usec()-previous_draw_usec > 1000000:
			push_error("Visible renderer stopped drawing during the measurement window")
			_write_json("render_sample_failure.json",{"source":_source_receipt(),"render_window":render_window})
			get_tree().quit(6)
			return
		if not await _capture_still():
			get_tree().quit(4)
			return
	var recorded := _write_json("review_sequence_complete.json", {"native_godot":true, "shots":4, "art_approved":false})
	get_tree().quit(0 if recorded else 4)

func _build_ocean() -> void:
	var mesh := PlaneMesh.new()
	mesh.size = Vector2(8000.0, 8000.0)
	mesh.subdivide_width = 0
	mesh.subdivide_depth = 0
	var ocean := MeshInstance3D.new()
	ocean.name = "MAT01_OriginalOcean"
	ocean.layers = REVIEW_WATER_LAYER
	ocean.mesh = mesh
	ocean.position = Vector3(0.0, -0.03, 0.0)
	ocean.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	ocean.extra_cull_margin = 0.5
	var material := ShaderMaterial.new()
	material.shader = load("res://materials/archipelago_ocean.gdshader")
	ocean.material_override = material
	ocean_material = material
	add_child(ocean)
	_build_planar_reflection()

func _build_light() -> void:
	var sky_material := ShaderMaterial.new()
	sky_material.shader = load("res://materials/archipelago_sky.gdshader")
	var sky := Sky.new()
	sky.radiance_size = Sky.RADIANCE_SIZE_512
	sky.process_mode = Sky.PROCESS_MODE_QUALITY
	sky.sky_material = sky_material
	var env := Environment.new()
	review_environment = env
	env.background_mode = Environment.BG_SKY
	env.sky = sky
	env.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	env.ambient_light_color = Color("b4c9b8")
	env.ambient_light_energy = 0.34
	env.tonemap_mode = Environment.TONE_MAPPER_FILMIC
	env.tonemap_white = 4.0
	env.fog_enabled = true
	env.fog_light_color = Color("c0d4cc")
	env.fog_density = 0.0005
	env.fog_sky_affect = 0.05
	env.ssao_enabled = true
	env.ssao_radius = 1.6
	env.ssao_intensity = 1.25
	env.ssao_light_affect = 0.12
	var world_environment := WorldEnvironment.new()
	world_environment.environment = env
	add_child(world_environment)
	var sun := DirectionalLight3D.new()
	review_sun = sun
	sun.name = "CostaSun"
	sun.rotation_degrees = Vector3(-43.0, -38.0, 0.0)
	sun.light_color = Color("fff0d3")
	sun.light_energy = 1.15
	sun.shadow_enabled = true
	sun.light_angular_distance = 0.65
	sun.directional_shadow_max_distance = 220.0
	add_child(sun)
	sky_material.set_shader_parameter("sun_direction",sun.global_basis.z.normalized())

func _load_native_bundle() -> bool:
	if not FileAccess.file_exists(BUNDLE):
		return false
	var parsed: Variant = JSON.parse_string(FileAccess.get_file_as_string(BUNDLE))
	if not parsed is Dictionary or parsed.get("schema_version", 0) != 1:
		push_error("Invalid native asset bundle")
		return false
	source_manifest = parsed
	var staged: Array[Node] = []
	for asset: Dictionary in source_manifest.get("assets", []):
		var path: String = asset.get("path", "")
		if not path.begins_with("res://assets/") or ".." in path or not path.ends_with(".glb"):
			push_error("Asset is outside the admitted project subtree")
			_release_staged(staged)
			return false
		if FileAccess.get_sha256(path).to_lower() != str(asset.get("sha256", "")).to_lower():
			push_error("Native asset digest mismatch: " + path)
			_release_staged(staged)
			return false
		var document := GLTFDocument.new()
		var state := GLTFState.new()
		if document.append_from_file(path, state) != OK:
			push_error("GLB import failed: " + path)
			_release_staged(staged)
			return false
		var instance := document.generate_scene(state)
		if instance == null:
			_release_staged(staged)
			return false
		staged.append(instance)
		var replacement := _remove_replaced_meshes(instance, asset.get("replace_meshes", []))
		var roles: Variant = asset.get("material_roles", {})
		if not replacement.ok or not roles is Dictionary or roles.size() > 16:
			push_error("Invalid bounded native asset replacement")
			_release_staged(staged)
			return false
		for source_material: Variant in roles:
			if not source_material is String or not roles[source_material] in ["stone","stone_dark","stone_light"]:
				_release_staged(staged)
				return false
		var counts := {"meshes":0,"surfaces":0,"vertices":0,"triangles":0}
		_apply_surface_style(instance, roles)
		if not foliage_audit.errors.is_empty():
			_release_staged(staged)
			return false
		if str(asset.get("id","")).begins_with("GEO01") or str(asset.get("id","")).begins_with("ARQ"):
			_build_review_colliders(instance)
		elif str(asset.get("id","")).begins_with("NAT01"):
			_build_review_colliders(instance,true)
		_inspect_geometry(instance, counts)
		loaded_assets.append({"id":asset.get("id"),"path":path,"sha256":asset.get("sha256"),"geometry":counts,"replaced_meshes":replacement.removed,"material_roles":roles})
	for instance in staged:
		add_child(instance)
	return not loaded_assets.is_empty()

func _remove_replaced_meshes(instance: Node, requested: Variant) -> Dictionary:
	var result := {"ok":false,"removed":[]}
	if not requested is Array or requested.size() > 64:
		return result
	var names: Dictionary = {}
	for item: Variant in requested:
		if not item is String or item.is_empty() or item.length() > 256 or names.has(item):
			return result
		names[item] = true
	var matches: Dictionary = {}
	if not _find_replaced_meshes(instance, names, matches) or matches.size() != names.size():
		return result
	for item in matches.values():
		if not item is MeshInstance3D or item.get_child_count() != 0:
			return result
	for label: String in names:
		var mesh_node: MeshInstance3D = matches[label]
		mesh_node.get_parent().remove_child(mesh_node)
		mesh_node.free()
		result.removed.append(label)
	result.ok = true
	return result

func _find_replaced_meshes(node: Node, names: Dictionary, matches: Dictionary) -> bool:
	var label := str(node.name)
	if names.has(label):
		if matches.has(label):
			return false
		matches[label] = node
	for child in node.get_children():
		if not _find_replaced_meshes(child, names, matches):
			return false
	return true

func _release_staged(nodes: Array[Node]) -> void:
	for node in nodes:
		node.free()
	loaded_assets.clear()

func _inspect_geometry(node: Node, counts: Dictionary) -> void:
	if node is MeshInstance3D and node.mesh != null:
		counts.meshes += 1
		for index in node.mesh.get_surface_count():
			counts.surfaces += 1
			counts.vertices += node.mesh.surface_get_array_len(index)
			var indices: int = node.mesh.surface_get_array_index_len(index)
			counts.triangles += indices / 3 if indices > 0 else node.mesh.surface_get_array_len(index) / 3
	for child in node.get_children():
		_inspect_geometry(child, counts)

func _build_hud() -> void:
	add_child(hud)
	var bar := PanelContainer.new()
	bar.position = Vector2(28,24)
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.953,0.941,0.91,0.95)
	style.content_margin_left = 22
	style.content_margin_right = 22
	style.content_margin_top = 15
	style.content_margin_bottom = 15
	bar.add_theme_stylebox_override("panel",style)
	hud.add_child(bar)
	var box := VBoxContainer.new()
	bar.add_child(box)
	var title := Label.new()
	title.text = "ARCHIPELAGO / COSTA DE LOS VIENTOS"
	title.add_theme_color_override("font_color",Color("203d38"))
	title.add_theme_font_size_override("font_size",22)
	box.add_child(title)
	status_label.add_theme_color_override("font_color",Color("405e44"))
	status_label.add_theme_font_size_override("font_size",16)
	box.add_child(status_label)
	var controls := Label.new()
	controls.text = "1–4 Vistas · Ratón derecho + WASD · Q/E Altura · F Pasear · Espacio Saltar · F5 Guardar cámara · F6 Captura · Tab Interfaz"
	controls.add_theme_color_override("font_color",Color("203d38"))
	controls.add_theme_font_size_override("font_size",14)
	box.add_child(controls)

func _set_shot(index: int) -> void:
	if walker != null:
		walker.call("leave")
	shot_index = clampi(index,0,shots.size()-1)
	var shot: Dictionary = shots[shot_index]
	camera.position = _vector(shot.eye)
	camera.look_at(_vector(shot.target),Vector3.UP)
	camera.fov = float(shot.fov)
	yaw = camera.rotation.y
	pitch = camera.rotation.x
	motion = Vector3.ZERO

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton and event.button_index == MOUSE_BUTTON_RIGHT:
		Input.mouse_mode = Input.MOUSE_MODE_CAPTURED if event.pressed else Input.MOUSE_MODE_VISIBLE
	if event is InputEventMouseMotion and Input.mouse_mode == Input.MOUSE_MODE_CAPTURED:
		yaw -= event.relative.x * 0.0022
		pitch = clampf(pitch-event.relative.y*0.0022,-1.48,1.48)
		camera.rotation = Vector3(pitch,yaw,0.0)
	if event is InputEventKey and event.pressed and not event.echo:
		if event.keycode >= KEY_1 and event.keycode <= KEY_4:
			_set_shot(event.keycode-KEY_1)
		elif event.keycode == KEY_ESCAPE:
			Input.mouse_mode = Input.MOUSE_MODE_VISIBLE
		elif event.keycode == KEY_F and walker != null:
			walker.call("toggle")
		elif event.keycode == KEY_TAB:
			hud.visible = not hud.visible
		elif event.keycode == KEY_F5:
			_save_camera_plan()
		elif event.keycode == KEY_F6 and asset_ready:
			_capture_still()

func _process(delta: float) -> void:
	var now_usec := Time.get_ticks_usec()
	if capture_deadline_usec > 0 and now_usec > capture_deadline_usec:
		push_error("Native capture exceeded its bounded draw deadline")
		get_tree().quit(6)
		return
	if previous_frame_usec > 0 and performance_sampling and wall_frame_samples.size() < 18000:
		wall_frame_samples.append(float(now_usec - previous_frame_usec) / 1000.0)
	previous_frame_usec = now_usec
	elapsed += delta
	if performance_sampling and frame_samples.size() < 18000:
		frame_samples.append(delta*1000.0)
	if Input.mouse_mode == Input.MOUSE_MODE_CAPTURED and (walker == null or not walker.get("active")):
		var direction := Vector3(float(Input.is_key_pressed(KEY_D))-float(Input.is_key_pressed(KEY_A)),float(Input.is_key_pressed(KEY_E))-float(Input.is_key_pressed(KEY_Q)),float(Input.is_key_pressed(KEY_S))-float(Input.is_key_pressed(KEY_W)))
		var speed := 13.0*(3.0 if Input.is_key_pressed(KEY_SHIFT) else 1.0)
		motion = motion.lerp(camera.basis*direction.normalized()*speed,1.0-exp(-10.0*delta))
		camera.position += motion*delta
		camera.position.y = clampf(camera.position.y,1.7,350.0)
	if asset_ready:
		status_label.text = "%s · Godot nativo · %d FPS · Revisión artística pendiente" % [shots[shot_index].title,Engine.get_frames_per_second()]

	_update_planar_reflection(delta)

func _load_camera_plan() -> void:
	var path := runtime_dir.path_join("camera_plan.json")
	if not FileAccess.file_exists(path):
		return
	var parsed: Variant = JSON.parse_string(FileAccess.get_file_as_string(path))
	if parsed is Dictionary and parsed.get("schema_version") == 1 and parsed.get("shots") is Array and parsed.shots.size() == DEFAULT_SHOTS.size():
		for candidate: Variant in parsed.shots:
			if not candidate is Dictionary or not _valid_shot(candidate):
				push_warning("Saved camera plan is invalid; source defaults retained")
				return
		shots = parsed.shots

func _valid_shot(shot: Dictionary) -> bool:
	for key in ["eye","target"]:
		var value: Variant = shot.get(key)
		if not value is Array or value.size() != 3:
			return false
		for number: Variant in value:
			if not (number is float or number is int) or not is_finite(float(number)):
				return false
	return shot.get("id") is String and shot.get("title") is String and float(shot.get("fov",0)) >= 20.0 and float(shot.get("fov",0)) <= 100.0

func _save_camera_plan() -> void:
	shots[shot_index].eye = _array(camera.position)
	shots[shot_index].target = _array(camera.position-camera.basis.z*30.0)
	shots[shot_index].fov = camera.fov
	shots[shot_index].human_locked = true
	_write_json("camera_plan.json",{"schema_version":1,"shots":shots})

func _source_receipt() -> Dictionary:
	return {"foliage_material":foliage_audit,"planar_reflection":{"enabled":planar_viewport != null,"capture_width":768,"capture_height":planar_viewport.size.y if planar_viewport != null else 0,"max_update_hz":30,"render_requests":planar_requests,"cached_by_pose":true,"linear_hdr":true,"note":"Render requests, not measured GPU passes; moving-camera cost differs from cached stills"},"runtime_diagnostics":{"window_focused":DisplayServer.window_is_focused(),"window_mode":DisplayServer.window_get_mode(),"vsync_mode":DisplayServer.window_get_vsync_mode(),"time_scale":Engine.time_scale,"process_ms":Performance.get_monitor(Performance.TIME_PROCESS)*1000.0,"physics_ms":Performance.get_monitor(Performance.TIME_PHYSICS_PROCESS)*1000.0},"render_settings":{"tonemap":review_environment.tonemap_mode,"white":review_environment.tonemap_white,"exposure":review_environment.tonemap_exposure,"sun_energy":review_sun.light_energy,"ambient_energy":review_environment.ambient_light_energy,"fog_density":review_environment.fog_density,"fog_sky_affect":review_environment.fog_sky_affect,"ambient_source":review_environment.ambient_light_source,"shaders_sha256":render_shader_hashes},"engine":Engine.get_version_info(),"renderer":RenderingServer.get_current_rendering_method(),"gpu":RenderingServer.get_video_adapter_name(),"viewport":[get_viewport().get_visible_rect().size.x,get_viewport().get_visible_rect().size.y],"assets":loaded_assets,"source_manifest":source_manifest,"camera":shots[shot_index],"art_approved":false,"capture_kind":"native_godot"}

func _capture_still() -> bool:
	if capture_busy:
		return false
	capture_busy = true
	capture_deadline_usec = Time.get_ticks_usec()+12000000
	var previous := hud.visible
	hud.visible = false
	await RenderingServer.frame_post_draw
	var stem := str(shots[shot_index].id)+"_"+str(Time.get_unix_time_from_system()).replace(".","_")
	var snapshot := get_viewport().get_texture().get_image()
	var image_path := runtime_dir.path_join(stem+"_godot.png")
	if snapshot.is_empty() or snapshot.save_png(image_path) != OK:
		push_error("Could not save native still")
		hud.visible = previous
		capture_busy = false
		return false
	var receipt := _source_receipt()
	receipt.rendered_frame_window = render_window
	receipt.drawn_frame_times_ms = drawn_frame_samples
	receipt.frame_times_ms = frame_samples
	receipt.wall_frame_times_ms = wall_frame_samples
	receipt.timing_note = "frame_times_ms are engine delta (may be smoothed); wall_frame_times_ms are monotonic process callback intervals, not GPU timestamps"
	receipt.camera_transform = {"eye":_array(camera.position),"rotation":_array(camera.rotation),"fov":camera.fov}
	receipt.image = {"filename":stem+"_godot.png","sha256":FileAccess.get_sha256(image_path),"width":snapshot.get_width(),"height":snapshot.get_height()}
	var recorded := _write_json(stem+".json",receipt)
	hud.visible = previous
	capture_busy = false
	capture_deadline_usec = 0
	return recorded

func _write_json(filename: String, data: Dictionary) -> bool:
	var path := runtime_dir.path_join(filename)
	var file := FileAccess.open(path+".tmp",FileAccess.WRITE)
	if file == null:
		push_error("Cannot persist native review evidence")
		return false
	file.store_string(JSON.stringify(data,"\t"))
	file.flush()
	var result := file.get_error()
	file.close()
	if result != OK:
		push_error("Cannot finish writing native review evidence")
		return false
	if FileAccess.file_exists(path):
		var backup := path+".previous."+str(Time.get_ticks_usec())
		if DirAccess.rename_absolute(path,backup) != OK:
			push_error("Cannot preserve previous review state")
			return false
	if DirAccess.rename_absolute(path+".tmp",path) != OK:
		push_error("Cannot publish saved review state")
		return false
	return true

func _vector(a: Array) -> Vector3:
	return Vector3(float(a[0]),float(a[1]),float(a[2]))

func _array(v: Vector3) -> Array:
	return [v.x,v.y,v.z]

func _light_audit_sequence() -> void:
	var profiles := [
		{"id":"audit_original","mode":Environment.TONE_MAPPER_FILMIC,"white":1.0,"sun":1.35,"ambient":0.58},
		{"id":"audit_white6","mode":Environment.TONE_MAPPER_FILMIC,"white":6.0,"sun":1.35,"ambient":0.58},
		{"id":"audit_linear","mode":Environment.TONE_MAPPER_LINEAR,"white":1.0,"sun":1.35,"ambient":0.58},
		{"id":"audit_lower","mode":Environment.TONE_MAPPER_FILMIC,"white":6.0,"sun":0.85,"ambient":0.32}
	]
	_set_shot(1)
	for profile: Dictionary in profiles:
		review_environment.tonemap_mode = profile.mode
		review_environment.tonemap_white = profile.white
		review_sun.light_energy = profile.sun
		review_environment.ambient_light_energy = profile.ambient
		shots[1].id = profile.id
		await get_tree().create_timer(2.0).timeout
		frame_samples.clear()
		wall_frame_samples.clear()
		await get_tree().create_timer(2.0).timeout
		if not await _capture_still():
			get_tree().quit(4)
			return
	get_tree().quit(0)


func _foliage_failure(message: String) -> StandardMaterial3D:
	foliage_audit.errors.append(message)
	push_error(message)
	return null

func _foliage_cutout(original: StandardMaterial3D, node: MeshInstance3D, index: int) -> StandardMaterial3D:
	var arrays := node.mesh.surface_get_arrays(index)
	var uv: Variant = arrays[Mesh.ARRAY_TEX_UV]
	if not uv is PackedVector2Array or uv.size() != node.mesh.surface_get_array_len(index):
		return _foliage_failure("Native foliage UV readback failed: "+str(node.name))
	if original.albedo_texture == null:
		return _foliage_failure("Native foliage texture binding missing")
	foliage_audit.surfaces = int(foliage_audit.surfaces)+1
	foliage_audit.uv_vertices = int(foliage_audit.uv_vertices)+uv.size()
	var image := original.albedo_texture.get_image()
	var pixels_available := image != null and not image.is_empty()
	if not pixels_available and DisplayServer.get_name() != "headless":
		return _foliage_failure("Native visible foliage pixel readback unavailable")
	var texture: Texture2D = original.albedo_texture
	if pixels_available:
		if image.is_compressed() and image.decompress() != OK:
			return _foliage_failure("Native foliage texture decompression failed")
		image.convert(Image.FORMAT_RGBA8)
		var pixel_bytes := image.get_data()
		var base_bytes := image.get_width()*image.get_height()*4
		var hasher := HashingContext.new()
		if hasher.start(HashingContext.HASH_SHA256) != OK or hasher.update(pixel_bytes.slice(0,base_bytes)) != OK:
			return _foliage_failure("Native foliage pixel hash failed")
		var digest := hasher.finish().hex_encode()
		if foliage_audit.has("image_sha256") and foliage_audit.image_sha256 != digest:
			return _foliage_failure("Foliage atlas pixels differ between GLB parts")
		foliage_audit.image_sha256 = digest
		if not styled_materials.has("pine_needles:cutout_r18"):
			var alpha_min := 255
			var alpha_max := 0
			var samples := 0
			var opaque := 0
			for offset in range(3,base_bytes,68):
				var alpha := int(pixel_bytes[offset])
				alpha_min = mini(alpha_min,alpha)
				alpha_max = maxi(alpha_max,alpha)
				opaque += 1 if alpha >= 128 else 0
				samples += 1
			if alpha_min >= 128 or alpha_max < 128:
				return _foliage_failure("Native foliage alpha has no usable cutout")
			var had_mips := image.has_mipmaps()
			if not had_mips:
				if image.generate_mipmaps() != OK:
					return _foliage_failure("Native foliage mipmap generation failed")
				texture = ImageTexture.create_from_image(image)
			foliage_audit.merge({"pixel_readback":"PASS","width":image.get_width(),"height":image.get_height(),"alpha_probe_stride_pixels":17,"alpha_min_sampled":alpha_min,"alpha_max_sampled":alpha_max,"alpha_coverage_ge_128_sampled":float(opaque)/samples,"mipmaps_imported":had_mips,"mipmaps_used":image.has_mipmaps(),"consumer_alpha_mode":"Godot Alpha Scissor","alpha_scissor_threshold":0.4,"source_glb_transparency_enum":original.transparency},true)
	else:
		foliage_audit.pixel_readback = "UNAVAILABLE_HEADLESS_REQUIRES_VISIBLE_REVIEW"
	if not styled_materials.has("pine_needles:cutout_r18"):
		var leaf := original.duplicate() as StandardMaterial3D
		leaf.albedo_texture = texture
		leaf.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA_SCISSOR
		leaf.alpha_scissor_threshold = 0.4
		leaf.cull_mode = BaseMaterial3D.CULL_DISABLED
		leaf.alpha_antialiasing_mode = BaseMaterial3D.ALPHA_ANTIALIASING_ALPHA_TO_COVERAGE
		leaf.alpha_antialiasing_edge = 0.2
		leaf.texture_filter = BaseMaterial3D.TEXTURE_FILTER_LINEAR_WITH_MIPMAPS_ANISOTROPIC
		leaf.roughness = 0.92
		leaf.metallic = 0.0
		leaf.metallic_specular = 0.12
		styled_materials["pine_needles:cutout_r18"] = leaf
	node.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_DOUBLE_SIDED
	return styled_materials["pine_needles:cutout_r18"] as StandardMaterial3D

func _apply_surface_style(node: Node, roles: Dictionary = {}) -> void:
	if node is MeshInstance3D and node.mesh != null:
		for index in node.mesh.get_surface_count():
			var original: Material = node.mesh.surface_get_material(index)
			if not original is StandardMaterial3D:
				continue
			var label: String = roles.get(original.resource_name, original.resource_name)
			if label == "pine_needles":
				var foliage := _foliage_cutout(original,node as MeshInstance3D,index)
				if foliage != null:
					node.set_surface_override_material(index,foliage)
				continue
			var kind := -1
			var bump := 0.012
			if label.begins_with("stone"):
				kind = 5 if "integrated_limestone" in str(node.name) else 1
				bump = 0.065 if kind == 5 else 0.04
			elif label in ["cal","tile","tile_dark","tile_light","coral"]:
				kind = 0
				bump = 0.008
			elif label == "wood":
				kind = 2
			elif label == "bark":
				kind = 3
				bump = 0.026
			elif label in ["sand","sage"]:
				kind = 6 if "_harbor_ground_" in str(node.name) else 4
				bump = 0.02
			if kind < 0:
				continue
			var style_key := label+":"+str(kind)
			if not styled_materials.has(style_key):
				var material := ShaderMaterial.new()
				material.shader = load("res://materials/archipelago_surface.gdshader")
				material.set_shader_parameter("base_color",original.albedo_color)
				material.set_shader_parameter("base_roughness",original.roughness)
				material.set_shader_parameter("surface_kind",kind)
				material.set_shader_parameter("bump_height",bump)
				styled_materials[style_key] = material
			node.set_surface_override_material(index,styled_materials[style_key])
	for child in node.get_children():
		_apply_surface_style(child, roles)

func _build_review_colliders(node: Node, trunks_only: bool = false) -> void:
	if node is MeshInstance3D and node.mesh != null and (not trunks_only or "_trunks_" in str(node.name) or "_pine_roots_" in str(node.name)):
		var mesh_node: MeshInstance3D = node
		var shape: ConcavePolygonShape3D = mesh_node.mesh.create_trimesh_shape()
		if shape != null:
			var body := StaticBody3D.new()
			body.name = "ReviewGeometryCollision"
			body.collision_layer = 1
			body.collision_mask = 0
			var collider := CollisionShape3D.new()
			collider.shape = shape
			body.add_child(collider)
			mesh_node.add_child(body)
	for child in node.get_children():
		if not child is StaticBody3D:
			_build_review_colliders(child,trunks_only)

func _capture_motion_review() -> void:
	if "--capture-motion-review" not in OS.get_cmdline_user_args():
		return
	capture_busy = true
	capture_deadline_usec = Time.get_ticks_usec()+36000000
	var was_visible := hud.visible
	hud.visible = false
	var folder_name := "motion_%d_%d" % [int(Time.get_unix_time_from_system()),Time.get_ticks_usec()]
	var folder := runtime_dir.path_join(folder_name)
	if DirAccess.make_dir_recursive_absolute(folder) != OK:
		get_tree().quit(4)
		return
	var receipt := _source_receipt()
	var cam := camera.duplicate(0) as Camera3D
	cam.current = false
	add_child(cam)
	cam.global_transform = camera.global_transform
	var focus := _vector(shots[shot_index].target)
	var offset := camera.global_position-focus
	cam.make_current()
	await RenderingServer.frame_post_draw
	var start := Time.get_ticks_usec()
	var utc_start := Time.get_unix_time_from_system()
	_begin_viewport_samples("motion_capture_with_image_io")
	var frames: Array[Dictionary] = []
	var last_slot := -1
	var total_bytes := 0
	var rendered := 0
	var reason := "duration"
	while Time.get_ticks_usec()-start < 24000000 and frames.size() < 720:
		var pose_us := Time.get_ticks_usec()-start
		var u := clampf(float(pose_us)/24000000.0,0,1)
		var eased := u*u*(3.0-2.0*u)
		cam.global_position = focus+offset.rotated(Vector3.UP,deg_to_rad(28.0)*eased)*lerpf(1.0,0.82,eased)
		cam.look_at(focus,Vector3.UP)
		var position := _array(cam.global_position)
		var basis := [_array(cam.global_basis.x),_array(cam.global_basis.y),_array(cam.global_basis.z)]
		await RenderingServer.frame_post_draw
		rendered += 1
		var sample_us := Time.get_ticks_usec()-start
		if sample_us >= 24000000:
			break
		var slot := int(floor(float(sample_us)*30.0/1000000.0))
		if slot <= last_slot:
			continue
		var jpg := get_viewport().get_texture().get_image().save_jpg_to_buffer(0.88)
		if jpg.is_empty() or total_bytes+jpg.size() > 536870912:
			reason = "image_or_byte_limit"
			break
		var filename := "frame_%06d.jpg" % frames.size()
		var file := FileAccess.open(folder.path_join(filename),FileAccess.WRITE)
		if file == null:
			reason = "open_failed"
			break
		file.store_buffer(jpg)
		file.flush()
		var error := file.get_error()
		file.close()
		if error != OK:
			reason = "write_failed"
			break
		var hash := HashingContext.new()
		hash.start(HashingContext.HASH_SHA256)
		hash.update(jpg)
		frames.append({"file":filename,"t_us":sample_us,"pose_us":pose_us,"position":position,"basis":basis,"sha256":hash.finish().hex_encode(),"missed_before":slot-last_slot-1})
		total_bytes += jpg.size()
		last_slot = slot
	var end_us := Time.get_ticks_usec()-start
	receipt["viewport_render_timing"] = _end_viewport_samples()
	var fov := cam.fov
	camera.make_current()
	cam.queue_free()
	hud.visible = was_visible
	capture_busy = false
	capture_deadline_usec = 0
	var complete := reason == "duration" and end_us >= 20000000 and end_us <= 30000000 and not frames.is_empty()
	var due := mini(720,int(floor(float(end_us)*30.0/1000000.0))+1)
	var recorded := _write_json(folder_name.path_join("motion.json"),{"status":"complete" if complete else "partial","reason":reason,"benchmark":false,"native_godot":true,"clock":"Time.get_ticks_usec","utc_start":utc_start,"end_us":end_us,"target_hz":30,"rendered_frames_seen":rendered,"missed_sample_deadlines":maxi(0,due-frames.size()),"bytes":total_bytes,"focus":_array(focus),"orbit_degrees":28,"final_radius_ratio":0.82,"renderer":RenderingServer.get_current_rendering_method(),"fov_degrees":fov,"source_receipt":receipt,"frames":frames})
	print("NATIVE_MOTION_REVIEW "+folder)
	get_tree().quit(0 if complete and recorded else 4)

func _on_frame_drawn() -> void:
	var now_usec := Time.get_ticks_usec()
	_sample_viewport_times(now_usec)
	if performance_sampling:
		drawn_count += 1
		if previous_draw_usec > 0:
			drawn_frame_samples.append(float(now_usec-previous_draw_usec)/1000.0)
	previous_draw_usec = now_usec

func _build_planar_reflection() -> void:
	if "--light-audit" in OS.get_cmdline_user_args():
		return
	planar_viewport = SubViewport.new()
	planar_viewport.name = "HarborPlanarReflection"
	planar_viewport.size = Vector2i(768,432)
	planar_viewport.world_3d = get_world_3d()
	planar_viewport.use_hdr_2d = true
	planar_viewport.msaa_3d = Viewport.MSAA_2X
	planar_viewport.screen_space_aa = Viewport.SCREEN_SPACE_AA_DISABLED
	planar_viewport.use_taa = false
	planar_viewport.render_target_update_mode = SubViewport.UPDATE_DISABLED
	add_child(planar_viewport)
	planar_camera = Camera3D.new()
	planar_viewport.add_child(planar_camera)
	planar_camera.current = true
	var reflection_env := review_environment.duplicate() as Environment
	reflection_env.tonemap_mode = Environment.TONE_MAPPER_LINEAR
	reflection_env.tonemap_exposure = 1.0
	reflection_env.adjustment_enabled = false
	reflection_env.glow_enabled = false
	reflection_env.ssr_enabled = false
	reflection_env.ssao_enabled = false
	reflection_env.ssil_enabled = false
	reflection_env.volumetric_fog_enabled = false
	reflection_env.fog_enabled = false
	planar_camera.environment = reflection_env
	ocean_material.set_shader_parameter("planar_texture",planar_viewport.get_texture())
	ocean_material.set_shader_parameter("planar_enabled",false)

func _update_planar_reflection(delta: float) -> void:
	if planar_viewport == null or ocean_material == null:
		return
	var source := get_viewport().get_camera_3d()
	if source == null or source.projection != Camera3D.PROJECTION_PERSPECTIVE:
		ocean_material.set_shader_parameter("planar_enabled",false)
		return
	var viewport_size := get_viewport().get_visible_rect().size
	var aspect := viewport_size.x / maxf(viewport_size.y,1.0)
	var source_transform := source.get_camera_transform()
	planar_clock += delta
	var changed := not source_transform.is_equal_approx(planar_last_transform) or not is_equal_approx(source.fov,planar_last_fov) or not is_equal_approx(aspect,planar_last_aspect) or source.keep_aspect != planar_last_keep_aspect
	if not changed and not planar_dirty:
		return
	if planar_clock < 1.0/30.0 and not planar_dirty:
		return
	planar_clock = 0.0
	planar_dirty = false
	planar_viewport.size = Vector2i(768,maxi(2,roundi(768.0/aspect)))
	planar_camera.keep_aspect = source.keep_aspect
	planar_camera.fov = source.fov
	planar_camera.near = source.near
	planar_camera.far = minf(source.far,320.0)
	planar_camera.cull_mask = source.cull_mask & ~REVIEW_WATER_LAYER
	var mirror := Vector3(1.0,-1.0,1.0)
	var b := source_transform.basis.orthonormalized()
	var reflected_basis := Basis(b.x*mirror,-b.y*mirror,b.z*mirror)
	var reflected_eye := source_transform.origin
	reflected_eye.y = -0.06-source_transform.origin.y
	planar_camera.global_transform = Transform3D(reflected_basis,reflected_eye)
	var tan_half := tan(deg_to_rad(source.fov)*0.5)
	var uv_scale := Vector2(1.0/(tan_half*aspect),1.0/tan_half)
	if source.keep_aspect == Camera3D.KEEP_WIDTH:
		uv_scale = Vector2(1.0/tan_half,aspect/tan_half)
	ocean_material.set_shader_parameter("planar_view",planar_camera.global_transform.affine_inverse())
	ocean_material.set_shader_parameter("planar_uv_scale",uv_scale)
	ocean_material.set_shader_parameter("planar_enabled",true)
	planar_viewport.render_target_update_mode = SubViewport.UPDATE_ONCE
	planar_last_transform = source_transform
	planar_last_fov = source.fov
	planar_last_aspect = aspect
	planar_last_keep_aspect = source.keep_aspect
	planar_requests += 1

func _begin_viewport_samples(kind: String) -> void:
	viewport_samples.clear()
	viewport_sample_dropped = 0
	viewport_sample_kind = kind
	viewport_sample_start_us = Time.get_ticks_usec()
	viewport_sample_request_start = planar_requests
	viewport_sample_previous_requests = planar_requests
	viewport_main_rid = get_viewport().get_viewport_rid()
	viewport_planar_rid = planar_viewport.get_viewport_rid() if planar_viewport != null else RID()
	RenderingServer.viewport_set_measure_render_time(viewport_main_rid,true)
	if viewport_planar_rid.is_valid():
		RenderingServer.viewport_set_measure_render_time(viewport_planar_rid,true)
	viewport_sampling = true

func _sample_viewport_times(now_us: int) -> void:
	if not viewport_sampling:
		return
	var request_delta := maxi(0,planar_requests-viewport_sample_previous_requests)
	viewport_sample_previous_requests = planar_requests
	if viewport_samples.size() >= VIEWPORT_SAMPLE_LIMIT:
		viewport_sample_dropped += 1
		return
	var row := {
		"t_us":now_us-viewport_sample_start_us,
		"engine_frames_drawn":Engine.get_frames_drawn(),
		"main_cpu_ms_last_reported":RenderingServer.viewport_get_measured_render_time_cpu(viewport_main_rid),
		"main_gpu_ms_last_reported":RenderingServer.viewport_get_measured_render_time_gpu(viewport_main_rid),
		"frame_setup_cpu_ms_last_reported":RenderingServer.get_frame_setup_time_cpu(),
		"planar_request_delta":request_delta,
		"planar_cpu_ms_last_reported":null,
		"planar_gpu_ms_last_reported":null
	}
	if request_delta > 0 and viewport_planar_rid.is_valid():
		row.planar_cpu_ms_last_reported = RenderingServer.viewport_get_measured_render_time_cpu(viewport_planar_rid)
		row.planar_gpu_ms_last_reported = RenderingServer.viewport_get_measured_render_time_gpu(viewport_planar_rid)
	viewport_samples.append(row)

func _end_viewport_samples() -> Dictionary:
	viewport_sampling = false
	var result := {
		"kind":viewport_sample_kind,
		"seconds":float(Time.get_ticks_usec()-viewport_sample_start_us)/1000000.0,
		"sample_limit":VIEWPORT_SAMPLE_LIMIT,
		"sample_count":viewport_samples.size(),
		"samples_dropped":viewport_sample_dropped,
		"planar_update_requests":planar_requests-viewport_sample_request_start,
		"gpu_cpu_unit":"milliseconds",
		"readback_note":"Latest reported per-viewport render times; may lag callback. Zero is unavailable/unresolved, not proof of zero cost. CPU excludes scripts and file IO. Not display presentation timing.",
		"planar_note":"Null when no update request since previous callback. Non-null is latest available readback tagged by request delta, not a proven same-frame GPU pass. Cached readings must not be accumulated; no combined main+planar timing is synthesized.",
		"samples":viewport_samples
	}
	RenderingServer.viewport_set_measure_render_time(viewport_main_rid,false)
	if viewport_planar_rid.is_valid():
		RenderingServer.viewport_set_measure_render_time(viewport_planar_rid,false)
	viewport_samples = []
	return result
