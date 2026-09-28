extends RefCounted
class_name TidelingRules

const THRESHOLDS := [0, 35, 120]
const DURATION := 180.0
const DASH_COOLDOWN := 2.5
const DASH_DURATION := .24
var stage := 1
var nutrition := 0
var score := 0
var chain := 0
var combo_time := 0.0
var dash_cooldown := 0.0
var dash_time := 0.0
var elapsed := 0.0

func can_eat(species: TidelingSpecies) -> bool:
	return species.edible_stage <= stage

func eat(species: TidelingSpecies) -> bool:
	if not can_eat(species): return false
	chain = chain + 1 if combo_time > 0 else 1
	combo_time = 3.0
	nutrition += species.nutrition
	score += species.nutrition * multiplier() * 10
	if nutrition >= THRESHOLDS[2]: stage = 3
	elif nutrition >= THRESHOLDS[1]: stage = 2
	return true

func multiplier() -> int:
	if chain >= 6: return 5
	if chain >= 4: return 3
	if chain >= 2: return 2
	return 1

func dash() -> bool:
	if dash_cooldown > 0: return false
	dash_cooldown = DASH_COOLDOWN
	dash_time = DASH_DURATION
	return true

func tick(delta: float) -> void:
	elapsed += delta
	combo_time = maxf(0, combo_time - delta)
	if combo_time == 0: chain = 0
	dash_cooldown = maxf(0, dash_cooldown - delta)
	dash_time = maxf(0, dash_time - delta)

func growth_progress() -> float:
	if stage == 3: return 1.0
	return float(nutrition - THRESHOLDS[stage-1]) / float(THRESHOLDS[stage] - THRESHOLDS[stage-1])
