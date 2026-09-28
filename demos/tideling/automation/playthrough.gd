extends SceneTree
var game
var frames := 0
var max_stage := 1
var started := false
func _initialize() -> void:
	boot.call_deferred()
func boot() -> void:
	game = load("res://reef.tscn").instantiate()
	root.add_child(game)
	game.start_game()
	started = true
func _physics_process(_delta: float) -> bool:
	if not started: return false
	frames += 1
	max_stage = maxi(max_stage, game.rules.stage)
	if game.finished or frames > 11000:
		print("TIDELING_PLAYTHROUGH "+JSON.stringify({"kind":"GAME_RUNTIME_BOT", "seconds":game.rules.elapsed, "stage":game.rules.stage,"score":game.rules.score,"survived":game.rules.elapsed>=180,"max_stage":max_stage,"frames":frames,"note":"Deterministic input bot; does not establish human movement feel."}))
		started = false
		shutdown.call_deferred(game.finished and game.rules.elapsed >= 120 and max_stage == 3)
		return false
	var nearest = null
	var dist := INF
	var avoid := Vector3.ZERO
	for fish in game.population:
		var d: float = fish.position.distance_to(game.player.position)
		if game.rules.can_eat(fish.species) and d < dist:
			nearest = fish
			dist = d
		elif fish.species.predator and not game.rules.can_eat(fish.species) and d < 5:
			avoid += (game.player.position-fish.position).normalized() * (5-d)*2
	var direction := Vector3.ZERO
	var best := INF
	for candidate in range(24):
		var angle := candidate * TAU / 24.0
		var d := Vector3(cos(angle),sin(angle),0)
		var next: Vector3 = game.player.position + d * 4.5
		var cost: float = next.distance_to(nearest.position) if nearest else 0.0
		cost += maxf(0,absf(next.x)-22.5)*10
		cost += maxf(0,next.y-7.0)*10 + maxf(0,-4.7-next.y)*10
		for fish in game.population:
			if fish.species.predator and not game.rules.can_eat(fish.species):
				for horizon in [.3,.8,1.3]:
					var future: Vector3 = game.player.position + d * 5.8 * horizon
					var enemy: Vector3 = fish.position + fish.velocity * horizon
					cost += pow(maxf(0,4.0-future.distance_to(enemy)),2)*15
		if cost < best:
			best = cost
			direction = d
	# Approach edible targets directly when safe and already close.
	if nearest and dist < 2.5 and avoid.length() < .1:
		direction = (nearest.position-game.player.position).normalized()
	for action in ["swim_left","swim_right","swim_up","swim_down","burst"]: Input.action_release(action)
	if direction.x > 0: Input.action_press("swim_right",direction.x)
	if direction.x < 0: Input.action_press("swim_left",-direction.x)
	if direction.y > 0: Input.action_press("swim_up",direction.y)
	if direction.y < 0: Input.action_press("swim_down",-direction.y)
	return false
func shutdown(ok: bool) -> void:
	game.audio.stop(); game.ambience.stop(); game.queue_free()
	for i in range(4): await process_frame
	await create_timer(.15, true, false, true).timeout
	quit(0 if ok else 1)
