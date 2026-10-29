"""Read-only reconciliation of an unsupported strict save; never replay the unknown mutation."""
from pathlib import Path
import hashlib,json,subprocess,datetime
A=Path(r"E:\Projects\worldwright-archipelago");D=A/"evidence/integration/20261029-godot-broker/godot-bridge01"
r=json.loads((D/"prepared.json").read_text());cfg=json.loads((D/"handoff-attempt02.json").read_text())
out=D/"scene-save-reconciliation01";out.mkdir(exist_ok=False)
prior=D/"existing-readback-attempt01"
failure=json.loads((prior/"15-save-supporting-only.envelope.json").read_text())
assert failure["error"]["code"]=="Unsupported" and failure["error"]["outcome_known"] is False
last=json.loads((prior/"14-camera-node-after.envelope.json").read_text())["data"]
sid=json.loads((prior/"00-sessions.envelope.json").read_text())["data"]["sessions"][0]["session"]
cli=Path(r["paths"]["bin"])/"semwright.exe"
def op(label,command,args):
    cmd=[str(cli),"--socket",r["socket"],"--session-file",str(Path(r["paths"]["config"])/"client-session.json"),
         "--json","execute","driver.godot."+command,"--args-json",json.dumps(args,separators=(",",":"))]
    p=subprocess.run(cmd,capture_output=True,timeout=25)
    with (out/(label+".envelope.json")).open("xb") as f:f.write(p.stdout)
    assert p.returncode==0
    e=json.loads(p.stdout);assert e["ok"] and e["execution"]["backend"]=="driver:godot"
    return e["data"]
scene=op("scene","scene.inspect",{"session":sid})
editor=op("editor","editor.state",{"session":sid})
camera=op("camera","node.inspect",{"session":sid,"path":"ProofCamera"})
assert scene["stamp"]==editor["stamp"]==camera["stamp"]==last["stamp"]
assert camera["data"]["observed"]==last["data"]["observed"] and camera["data"]["properties"]==last["data"]["properties"]
path=A/"game"/cfg["supporting_scene"][6:];raw=path.read_bytes()
assert b'ProofCamera' not in raw and b'FaroRock' not in raw and b'FaroThreshold' not in raw and len(raw)<512
assert cfg["supporting_scene"] in editor["data"]["unsaved_scenes"]
files=[];total=0
for p in sorted((A/"game").rglob("*")):
    if not p.is_file() or ".godot" in p.relative_to(A/"game").parts:continue
    if p.is_symlink() or getattr(p.lstat(),"st_file_attributes",0)&0x400:raise RuntimeError("Link in source inventory")
    data=p.read_bytes();total+=len(data);assert total<2*1024**3 and len(files)<10000
    files.append({"path":p.relative_to(A/"game").as_posix(),"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()})
with (out/"source-files.json").open("x",encoding="utf-8") as f:json.dump(files,f,indent=2);f.write("\n")
with (out/"supporting-scene-before.source").open("xb") as f:f.write(raw)
result={"schema":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"original_request_id":failure["request_id"],
"original_error":failure["error"],"same_native_stamp_as_before_error":True,"camera_unchanged":True,
"disk_contains_initial_root_only":True,"fixture_still_unsaved":True,"source_files":len(files),"source_bytes":total,
"reconciliation":"No persisted fixture edit observed; conservative nested-instance preflight rejects before save",
"previous_failure_promoted_to_success":False,"next_candidate_mode":"Existing supported scene.save with save_external_resources=true; new request after this reconciliation"}
with (out/"receipt.json").open("x",encoding="utf-8") as f:json.dump(result,f,indent=2);f.write("\n")
print(json.dumps(result,indent=2))
