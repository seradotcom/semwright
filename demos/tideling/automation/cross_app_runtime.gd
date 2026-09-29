extends SceneTree
func _initialize() -> void:
	run.call_deferred()
func run() -> void:
	var game = load("res://reef.tscn").instantiate()
	root.add_child(game)
	game.start_game()
	game.active = false
	var candidates: Array = []
	for fish in game.population:
		if fish.species.id == "blue_gold": candidates.append(fish)
	var auto_spawned := candidates.size()
	if auto_spawned == 0:
		print("TIDELING_CROSS_APP_RUNTIME "+JSON.stringify({"passed":false,"auto_spawned":0}))
		quit(1)
		return
	var specimen = candidates[0]
	var animation_present: bool = specimen.animation != null and specimen.animation.is_playing()
	var animation_clips: PackedStringArray = specimen.animation.get_animation_list() if specimen.animation else PackedStringArray()
	var skeleton := specimen.model.find_child("Skeleton3D",true,false) as Skeleton3D
	var tail: int = skeleton.find_bone("Tail") if skeleton else -1
	var initial_pose: Quaternion = skeleton.get_bone_pose_rotation(tail) if tail >= 0 else Quaternion.IDENTITY
	for i in range(16): await physics_frame
	var bones_moving: bool = tail >= 0 and not initial_pose.is_equal_approx(skeleton.get_bone_pose_rotation(tail))
	var grouped: bool = specimen.is_in_group("PreyTier2") and specimen.collision_layer == 2
	for fish in game.population:
		if fish != specimen: fish.queue_free()
	game.population = [specimen] as Array[ReefFish]
	for i in range(3): await physics_frame
	game.active = true
	specimen.position = game.player.position
	for i in range(4): await physics_frame
	var denied: bool = game.rules.score == 0 and is_instance_valid(specimen)
	specimen.position = Vector3(20,0,0)
	# Supply ordinary prey at the player's collision volume, letting real consumption grow it.
	for i in range(12):
		var food = game.spawn_fish(false,load("res://species/yellow.tres"))
		food.position = game.player.position
		for j in range(3): await physics_frame
	var grown: bool = game.rules.stage == 2
	var before: int = game.rules.score
	specimen.position = game.player.position
	for i in range(5): await physics_frame
	var eaten: bool = not is_instance_valid(specimen) and game.rules.score > before
	var passed := auto_spawned > 0 and animation_present and bones_moving and grouped and denied and grown and eaten
	print("TIDELING_CROSS_APP_RUNTIME "+JSON.stringify({"passed":passed,"auto_spawned":auto_spawned,"swim_animation_playing":animation_present,"animated_tail_moves":bones_moving,"animation_clips":animation_clips,"prey_tier_2_and_collision_layer":grouped,"inedible_stage_1":denied,"grew_through_collisions":grown,"consumed_stage_2":eaten,"score_before":before,"score_after":game.rules.score}))
	game.audio.stop();game.ambience.stop();game.queue_free()
	for i in range(4): await process_frame
	await create_timer(.2).timeout
	quit(0 if passed else 1)
