extends Resource
class_name TidelingSpecies

@export var id := "yellow"
@export var display_name := "Sun minnow"
@export var asset_path := "res://assets/yellow.glb"
@export_range(1, 4) var edible_stage := 1
@export var size := 0.25
@export var speed := 1.4
@export var nutrition := 3
@export var predator := false
@export var spawn_weight := 1.0
@export var enabled := true
