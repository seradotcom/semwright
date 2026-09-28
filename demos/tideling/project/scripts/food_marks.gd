extends Control
var game: Node3D
func _process(_delta: float) -> void:
	queue_redraw()
func _draw() -> void:
	if not game or not game.active: return
	for fish in game.population:
		var distance: float = fish.position.distance_to(game.player.position)
		if distance > 5: continue
		var point: Vector2 = game.camera.unproject_position(fish.global_position)
		if game.rules.can_eat(fish.species):
			var radius := clampf(fish.species.size * 32, 10, 28)
			for side in [0.0, PI]:
				draw_arc(point, radius, side-.55, side+.55, 12, Color(.015,.07,.09,.85),4,true)
				draw_arc(point, radius, side-.55, side+.55, 12, Color("c9f2de"),1.5,true)
		elif fish.species.predator:
			var at := point + Vector2(0,-fish.species.size*30-8)
			draw_polyline(PackedVector2Array([at+Vector2(0,-5),at+Vector2(5,0),at+Vector2(0,5),at+Vector2(-5,0),at+Vector2(0,-5)]),Color("ffbf83"),2,true)
