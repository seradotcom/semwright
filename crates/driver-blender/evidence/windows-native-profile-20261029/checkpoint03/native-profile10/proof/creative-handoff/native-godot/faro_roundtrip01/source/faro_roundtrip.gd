extends SceneTree
func _initialize() -> void:
	call_deferred("_run")
func _run() -> void:
	if change_scene_to_file("res://main.tscn") != OK:
		quit(2)
		return
	await process_frame
	await process_frame
	if current_scene == null or not current_scene.get("asset_ready"):
		quit(2)
		return
	var walker: CharacterBody3D = current_scene.get("walker")
	walker.set("validation_route_path","res://config/review_faro_roundtrip.json")
	walker.call("run_validation")
