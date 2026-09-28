extends Node3D

var motes: Array[Node3D] = []
var fronds: Array[Node3D] = []
var clock := 0.0
var camera: Camera3D
var rng := RandomNumberGenerator.new()

func asset(id: String, at: Vector3, size: float, parent: Node = self) -> Node3D:
	var scene := load("res://assets/" + id + ".glb") as PackedScene
	var obj := scene.instantiate() as Node3D
	obj.name = "Reef_" + id
	parent.add_child(obj)
	obj.position = at
	obj.scale = Vector3.ONE * size
	return obj

func simple_material(color: Color) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = color
	m.roughness = 0.8
	return m

func _ready() -> void:
	rng.seed = 281906
	build_reef()
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("capture="):
			capture.call_deferred(arg.trim_prefix("capture="))

func build_reef() -> void:
	var env := WorldEnvironment.new()
	env.name = "ReefAtmosphere"
	env.environment = Environment.new()
	env.environment.background_mode = Environment.BG_COLOR
	env.environment.background_color = Color("083c50")
	env.environment.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.environment.ambient_light_color = Color("648a94")
	env.environment.ambient_light_energy = 0.34
	env.environment.tonemap_mode = Environment.TONE_MAPPER_FILMIC
	env.environment.fog_enabled = true
	env.environment.fog_light_color = Color("0d4257")
	env.environment.fog_density = 0.022
	add_child(env)
	var sun := DirectionalLight3D.new()
	sun.name = "SurfaceSun"
	sun.rotation_degrees = Vector3(-34, -20, -18)
	sun.light_color = Color("ffe1ac")
	sun.light_energy = 1.1
	sun.shadow_enabled = true
	add_child(sun)
	var fill := OmniLight3D.new()
	fill.position = Vector3(0, 3, 8)
	fill.light_color = Color("8fe0d6")
	fill.light_energy = 0.65
	fill.omni_range = 36
	add_child(fill)
	var floor_mesh := MeshInstance3D.new()
	floor_mesh.name = "ReefSand"
	var plane := PlaneMesh.new()
	plane.size = Vector2(100, 80)
	floor_mesh.mesh = plane
	floor_mesh.position.y = -8.4
	var sand := ShaderMaterial.new()
	sand.shader = load("res://scripts/sand.gdshader")
	floor_mesh.material_override = sand
	add_child(floor_mesh)
	for layer in range(3):
		for i in range(19):
			var x := (i - 9) * 3.2 + rng.randf_range(-0.6, 0.6)
			var z := -float(layer) * 7.0 - 3.0
			var rock := asset("rock", Vector3(x, -8.1, z), rng.randf_range(1.3, 3.0))
			rock.rotation.y = rng.randf() * TAU
			var kind: String = ["coral", "fan", "kelp", "anemone"][rng.randi_range(0, 3)]
			var item := asset(kind, Vector3(x, -7.5, z - 0.5), rng.randf_range(1.2, 2.8) * (1.3 if layer > 0 else 1.0))
			item.rotation.y = rng.randf_range(-0.45, 0.45)
			if kind == "kelp":
				item.scale *= .58
				if layer == 0: darken(item)
				else: darken(item, Color("1c505c"))
				fronds.append(item)
	# Tall distant walls and dark near-edge fronds frame the navigable lagoon.
	for side in [-1.0, 1.0]:
		for i in range(4):
			asset("rock", Vector3(side * (20 + i * 2), -7, -8), 4.0 + i)
			var item := asset("kelp", Vector3(side * (18 + i * 4), -9.0, 5 + i), 2.6)
			darken(item)
			fronds.append(item)
			asset("fan", Vector3(side * (19 + i * 3), -6.3, -13), 4.0)
	for i in range(12):
		asset("shell", Vector3(rng.randf_range(-24, 24), -8.1, rng.randf_range(-3, 4)), rng.randf_range(.45, .8))
	for i in range(8):
		var ray := MeshInstance3D.new()
		ray.name = "SunRibbon"
		var q := QuadMesh.new()
		q.size = Vector2(rng.randf_range(.8, 2.5), 34)
		ray.mesh = q
		ray.position = Vector3(-24 + i * 7, 7, -12 - i * .3)
		ray.rotation.z = -.32
		var m := ShaderMaterial.new()
		m.shader = load("res://scripts/ray.gdshader")
		m.set_shader_parameter("phase", float(i))
		ray.material_override = m
		add_child(ray)
	var dustmat := simple_material(Color("b5e2d6"))
	dustmat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	var dustmesh := SphereMesh.new()
	dustmesh.radius = .022
	dustmesh.height = .044
	dustmesh.radial_segments = 8
	dustmesh.rings = 4
	for i in range(95):
		var mote := MeshInstance3D.new()
		mote.mesh = dustmesh
		mote.material_override = dustmat
		mote.position = Vector3(rng.randf_range(-30,30), rng.randf_range(-8,15), rng.randf_range(-14,3))
		add_child(mote)
		motes.append(mote)
	camera = Camera3D.new()
	camera.name = "ReefCamera"
	camera.projection = Camera3D.PROJECTION_ORTHOGONAL
	camera.size = 23
	camera.position = Vector3(0, 1, 32)
	camera.current = true
	add_child(camera)

func _process(delta: float) -> void:
	clock += delta
	for i in range(motes.size()):
		motes[i].position.y += delta * (.08 + (i % 4) * .035)
		if motes[i].position.y > 14: motes[i].position.y = -8
	for i in range(fronds.size()):
		fronds[i].rotation.z = sin(clock * .6 + i) * .045

func capture(path: String) -> void:
	for i in range(30): await get_tree().process_frame
	await RenderingServer.frame_post_draw
	get_viewport().get_texture().get_image().save_png(path)
	get_tree().quit()

func darken(node: Node, color := Color("073f42")) -> void:
	if node is MeshInstance3D:
		var m := simple_material(color)
		m.cull_mode = BaseMaterial3D.CULL_DISABLED
		node.material_override = m
	for child in node.get_children(): darken(child, color)
