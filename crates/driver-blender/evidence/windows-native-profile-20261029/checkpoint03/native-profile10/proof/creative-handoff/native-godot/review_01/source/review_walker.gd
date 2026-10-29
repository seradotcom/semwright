extends CharacterBody3D
## Small G2 scale and traversal probe. This is not the full gameplay controller.
var review: Node3D
var camera: Camera3D
var active := false
var automated := false
var route: Array[Vector2] = []
var waypoint := 0
var samples: Array[Dictionary] = []
var run_seconds := 0.0
var sample_seconds := 0.0
var airborne_seconds := 0.0
var maximum_airborne := 0.0
var stalled_seconds := 0.0
var previous_distance := 100000.0
var validation_route_path := "res://config/review_walk_route.json"

func configure(owner_node: Node3D, view: Camera3D) -> void:
	review = owner_node
	camera = view
	name = "Paseo"
	collision_layer = 2
	collision_mask = 1
	floor_snap_length = 0.30
	floor_max_angle = deg_to_rad(40.0)
	safe_margin = 0.025
	var capsule := CapsuleShape3D.new()
	capsule.radius = 0.28
	capsule.height = 1.72
	var collider := CollisionShape3D.new()
	collider.shape = capsule
	collider.position.y = 0.86
	add_child(collider)

func _ground_at(point: Vector3) -> Dictionary:
	var query := PhysicsRayQueryParameters3D.create(Vector3(point.x,120.0,point.z),Vector3(point.x,-5.0,point.z),1)
	query.exclude = [get_rid()]
	return get_world_3d().direct_space_state.intersect_ray(query)

func enter(point: Vector3) -> bool:
	var hit := _ground_at(point)
	if hit.is_empty() or hit.normal.dot(Vector3.UP) < 0.6:
		return false
	position = hit.position + Vector3.UP * 0.06
	velocity = Vector3.ZERO
	active = true
	return true

func toggle() -> void:
	if active:
		leave()
	elif not enter(camera.position):
		enter(Vector3(18.0,10.0,6.0))

func leave() -> void:
	active = false
	automated = false
	velocity = Vector3.ZERO

func run_validation() -> void:
	if "--verify-bridge" in OS.get_cmdline_user_args():
		validation_route_path = "res://config/review_bridge_route.json"
	await get_tree().physics_frame
	await get_tree().physics_frame
	var parsed: Variant = JSON.parse_string(FileAccess.get_file_as_string(validation_route_path))
	if not parsed is Dictionary or not parsed.get("points") is Array:
		_finish(false,"route_unavailable")
		return
	for point: Array in parsed.points:
		route.append(Vector2(float(point[0]),float(point[1])))
	if route.size() < 2 or not enter(Vector3(route[0].x,0,route[0].y)):
		_finish(false,"no_initial_ground")
		return
	waypoint = 1
	automated = true
	run_seconds = 0.0
	samples.clear()
	camera.rotation = Vector3(-0.06,0,0)

func _physics_process(delta: float) -> void:
	if not active:
		return
	var desired := Vector3.ZERO
	if automated:
		run_seconds += delta
		var target := Vector3(route[waypoint].x,position.y,route[waypoint].y)
		var remaining := position.distance_to(target)
		if remaining < 0.34:
			waypoint += 1
			previous_distance = 100000.0
			stalled_seconds = 0.0
			if waypoint >= route.size():
				_finish(true,"route_completed")
				return
			target = Vector3(route[waypoint].x,position.y,route[waypoint].y)
			remaining = position.distance_to(target)
		desired = (target-position).normalized()*3.0
		if remaining > previous_distance-0.003:
			stalled_seconds += delta
		else:
			stalled_seconds = 0.0
		previous_distance = remaining
		if run_seconds > 70.0 or stalled_seconds > 3.0 or position.y < 0.4:
			_finish(false,"blocked_or_fell")
			return
		if desired.length_squared() > 0.01:
			camera.look_at(camera.position+desired.normalized()+Vector3(0,-0.06,0),Vector3.UP)
	else:
		var input := Vector2(float(Input.is_key_pressed(KEY_D))-float(Input.is_key_pressed(KEY_A)),float(Input.is_key_pressed(KEY_S))-float(Input.is_key_pressed(KEY_W)))
		var forward := Vector3(camera.basis.z.x,0,camera.basis.z.z).normalized()
		var right := Vector3(camera.basis.x.x,0,camera.basis.x.z).normalized()
		desired = (right*input.x+forward*input.y).normalized()*(5.0 if Input.is_key_pressed(KEY_SHIFT) else 3.1)
		if is_on_floor() and Input.is_key_pressed(KEY_SPACE):
			velocity.y = 4.8
	velocity.x = move_toward(velocity.x,desired.x,18.0*delta)
	velocity.z = move_toward(velocity.z,desired.z,18.0*delta)
	if not is_on_floor():
		velocity.y -= 15.0*delta
	elif velocity.y < 0:
		velocity.y = 0
	move_and_slide()
	camera.position = position+Vector3.UP*1.60
	if automated:
		airborne_seconds = 0.0 if is_on_floor() else airborne_seconds+delta
		maximum_airborne = maxf(maximum_airborne,airborne_seconds)
		if airborne_seconds > 0.45:
			_finish(false,"ground_contact_lost")
			return
		sample_seconds += delta
		if sample_seconds >= 0.10:
			sample_seconds = 0
			samples.append({"seconds":run_seconds,"position":[position.x,position.y,position.z],"velocity":[velocity.x,velocity.y,velocity.z],"on_floor":is_on_floor(),"waypoint":waypoint,"monotonic_usec":Time.get_ticks_usec()})

func _finish(passed: bool, reason: String) -> void:
	active = false
	automated = false
	var receipt: Dictionary = review.call("_source_receipt")
	receipt["native_physics"] = true
	receipt["test_kind"] = "automated_ground_route_using_same_CharacterBody_controller"
	receipt["human_playtest"] = false
	receipt["passed"] = passed
	receipt["reason"] = reason
	receipt["seconds"] = run_seconds
	receipt["reached_waypoints"] = waypoint
	receipt["maximum_airborne_seconds"] = maximum_airborne
	receipt["samples"] = samples
	receipt["route_sha256"] = FileAccess.get_sha256(validation_route_path)
	receipt["route_path"] = validation_route_path
	receipt["final_position"] = [position.x,position.y,position.z]
	var contacts: Array[Dictionary] = []
	for index in get_slide_collision_count():
		var hit := get_slide_collision(index)
		var other := hit.get_collider() as Node
		contacts.append({"collider_path":str(other.get_path()) if other != null else "", "normal":[hit.get_normal().x,hit.get_normal().y,hit.get_normal().z], "position":[hit.get_position().x,hit.get_position().y,hit.get_position().z]})
	receipt["final_contacts"] = contacts
	var filename := "native_bridge_readback.json" if "--verify-bridge" in OS.get_cmdline_user_args() else "native_walk_readback.json"
	var recorded: bool = review.call("_write_json",filename,receipt)
	print("NATIVE_WALK_RESULT "+JSON.stringify({"passed":passed,"reason":reason,"seconds":run_seconds,"waypoints":waypoint}))
	get_tree().quit(0 if passed and recorded else 5)
