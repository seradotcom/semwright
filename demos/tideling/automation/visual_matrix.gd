extends SceneTree
var game
var output := ""
func _initialize() -> void:
	output = OS.get_cmdline_user_args()[0]
	run.call_deferred()
func capture(id: String) -> void:
	for i in range(100): await process_frame
	await RenderingServer.frame_post_draw
	root.get_texture().get_image().save_png(output.path_join(id+".png"))
func run() -> void:
	game = load("res://reef.tscn").instantiate()
	root.add_child(game)
	await capture("title-004")
	game.start_game()
	game.invulnerable = 10000
	await capture("game-004")
	game.paused = true
	for fish in game.population: fish.queue_free()
	game.population.clear()
	game.player.position = Vector3.ZERO
	for id in ["yellow","blue_gold","grouper"]:
		var fish = game.spawn_fish(false,load("res://species/"+id+".tres"))
		fish.position = Vector3(-3,0,0) if id=="yellow" else Vector3(3,0,0) if id=="blue_gold" else Vector3(4,2,0)
		fish.model.rotation.y = PI if id != "yellow" else 0
	await capture("hierarchy")
	game.paused = false
	# Deliberate visual fixture staging, not gameplay evidence.
	for i in range(35): game.rules.eat(load("res://species/fry.tres"))
	game.set_hero(2)
	await capture("stage-2")
	for i in range(85): game.rules.eat(load("res://species/fry.tres"))
	game.set_hero(3)
	await capture("stage-3")
	game.end_game(true)
	await capture("ending")
	game.audio.stop();game.ambience.stop();game.queue_free()
	await create_timer(.2).timeout
	quit()
