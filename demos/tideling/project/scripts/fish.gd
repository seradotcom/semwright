extends Area3D
class_name ReefFish

var species: TidelingSpecies
var model: Node3D
var velocity := Vector3.ZERO
var heading := 1.0
var phase := 0.0
var animation: AnimationPlayer
var eaten := false

func setup(data: TidelingSpecies) -> void:
	species = data
	name = "Fish_" + data.id
	collision_layer = 2 if not data.predator else 4
	collision_mask = 1
	add_to_group("PreyTier" + str(data.edible_stage))
	if data.predator: add_to_group("PredatorTier" + str(data.edible_stage))
	model = (load(data.asset_path) as PackedScene).instantiate()
	add_child(model)
	model.scale = Vector3.ONE * data.size
	var shape := CollisionShape3D.new()
	var ball := SphereShape3D.new()
	ball.radius = data.size * .8
	shape.shape = ball
	add_child(shape)
	animation = model.find_child("AnimationPlayer", true, false) as AnimationPlayer
	if animation:
		for clip in animation.get_animation_list():
			# Godot consumes the _loop import hint and renames the clip.
			if String(clip).trim_suffix("_loop").ends_with("_Swim"):
				animation.get_animation(clip).loop_mode = Animation.LOOP_LINEAR
				animation.play(clip)
				animation.speed_scale = 1.0 / sqrt(data.size + .3)
				break

func swim(delta: float, target: Vector3, stage: int, time: float, playing: bool, neighbors: Array[ReefFish] = []) -> void:
	var move := Vector3(heading, sin(time * 1.4 + phase) * .18, 0)
	var distance := position.distance_to(target)
	if playing and species.predator and species.edible_stage > stage and distance < 7.5:
		move = (target - position).normalized()
	elif playing and species.edible_stage <= stage and distance < 2.7:
		move = (position - target).normalized()
	if species.schooling and not species.predator and distance > 2.7:
		var center := Vector3.ZERO
		var alignment := Vector3.ZERO
		var separation := Vector3.ZERO
		var count := 0
		for neighbor in neighbors:
			if neighbor == self or neighbor.species.id != species.id: continue
			var gap := position.distance_to(neighbor.position)
			if gap > 4.0: continue
			center += neighbor.position
			alignment += neighbor.velocity
			count += 1
			if gap < .9: separation += (position-neighbor.position).normalized()
		if count > 0:
			move = (move + (center/count-position)*.13 + alignment.normalized()*.3 + separation*.6).normalized()
	velocity = velocity.lerp(move * species.speed, 1.0 - exp(-delta * 2.2))
	position += velocity * delta
	position.y = clampf(position.y, -5.9, 8.7)
	if position.x > 27: heading = -1
	if position.x < -27: heading = 1
	model.rotation.y = lerp_angle(model.rotation.y, 0.0 if velocity.x >= 0 else PI, delta * 5)
	model.rotation.z = clampf(velocity.y * .13, -.25, .25)
