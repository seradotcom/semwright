extends SceneTree

func _initialize() -> void:
	run.call_deferred()

func check(ok: bool, message: String) -> void:
	if not ok:
		push_error(message)
		quit(1)

func run() -> void:
	var game = load("res://reef.tscn").instantiate()
	root.add_child(game)
	game.start_game()
	game.active = false
	var animations_playing: bool = game.player_animation != null and game.player_animation.is_playing()
	for fish in game.population:
		animations_playing = animations_playing and fish.animation != null and fish.animation.is_playing()
	for fish in game.population: fish.queue_free()
	game.population.clear()
	for i in range(3): await physics_frame
	game.active = true
	var bluegold = load("res://species/blue_gold.tres")
	check(not bluegold.enabled and bluegold.spawn_weight == 0, "Baseline unexpectedly contains BlueGoldFish")
	var specimen = game.spawn_fish(false, bluegold)
	specimen.position = game.player.position
	for i in range(4): await physics_frame
	var blocked_at_one: bool = game.rules.score == 0 and is_instance_valid(specimen)
	var yellow = load("res://species/yellow.tres")
	for i in range(12): game.rules.eat(yellow)
	game.set_hero(game.rules.stage)
	var before: int = game.rules.score
	specimen.position = game.player.position
	for i in range(4): await physics_frame
	var eaten_at_two: bool = game.rules.score > before and not is_instance_valid(specimen)
	Input.action_press("swim_right")
	var start: Vector3 = game.player.position
	for i in range(30): await physics_frame
	Input.action_release("swim_right")
	var movement: bool = game.player.position.x > start.x + .5
	Input.action_press("burst")
	await physics_frame
	Input.action_release("burst")
	await physics_frame
	var dash: bool = game.rules.dash_cooldown > 0 and game.motion.length() > 10
	game.invulnerable = 0
	var hunter = game.spawn_fish(false, load("res://species/grouper.tres"))
	hunter.position = game.player.position
	for i in range(5): await physics_frame
	var died: bool = game.finished and not game.active
	var record := {"kind":"GAME_RUNTIME", "hero_and_fauna_swim_playing":animations_playing, "bluegold_blocked_stage_1":blocked_at_one, "bluegold_consumed_stage_2":eaten_at_two, "keyboard_moves_player":movement, "dash_accelerates":dash, "predator_ends_run":died, "score":game.rules.score, "stage":game.rules.stage, "note":"Direct runtime test, not Semwright proof. Species explicitly injected only in this external test."}
	print("TIDELING_RUNTIME_PROOF "+JSON.stringify(record))
	game.audio.stop()
	game.ambience.stop()
	game.queue_free()
	for i in range(4): await process_frame
	# Let the audio server release its command queue before engine shutdown.
	await create_timer(.15).timeout
	quit(0 if animations_playing and blocked_at_one and eaten_at_two and died and movement and dash else 1)
