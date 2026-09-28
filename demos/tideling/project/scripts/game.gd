extends "res://scripts/reef.gd"

const Rules = preload("res://scripts/rules.gd")
const Fish = preload("res://scripts/fish.gd")
var rules := Rules.new()
var player: Area3D
var player_model: Node3D
var player_animation: AnimationPlayer
var population: Array[ReefFish] = []
var species_list: Array[TidelingSpecies] = []
var motion := Vector3.ZERO
var facing := 1.0
var active := false
var paused := false
var finished := false
var invulnerable := 0.0
var spawn_clock := 0.0
var stage_pulse := 0.0
var ui: CanvasLayer
var menu: Control
var title: Label
var subtitle: Label
var dive: Button
var hud: Control
var growth: ProgressBar
var cooldown: ProgressBar
var stage_label: Label
var score_label: Label
var combo_label: Label
var timer_label: Label
var hint: Label
var pulse: ColorRect
var mute_button: Button
var audio: AudioStreamPlayer
var ambience: AudioStreamPlayer
var effects: Array[Dictionary] = []
var reduced_motion := false

func _ready() -> void:
	super._ready()
	name = "TidelingReef"
	setup_input()
	for id in ["fry", "yellow", "blue", "butterfly", "puffer", "barracuda", "grouper", "blue_gold"]:
		var data := load("res://species/" + id + ".tres") as TidelingSpecies
		if data.enabled and data.spawn_weight > 0: species_list.append(data)
	player = Area3D.new()
	player.name = "Fish_Player"
	player.collision_layer = 1
	player.collision_mask = 6
	add_child(player)
	var shape := CollisionShape3D.new()
	shape.name = "PlayerCollision"
	shape.shape = SphereShape3D.new()
	(shape.shape as SphereShape3D).radius = .38
	player.add_child(shape)
	player.area_entered.connect(encounter)
	player.position = Vector3(7, 1, 1)
	set_hero(1)
	player_model.scale = Vector3.ONE * 1.4
	for i in range(42): spawn_fish(true)
	build_ui()
	audio = AudioStreamPlayer.new()
	add_child(audio)
	ambience = AudioStreamPlayer.new()
	ambience.volume_db = -20
	add_child(ambience)
	set_ambience(1)
	for arg in OS.get_cmdline_user_args():
		if arg == "play-capture":
			start_game()

func setup_input() -> void:
	var keys := {"swim_left":[KEY_A,KEY_LEFT], "swim_right":[KEY_D,KEY_RIGHT], "swim_up":[KEY_W,KEY_UP], "swim_down":[KEY_S,KEY_DOWN], "burst":[KEY_SPACE]}
	for action in keys:
		if not InputMap.has_action(action): InputMap.add_action(action)
		for key in keys[action]:
			var event := InputEventKey.new()
			event.physical_keycode = key
			InputMap.action_add_event(action, event)
		if action != "burst":
			var joy := InputEventJoypadMotion.new()
			joy.axis = JOY_AXIS_LEFT_X if action in ["swim_left", "swim_right"] else JOY_AXIS_LEFT_Y
			joy.axis_value = -1 if action in ["swim_left", "swim_up"] else 1
			InputMap.action_add_event(action, joy)
	var button := InputEventJoypadButton.new()
	button.button_index = JOY_BUTTON_A
	InputMap.action_add_event("burst", button)

func set_hero(stage: int) -> void:
	if player_model: player_model.queue_free()
	var id: String = ["hero_juvenile", "hero_medium", "hero_mature"][stage-1]
	player_model = (load("res://assets/" + id + ".glb") as PackedScene).instantiate()
	player_model.name = "HeroStage" + str(stage)
	player.add_child(player_model)
	player_model.scale = Vector3.ONE * [.50,.70,.95][stage-1]
	player_animation = player_model.find_child("AnimationPlayer", true, false) as AnimationPlayer
	play_action("Swim_loop")
	var shape := player.get_node("PlayerCollision") as CollisionShape3D
	(shape.shape as SphereShape3D).radius = [.38,.52,.69][stage-1]

func play_action(fragment: String) -> void:
	if not player_animation: return
	for clip in player_animation.get_animation_list():
		if fragment in clip:
			player_animation.get_animation(clip).loop_mode = Animation.LOOP_LINEAR if "loop" in fragment else Animation.LOOP_NONE
			player_animation.play(clip, .12)
			if "loop" not in fragment: player_animation.queue(find_swim())
			return

func find_swim() -> String:
	for clip in player_animation.get_animation_list():
		if "Swim_loop" in clip: return clip
	return ""

func spawn_fish(initial := false, forced: TidelingSpecies = null) -> ReefFish:
	var data := forced
	if data == null:
		var weight := 0.0
		for entry in species_list: weight += entry.spawn_weight
		var pick := rng.randf() * weight
		for entry in species_list:
			pick -= entry.spawn_weight
			if pick <= 0:
				data = entry
				break
	var fish := Fish.new() as ReefFish
	add_child(fish)
	fish.setup(data)
	fish.phase = rng.randf() * TAU
	fish.heading = 1.0 if rng.randf() > .5 else -1.0
	fish.position = Vector3(rng.randf_range(-25,25) if initial else -fish.heading * 26, rng.randf_range(-5,8), 0)
	if data.predator and fish.position.distance_to(player.position) < 8:
		fish.position.x = -22
	population.append(fish)
	return fish

func start_game() -> void:
	rules = Rules.new()
	active = true
	paused = false
	finished = false
	motion = Vector3.ZERO
	facing = 1
	invulnerable = 3.0
	player.position = Vector3(0, 0, 0)
	set_hero(1)
	set_ambience(1)
	for fish in population: fish.queue_free()
	population.clear()
	for i in range(42): spawn_fish(true)
	menu.hide()
	hud.show()
	dive.release_focus()
	hint.text = "The small brackets mark a meal. Keep clear of hunters."

func encounter(other: Area3D) -> void:
	if not active or paused or not other is ReefFish: return
	var fish := other as ReefFish
	if fish.eaten: return
	if rules.can_eat(fish.species):
		var previous := rules.stage
		if not rules.eat(fish.species): return
		fish.eaten = true
		particles_at(fish.position, Color("ffd895"), 9)
		population.erase(fish)
		fish.queue_free()
		play_action("Bite")
		sound("eat")
		if rules.stage != previous:
			set_hero(rules.stage)
			stage_pulse = 1.0
			invulnerable = 2.0
			particles_at(player.position, Color("a8f7dd"), 24)
			sound("grow")
			set_ambience(rules.stage)
	elif fish.species.predator and invulnerable <= 0:
		end_game(false)

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		match event.physical_keycode:
			KEY_ENTER:
				if not active: start_game()
			KEY_R:
				if finished: start_game()
			KEY_M: toggle_mute()
			KEY_ESCAPE:
				if active:
					paused = not paused
					hint.text = "Paused · Esc to swim again" if paused else ""
	if event is InputEventJoypadButton and event.pressed and event.button_index == JOY_BUTTON_START:
		if not active: start_game()
		else:
			paused = not paused
			hint.text = "Paused · Start to swim again" if paused else ""

func _physics_process(delta: float) -> void:
	if paused: return
	if active:
		rules.tick(delta)
		invulnerable = maxf(0, invulnerable - delta)
		var direction := Input.get_vector("swim_left", "swim_right", "swim_up", "swim_down")
		var target := Vector3(direction.x, -direction.y, 0)
		if absf(direction.x) > .08: facing = signf(direction.x)
		if Input.is_action_just_pressed("burst") and rules.dash():
			if target.length() < .1: target = Vector3(facing, 0, 0)
			motion = target.normalized() * 17
			play_action("Dash")
			sound("dash")
		if rules.dash_time <= 0:
			motion = motion.lerp(target * (5.4 + rules.stage * .4), 1 - exp(-delta * (7.5 if target.length() > .1 else 5.0)))
		elif not reduced_motion and Engine.get_physics_frames() % 3 == 0:
			particles_at(player.position - Vector3(facing*.3,0,0), Color("8ed9d4"), 1)
		player.position += motion * delta
		player.position.x = clampf(player.position.x, -25, 25)
		player.position.y = clampf(player.position.y, -5.8, 8.4)
		player_model.rotation.y = lerp_angle(player_model.rotation.y, 0 if facing > 0 else PI, delta * 12)
		player_model.rotation.z = lerp_angle(player_model.rotation.z, motion.y * .035 * facing, delta * 6)
		spawn_clock += delta
		if spawn_clock > .65 and population.size() < 45:
			spawn_clock = 0
			spawn_fish()
		# Re-evaluate overlaps after growth, so a newly edible fish needs no re-entry.
		for other in player.get_overlapping_areas(): encounter(other)
		if rules.elapsed >= Rules.DURATION: end_game(true)
		update_hud()
	elif not finished:
		player.position.y = 1.0 + sin(clock*.8)*.35
		player_model.rotation.y = -.2
	for fish in population:
		fish.swim(delta, player.position, rules.stage, clock, active, population)

func _process(delta: float) -> void:
	super._process(delta)
	if not is_instance_valid(player): return
	stage_pulse = maxf(0, stage_pulse - delta)
	if pulse: pulse.color.a = 0 if reduced_motion else stage_pulse * .07
	var target := Vector3(clampf(player.position.x*.65,-16,16), clampf(player.position.y*.40,-1.5,3.5),32) if active else Vector3(0,1,32)
	camera.position = camera.position.lerp(target, 1-exp(-delta*1.6))
	camera.size = lerpf(camera.size, (18.5 + (rules.stage-1)*2.3 + stage_pulse*.8) if active else 23.0, 1-exp(-delta*1.2))
	for i in range(effects.size()-1,-1,-1):
		var e: Dictionary = effects[i]
		e.life -= delta
		var node: Node3D = e.node
		node.position += e.velocity * delta
		node.scale *= maxf(0, 1-delta*2)
		if e.life <= 0:
			node.queue_free()
			effects.remove_at(i)

func particles_at(at: Vector3, color: Color, count: int) -> void:
	if reduced_motion: return
	for i in range(count):
		var obj := MeshInstance3D.new()
		var mesh := SphereMesh.new()
		mesh.radius = .045
		mesh.height = .09
		mesh.radial_segments = 8
		mesh.rings = 4
		obj.mesh = mesh
		obj.material_override = simple_material(color)
		add_child(obj)
		obj.position = at
		effects.append({"node":obj, "velocity":Vector3(rng.randf_range(-1.8,1.8),rng.randf_range(-.8,2.0),.4), "life":.7})

func sound(id: String) -> void:
	audio.stream = load("res://assets/"+id+".wav")
	audio.volume_db = -13
	audio.play()

func set_ambience(stage: int) -> void:
	if not ambience: return
	ambience.stream = load("res://assets/ambience_"+str(stage)+".wav")
	ambience.play()

func end_game(survived: bool) -> void:
	active = false
	finished = true
	menu.show()
	hud.hide()
	title.text = "A little bigger." if survived else "The reef goes on."
	title.add_theme_font_size_override("font_size", 52)
	subtitle.text = ("You found your place in the blue." if survived else "A hunter caught you. Take another breath.") + "\n\n" + str(rules.score) + " points   ·   " + str(int(rules.elapsed)) + " seconds   ·   Stage " + str(rules.stage)
	dive.text = "Dive again"
	dive.grab_focus()
	if not survived: sound("caught")

func label(text: String, size: int, at: Vector2, parent: Node, color := Color("f4ecd9")) -> Label:
	var node := Label.new()
	node.text = text
	node.position = at
	node.add_theme_font_size_override("font_size", size)
	node.add_theme_color_override("font_color", color)
	node.add_theme_color_override("font_shadow_color", Color(0.015,.07,.09,.9))
	node.add_theme_constant_override("shadow_offset_x", 1)
	node.add_theme_constant_override("shadow_offset_y", 2)
	parent.add_child(node)
	return node

func bar(at: Vector2, width: float, parent: Node, color: Color) -> ProgressBar:
	var node := ProgressBar.new()
	node.position = at
	node.max_value = 1
	node.show_percentage = false
	var bg := StyleBoxFlat.new()
	bg.bg_color = Color(.02,.12,.16,.8)
	var fill := StyleBoxFlat.new()
	fill.bg_color = color
	node.add_theme_stylebox_override("background", bg)
	node.add_theme_stylebox_override("fill", fill)
	parent.add_child(node)
	node.size = Vector2(width, 5)
	return node

func build_ui() -> void:
	ui = CanvasLayer.new()
	ui.name = "ReefInterface"
	add_child(ui)
	var marks := Control.new()
	marks.set_script(load("res://scripts/food_marks.gd"))
	marks.game = self
	marks.mouse_filter = Control.MOUSE_FILTER_IGNORE
	ui.add_child(marks)
	menu = Control.new()
	menu.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	ui.add_child(menu)
	var shade := ColorRect.new()
	shade.size = Vector2(1280,800)
	shade.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var mat := ShaderMaterial.new()
	mat.shader = load("res://scripts/menu.gdshader")
	shade.material = mat
	menu.add_child(shade)
	title = label("TIDELING", 78, Vector2(66,180), menu)
	if ResourceLoader.exists("res://assets/title.ttf"):
		title.add_theme_font_override("font", load("res://assets/title.ttf"))
	subtitle = label("A small life. A vast blue world.\nSwim, grow, and find your place in the reef.", 22, Vector2(70,295), menu, Color("b7dad6"))
	dive = Button.new()
	dive.text = "Dive in"
	dive.position = Vector2(70,418)
	dive.size = Vector2(210,52)
	dive.add_theme_font_size_override("font_size",20)
	var style := StyleBoxFlat.new()
	style.bg_color = Color("efbf89")
	style.corner_radius_top_left = 4
	style.corner_radius_bottom_right = 4
	dive.add_theme_stylebox_override("normal",style)
	var hover := style.duplicate() as StyleBoxFlat
	hover.bg_color = Color("ffdda6")
	dive.add_theme_stylebox_override("hover",hover)
	dive.add_theme_stylebox_override("pressed",hover)
	dive.add_theme_color_override("font_color", Color("183847"))
	dive.add_theme_color_override("font_hover_color", Color("183847"))
	dive.add_theme_color_override("font_focus_color", Color("183847"))
	dive.add_theme_color_override("font_pressed_color", Color("183847"))
	menu.add_child(dive)
	dive.pressed.connect(start_game)
	dive.grab_focus()
	label("WASD / arrows · swim    Space · burst\nLeft stick + A also work. Three minutes. One reef.",16,Vector2(70,509),menu,Color("b7dad6"))
	var motion_button := CheckButton.new()
	motion_button.text = "Reduced effects"
	motion_button.position = Vector2(62, 590)
	motion_button.toggled.connect(func(value: bool): reduced_motion = value)
	menu.add_child(motion_button)
	mute_button = Button.new()
	mute_button.text = "Sound on · M"
	mute_button.position = Vector2(1080,734)
	mute_button.size = Vector2(150,36)
	mute_button.pressed.connect(toggle_mute)
	ui.add_child(mute_button)
	hud = Control.new()
	ui.add_child(hud)
	var edge_shade := ColorRect.new()
	edge_shade.size = Vector2(1280,800)
	edge_shade.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var edge_material := ShaderMaterial.new()
	edge_material.shader = load("res://scripts/hud.gdshader")
	edge_shade.material = edge_material
	hud.add_child(edge_shade)
	stage_label = label("Juvenile", 23, Vector2(44,32),hud)
	growth = bar(Vector2(44,71),270,hud,Color("efbf89"))
	score_label = label("0",28,Vector2(1060,28),hud)
	timer_label = label("3:00",16,Vector2(1135,67),hud,Color("b7dad6"))
	combo_label = label("",23,Vector2(595,36),hud,Color("ffce89"))
	label("BURST · SPACE / A",13,Vector2(44,726),hud,Color("b7dad6"))
	cooldown = bar(Vector2(44,753),150,hud,Color("a6dcd4"))
	hint = label("",18,Vector2(320,724),hud)
	hud.hide()
	pulse = ColorRect.new()
	pulse.size = Vector2(1280,800)
	pulse.color = Color(.5,1,.85,0)
	pulse.mouse_filter = Control.MOUSE_FILTER_IGNORE
	ui.add_child(pulse)

func toggle_mute() -> void:
	var muted := not AudioServer.is_bus_mute(0)
	AudioServer.set_bus_mute(0, muted)
	mute_button.text = "Sound off · M" if muted else "Sound on · M"

func update_hud() -> void:
	stage_label.text = ["Juvenile", "Explorer", "Reefkeeper"][rules.stage-1]
	growth.value = rules.growth_progress()
	cooldown.value = 1-rules.dash_cooldown/Rules.DASH_COOLDOWN
	score_label.text = str(rules.score)
	combo_label.text = str(rules.multiplier())+"×" if rules.multiplier()>1 else ""
	var remaining := maxi(0, int(ceil(Rules.DURATION-rules.elapsed)))
	timer_label.text = "%d:%02d" % [remaining/60,remaining%60]
	if rules.elapsed > 8 and not paused: hint.text = ""

