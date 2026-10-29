"""Restart the same pinned native-console broker after exact expired-secret-lease removal."""
from pathlib import Path
import json,hashlib,subprocess,datetime,time,os
A=Path(r"E:\Projects\worldwright-archipelago")
D=A/"evidence/integration/20261029-godot-broker/godot-bridge01"
r=json.loads((D/"prepared.json").read_text());old=json.loads((D/"broker-launch.json").read_text())
editor=json.loads((D/"editor-launch.json").read_text())
previous=D/"operator-console02";out=D/"operator-console03";out.mkdir(exist_ok=False)
H=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def save(name,value):
    with (out/name).open("x",encoding="utf-8") as f:json.dump(value,f,indent=2);f.write("\n")
def psjson(code):
    p=subprocess.run(["powershell.exe","-NoProfile","-Command",code],capture_output=True,text=True,timeout=20)
    assert p.returncode==0,p.stderr
    return json.loads(p.stdout) if p.stdout.strip() else []
def process(pid):
    return psjson(f"$p=Get-Process -Id {int(pid)} -ErrorAction Stop; @{{pid=$p.Id;path=$p.Path;created=$p.StartTime.ToUniversalTime().ToString('o');session=$p.SessionId;window=$p.MainWindowHandle.ToInt64()}} | ConvertTo-Json -Compress")
lease=json.loads((previous/"secret-acl-recovery.json").read_text())
assert lease["only_exact_lease_rules_removed"] and lease["secret_bytes_unchanged"] and not lease["new_permissions_added"]
retired=json.loads((previous/"old-broker-retirement.json").read_text());assert retired["both_absent"]
assert not psjson("@(Get-Process -Id 37772,36556,43672 -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id) | ConvertTo-Json -Compress")
edproc=process(editor["pid"]);assert edproc["created"]==editor["process_creation_utc"]
for row in r["candidate_files"]:assert H(Path(row["path"]))==row["sha256"]
pins=json.loads((previous/"launch.json").read_text())["configuration_sha256"]
assert all(H(Path(r[key]))==value for key,value in pins.items())
source_rows=json.loads((D/"scene-save-reconciliation01/source-files.json").read_text())
for row in source_rows:assert H(A/"game"/row["path"])==row["sha256"]
before=json.loads((previous/"before-scene.envelope.json").read_text())["data"]
camera=json.loads((previous/"before-camera.envelope.json").read_text())["data"]
state=json.loads((previous/"before-editor.envelope.json").read_text())["data"]
cli=Path(r["paths"]["bin"])/"semwright.exe"
newsession=Path(r["paths"]["config"])/"client-session-operator03.json";assert not newsession.exists()
def op(label,command,args,sessionfile):
    cmd=[str(cli),"--socket",r["socket"],"--session-file",str(sessionfile),"--json","execute","driver.godot."+command,
         "--args-json",json.dumps(args,separators=(",",":"))]
    p=subprocess.run(cmd,capture_output=True,timeout=25)
    with (out/(label+".envelope.json")).open("xb") as f:f.write(p.stdout)
    assert p.returncode==0,(label,p.stderr.decode(errors="replace"))
    value=json.loads(p.stdout);assert value["ok"] and value["execution"]["backend"]=="driver:godot"
    return value["data"]
save("repair-input.json",{"exact_secret_lease_receipt_sha256":H(previous/"secret-acl-recovery.json"),
     "secret_file_bytes_changed":False,"policy_grants_changed":False,"all_source_files_unchanged_before_restart":len(source_rows)})
argv=[old["argv"][0],"--config",r["config"],"--socket",r["socket"],"--data-dir",r["paths"]["broker"],
      "--mcp-upstreams",r["mcp_registry"],"--log-format","json","--approval-console"]
si=subprocess.STARTUPINFO();si.dwFlags|=subprocess.STARTF_USESHOWWINDOW;si.wShowWindow=1
# Native ConsoleTerminal explicitly opens CONIN$/CONOUT$, independent of redirected log stdio.
with (out/"broker.stdout.log").open("xb") as stdout,(out/"broker.stderr.log").open("xb") as stderr:
    child=subprocess.Popen(argv,cwd=A,stdin=subprocess.DEVNULL,stdout=stdout,stderr=stderr,
                           creationflags=subprocess.CREATE_NEW_CONSOLE,startupinfo=si)
save("launch.json",{"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"pid":child.pid,"argv":argv,
    "native_approval_console":True,"approval_input_automated":False,"configuration_sha256":pins})
for n in range(150):
    assert child.poll() is None,"New broker exited; inspect preserved stderr"
    text=(out/"broker.stderr.log").read_text(errors="replace")
    if '"message":"broker ready; authorization defaults deny side effects"' in text:break
    time.sleep(.2)
else:raise RuntimeError("Console broker readiness not observed")
newproc=process(child.pid);assert newproc["session"]==edproc["session"]
for n in range(20):
    paired=op(f"after-sessions-{n:02}","session.list",{},newsession)
    if len(paired["sessions"])==1 and paired["sessions"][0]["project"]==r["project_id"]:break
    time.sleep(.5)
else:raise RuntimeError("Existing Editor did not reconnect; no plugin reload attempted")
newsid=paired["sessions"][0]["session"]
after=op("after-scene","scene.inspect",{"session":newsid},newsession)
aftercam=op("after-camera","node.inspect",{"session":newsid,"path":"ProofCamera"},newsession)
afterstate=op("after-editor","editor.state",{"session":newsid},newsession)
projection=lambda value:{k:v for k,v in value.items() if k!="ref"}
assert before["data"]["nodes"]==after["data"]["nodes"] and before["data"]["scene"]==after["data"]["scene"]
assert before["stamp"]["fingerprint"]==after["stamp"]["fingerprint"]
assert projection(camera["data"])==projection(aftercam["data"])
for key in ("scene","unsaved_scenes","playing"):assert state["data"][key]==afterstate["data"][key]
for row in source_rows:assert H(A/"game"/row["path"])==row["sha256"]
assert all(H(Path(r[key]))==value for key,value in pins.items())
assert process(editor["pid"])["created"]==edproc["created"]
summary={"schema":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"ready_for_human":True,
    "old_broker_and_driver_retired":True,"new_broker":newproc,"editor":edproc,"sessions":paired,
    "active_fixture":after["data"]["scene"],"fresh_stamp":after["stamp"],"source_files_unchanged":len(source_rows),
    "scene_projection_preserved":True,"fixture_camera_preserved":True,"unsaved_scenes":afterstate["data"]["unsaved_scenes"],
    "configuration_unchanged":True,"approval_console_native":True,"approval_prompts_submitted":0,
    "approval_input_automated":False,"human_camera_claimed":False,"G6":"NOT_COMPLETE","art_status":"REVISE"}
save("receipt.json",summary)
print(json.dumps(summary,indent=2))
