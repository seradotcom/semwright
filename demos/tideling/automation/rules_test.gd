extends SceneTree
const Rules = preload("res://scripts/rules.gd")
var failed := false
var checks := 0
func verify(value: bool, message: String) -> void:
	checks += 1
	if not value:
		failed = true
		push_error(message)
func _initialize() -> void:
	var r = Rules.new()
	var small = load("res://species/fry.tres")
	var medium = load("res://species/blue.tres")
	var large = load("res://species/puffer.tres")
	var hunter = load("res://species/grouper.tres")
	verify(not r.eat(medium) and r.nutrition == 0 and r.score == 0,"Inedible prey must not mutate score or growth")
	for i in range(34): r.eat(small)
	verify(r.stage == 1,"Stage boundary below 35")
	r.eat(small)
	verify(r.stage == 2 and r.can_eat(medium) and not r.can_eat(large),"Stage 2 boundary and food chain")
	for i in range(85): r.eat(small)
	verify(r.stage == 3 and r.can_eat(large) and not r.can_eat(hunter),"Stage 3 boundary and permanent predator")
	verify(r.growth_progress() == 1,"Mature growth progress")
	r = Rules.new()
	r.eat(small)
	verify(r.multiplier() == 1,"First prey combo")
	r.eat(small)
	verify(r.multiplier() == 2,"Second prey combo")
	r.eat(small); r.eat(small)
	verify(r.multiplier() == 3,"Fourth prey combo")
	r.eat(small); r.eat(small)
	verify(r.multiplier() == 5,"Sixth prey combo")
	r.tick(3.1)
	verify(r.multiplier() == 1 and r.chain == 0,"Combo expires")
	verify(r.dash() and not r.dash(),"No cooldown bypass")
	r.tick(.25)
	verify(r.dash_time == 0 and not r.dash(),"Dash ends before cooldown")
	r.tick(2.25)
	verify(r.dash(),"Cooldown recharges")
	print("TIDELING_RULES "+JSON.stringify({"checks":checks,"passed":not failed}))
	quit(1 if failed else 0)
