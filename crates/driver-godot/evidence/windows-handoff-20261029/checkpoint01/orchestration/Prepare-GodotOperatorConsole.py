"""Move the owned broker to its native human console; never read or synthesize approval input."""
from pathlib import Path
import json,hashlib,subprocess,datetime,time,os,shutil
A=Path(r"E:\Projects\worldwright-archipelago")
D=A/"evidence/integration/20261029-godot-broker/godot-bridge01"
r=json.loads((D/"prepared.json").read_text());old=json.loads((D/"broker-launch.json").read_text())
editor=json.loads((D/"editor-launch.json").read_text())
out=D/"operator-console02";out.mkdir(exist_ok=False)
H=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def save(name,value):
    with (out/name).open("x",encoding="utf-8") as f:json.dump(value,f,indent=2);f.write("\n")
for row in r["candidate_files"]:assert H(Path(row["path"]))==row["sha256"]
pins={key:H(Path(r[key])) for key in ("config","manifest","mcp_registry")}
for row in r["companions"]:assert H(A/"game"/row["destination"])==row["sha256"]
for req in D.rglob("*.request.json"):
    assert req.with_name(req.name.replace(".request.json",".envelope.json")).is_file(),"Unresolved prior request"
def psjson(code):
    p=subprocess.run(["powershell.exe","-NoProfile","-Command",code],capture_output=True,text=True,timeout=20)
    assert p.returncode==0,p.stderr
    return json.loads(p.stdout) if p.stdout.strip() else []
def process(pid):
    return psjson(f"$p=Get-Process -Id {int(pid)} -ErrorAction Stop; @{{pid=$p.Id;path=$p.Path;created=$p.StartTime.ToUniversalTime().ToString('o');session=$p.SessionId;window=$p.MainWindowHandle.ToInt64()}} | ConvertTo-Json -Compress")
oldproc=process(old["pid"]);edproc=process(editor["pid"])
assert oldproc["path"].lower()==old["argv"][0].lower() and oldproc["created"]==old["process_creation_utc"]
assert edproc["path"].lower()==editor["argv"][0].lower() and edproc["created"]==editor["process_creation_utc"]
host=[json.loads(x) for x in (D/"broker.stderr.log").read_text().splitlines() if x.strip()]
attested=[x for x in host if x.get("phase")=="initial_process_attested_before_resume"]
assert len(attested)==1 and attested[0]["token"]["lpac"] and attested[0]["token"]["capability_count"]==0
driver=process(attested[0]["pid"])
driver_path=next(x["path"] for x in r["candidate_files"] if x["name"]=="semwright-godot-driver.exe")
staged=Path(driver["path"]).resolve()
assert staged.parent==Path(r["paths"]["broker"])/"state/drivers" and staged.suffix==".exe"
assert H(staged)==H(Path(driver_path))
parent=psjson(f"(Get-CimInstance Win32_Process -Filter 'ProcessId={driver['pid']}').ParentProcessId | ConvertTo-Json -Compress")
assert parent==oldproc["pid"],"Driver is not a child of the owned broker"
cli=Path(r["paths"]["bin"])/"semwright.exe"
oldsession=Path(r["paths"]["config"])/"client-session.json"
newsession=Path(r["paths"]["config"])/"client-session-operator01.json";assert not newsession.exists()
def op(label,command,args,sessionfile):
    cmd=[str(cli),"--socket",r["socket"],"--session-file",str(sessionfile),"--json","execute","driver.godot."+command,
         "--args-json",json.dumps(args,separators=(",",":"))]
    p=subprocess.run(cmd,capture_output=True,timeout=25)
    with (out/(label+".envelope.json")).open("xb") as f:f.write(p.stdout)
    assert p.returncode==0,(label,p.stderr.decode(errors="replace"))
    value=json.loads(p.stdout);assert value["ok"] and value["execution"]["backend"]=="driver:godot"
    return value["data"]
sessions=op("before-sessions","session.list",{},oldsession)
assert len(sessions["sessions"])==1 and sessions["sessions"][0]["project"]==r["project_id"]
sid=sessions["sessions"][0]["session"]
before=op("before-scene","scene.inspect",{"session":sid},oldsession)
camera=op("before-camera","node.inspect",{"session":sid,"path":"ProofCamera"},oldsession)
state=op("before-editor","editor.state",{"session":sid},oldsession)
idle=op("before-assets","assets.status",{"session":sid},oldsession)
assert idle["data"]["scanning"] is False and idle["data"]["importing"] is False
cfg=json.loads((D/"handoff-attempt02.json").read_text())
assert before["data"]["scene"]==cfg["supporting_scene"]
recon=json.loads((D/"scene-save-reconciliation01/scene.envelope.json").read_text())["data"]
assert before["stamp"]==recon["stamp"] and before["data"]["nodes"]==recon["data"]["nodes"]
source_rows=json.loads((D/"scene-save-reconciliation01/source-files.json").read_text())
for row in source_rows:assert H(A/"game"/row["path"])==row["sha256"]
save("before-processes.json",{"broker":oldproc,"driver":driver,"editor":edproc})
# Terminate this exact idle broker only. Its Job owns the confined driver; the Editor is not its child.
kill=f"$p=Get-Process -Id {oldproc['pid']} -ErrorAction Stop; if ($p.Path -ne '{oldproc['path']}' -or $p.StartTime.ToUniversalTime().ToString('o') -ne '{oldproc['created']}') {{throw 'Owned broker changed'}}; Stop-Process -InputObject $p -ErrorAction Stop"
subprocess.run(["powershell.exe","-NoProfile","-Command",kill],capture_output=True,check=True,timeout=15)
for n in range(30):
    live=psjson(f"@(Get-Process -Id {oldproc['pid']},{driver['pid']} -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id) | ConvertTo-Json -Compress")
    if not live:break
    time.sleep(.1)
assert not live,"Old owned broker/driver did not exit"
assert process(editor["pid"])["created"]==edproc["created"]
audit=Path(r["paths"]["broker"])/"state/audit/audit.jsonl"
shutil.copyfile(audit,out/"pre-console-audit.jsonl")
save("old-broker-retirement.json",{"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),
     "old_broker":oldproc,"old_driver":driver,"both_absent":True,"editor_preserved":True,
     "audit_sha256":H(out/"pre-console-audit.jsonl"),"new_mutations_submitted":False})
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
