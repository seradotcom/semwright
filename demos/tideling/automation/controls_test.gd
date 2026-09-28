extends SceneTree
# Real Input events exercise desktop controls; this does not certify physical hardware.
var game
var results := {}
func _initialize() -> void:
    run.call_deferred()
func frames(count: int) -> void:
    for i in range(count): await physics_frame
func key(code: Key, pressed: bool) -> void:
    var event := InputEventKey.new()
    event.physical_keycode = code
    event.pressed = pressed
    Input.parse_input_event(event)
func stick(value: float) -> void:
    var event := InputEventJoypadMotion.new()
    event.device = 0
    event.axis = JOY_AXIS_LEFT_X
    event.axis_value = value
    Input.parse_input_event(event)
func run() -> void:
    game = load("res://reef.tscn").instantiate()
    root.add_child(game)
    game.start_game()
    game.invulnerable = 100
    # Isolate movement measurements from food/predator collisions.
    for fish in game.population: fish.queue_free()
    game.population.clear()
    game.spawn_clock = -100
    await frames(3)
    var start: Vector3 = game.player.position
    key(KEY_D, true)
    await frames(15)
    results["keyboard_response_within_250ms"] = game.player.position.x > start.x + .5 and game.motion.x > 4
    await frames(20)
    key(KEY_D, false)
    var released: Vector3 = game.player.position
    await frames(60)
    results["coasts_under_1_3_units_and_stops"] = game.player.position.distance_to(released) < 1.3 and game.motion.length() < .1
    results["idle_animation_when_stopped"] = "_Idle" in game.player_animation.current_animation
    key(KEY_A, true)
    await frames(3)
    results["authored_turn_animation"] = game.facing == -1 and "_Turn" in game.player_animation.current_animation
    key(KEY_A, false)
    await frames(75)
    var deadzone_start: Vector3 = game.player.position
    stick(.10)
    await frames(30)
    results["stick_deadzone"] = game.player.position.distance_to(deadzone_start) < .05
    stick(.75)
    await frames(30)
    results["analog_stick_moves"] = game.player.position.x > deadzone_start.x + .4
    stick(0)
    await frames(60)
    var dash_start: Vector3 = game.player.position
    key(KEY_SPACE, true)
    await frames(3)
    key(KEY_SPACE, false)
    results["dash_animation_and_acceleration"] = game.motion.length() > 10 and "_Dash" in game.player_animation.current_animation
    await frames(12)
    results["dash_distance"] = game.player.position.distance_to(dash_start) > 3
    key(KEY_SPACE, true)
    await frames(2)
    key(KEY_SPACE, false)
    results["dash_cooldown_blocks_repeat"] = game.rules.dash_time == 0 and game.rules.dash_cooldown > 1
    game.stage_pulse = 1
    game.spawn_clock = .6
    game.particles_at(game.player.position, Color.WHITE, 3)
    game.start_game()
    results["retry_clears_transients"] = game.stage_pulse == 0 and game.spawn_clock == 0 and game.effects.is_empty()
    var passed := true
    for result in results.values(): passed = passed and bool(result)
    print("TIDELING_CONTROLS "+JSON.stringify({"passed":passed,"checks":results,"scope":"Synthetic keyboard and joypad events; no physical controller or human feel claim."}))
    game.audio.stop(); game.ambience.stop(); game.queue_free()
    await frames(4)
    await create_timer(.2).timeout
    quit(0 if passed else 1)
