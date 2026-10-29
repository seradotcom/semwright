"""Verify the exact editor startup normalization before accepting a new byte baseline."""
from pathlib import Path
import hashlib,json,subprocess,datetime,re
A=Path(r"E:\Projects\worldwright-archipelago")
D=A/"evidence/integration/20261029-godot-broker/godot-bridge01"
r=json.loads((D/"prepared.json").read_text())
H=lambda b:hashlib.sha256(b).hexdigest()
original=(D/"project.godot.before.source").read_bytes()
expected=original.rstrip()+b'\n\n[editor_plugins]\nenabled=PackedStringArray("res://addons/semwright/plugin.cfg")\n'
assert H(expected)==r["project_after_sha256"]
current=(A/"game/project.godot").read_bytes()
assert H(current)=="ef5c468170a348a64b6744d67a3e9824a068f3415f30e5dccf43e78d6161d042"
def config(raw):
    section="";out={}
    for line in raw.decode().splitlines():
        line=line.strip()
        if not line or line.startswith(";"):continue
        if line.startswith("["):section=line[1:-1];continue
        k,v=line.split("=",1)
        # Godot changed only whitespace outside quoted strings in the reviewed Color literal.
        if k=="environment/defaults/default_clear_color":v=v.replace(" ","")
        out[section+"/"+k]=v
    return out
before=config(expected);after=config(current)
removed={"rendering/renderer/rendering_method":'"forward_plus"',
         "rendering/textures/default_filters/use_nearest_mipmap_filter":"false"}
assert {k:v for k,v in before.items() if k not in after}==removed
assert {k:v for k,v in before.items() if k not in removed}==after
probe=D/"startup_settings_probe.gd"
code='extends SceneTree\nfunc _initialize() -> void:\n\tprint("STARTUP_SETTINGS "+JSON.stringify({"renderer":ProjectSettings.get_setting("rendering/renderer/rendering_method"),"nearest_mipmap":ProjectSettings.get_setting("rendering/textures/default_filters/use_nearest_mipmap_filter"),"vsync":ProjectSettings.get_setting("display/window/vsync/vsync_mode"),"main_scene":ProjectSettings.get_setting("application/run/main_scene")}))\n\tquit()\n'
with probe.open("x",encoding="utf-8") as f:f.write(code)
exe=Path(r["godot"]["path"]);assert H(exe.read_bytes())==r["godot"]["sha256"]
cmd=[str(exe),"--headless","--path",str(A/"game"),"--script",str(probe)]
run=subprocess.run(cmd,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=30)
with (D/"startup-settings.log").open("xb") as f:f.write(run.stdout)
assert run.returncode==0
rows=[x[len("STARTUP_SETTINGS "):] for x in run.stdout.decode(errors="replace").splitlines() if x.startswith("STARTUP_SETTINGS ")]
assert len(rows)==1
observed=json.loads(rows[0])
assert observed=={"renderer":"forward_plus","nearest_mipmap":False,"vsync":0,"main_scene":"res://main.tscn"}
assert (A/"game/project.godot").read_bytes()==current
for name,raw in [("project.godot.enabled-before-start.source",expected),("project.godot.normalized.source",current)]:
    with (D/name).open("xb") as f:f.write(raw)
result={"schema":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"expected_enabled_sha256":H(expected),
"normalized_project_sha256":H(current),"only_reviewed_formatting_and_default_elision":True,"removed_explicit_values":removed,
"native_effective_settings":observed,"diagnostic_broker_used":False,"diagnostic_exit_code":run.returncode,
"preflight01_failed_before_client_launch":True,"prepared_receipt_preserved":True,"project_writes_by_reconciliation":False}
with (D/"startup-config-reconciliation.json").open("x",encoding="utf-8") as f:json.dump(result,f,indent=2);f.write("\n")
print(json.dumps(result,indent=2))
