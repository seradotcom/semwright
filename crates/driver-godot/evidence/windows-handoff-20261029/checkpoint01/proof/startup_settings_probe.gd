extends SceneTree
func _initialize() -> void:
	print("STARTUP_SETTINGS "+JSON.stringify({"renderer":ProjectSettings.get_setting("rendering/renderer/rendering_method"),"nearest_mipmap":ProjectSettings.get_setting("rendering/textures/default_filters/use_nearest_mipmap_filter"),"vsync":ProjectSettings.get_setting("display/window/vsync/vsync_mode"),"main_scene":ProjectSettings.get_setting("application/run/main_scene")}))
	quit()
