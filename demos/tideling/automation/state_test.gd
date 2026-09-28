extends SceneTree
func _initialize() -> void:
	run.call_deferred()
func run() -> void:
	var game = load("res://reef.tscn").instantiate()
	root.add_child(game)
	game.start_game()
	game.paused = true
	var before: float = game.rules.elapsed
	for i in range(4): await physics_frame
	var pause_ok: bool = game.rules.elapsed == before
	game.paused = false
	game.invulnerable = 100
	game.rules.elapsed = 179.95
	for i in range(10): await physics_frame
	var timer_ok: bool = game.finished and game.title.text == "A little bigger."
	game.start_game()
	var retry_ok: bool = game.rules.stage == 1 and game.rules.score == 0 and game.rules.elapsed == 0 and game.active and not game.finished
	var previous := AudioServer.is_bus_mute(0)
	game.toggle_mute()
	var mute_ok := AudioServer.is_bus_mute(0) != previous
	game.toggle_mute()
	var ok := pause_ok and timer_ok and retry_ok and mute_ok
	print("TIDELING_STATES "+JSON.stringify({"passed":ok,"pause_freezes_game":pause_ok,"timer_ends_run":timer_ok,"retry_resets_game":retry_ok,"mute_toggles":mute_ok}))
	game.audio.stop(); game.ambience.stop(); game.queue_free()
	for i in range(4): await process_frame
	await create_timer(.2).timeout
	quit(0 if ok else 1)
