"""Read-only broker diagnosis of initial editor state; no save, close or discard."""
from pathlib import Path
import json,subprocess,hashlib,datetime
A=Path(r"E:\Projects\worldwright-archipelago")
D=A/"evidence/integration/20261029-godot-broker/godot-bridge01"
r=json.loads((D/"prepared.json").read_text());out=D/"startup-inspection01";out.mkdir(exist_ok=False)
exe=Path(r["paths"]["bin"])/"semwright.exe"
assert hashlib.sha256(exe.read_bytes()).hexdigest()==next(x["sha256"] for x in r["candidate_files"] if x["name"]=="semwright.exe")
def op(label,command,args):
    with (out/(label+".args.json")).open("x",encoding="utf-8") as f:json.dump(args,f,indent=2)
    cmd=[str(exe),"--socket",r["socket"],"--session-file",str(Path(r["paths"]["config"])/"client-session.json"),
         "--json","execute","driver.godot."+command,"--args-file",str(out/(label+".args.json"))]
    p=subprocess.run(cmd,capture_output=True,timeout=25)
    with (out/(label+".envelope.json")).open("xb") as f:f.write(p.stdout)
    with (out/(label+".stderr.txt")).open("xb") as f:f.write(p.stderr)
    assert p.returncode==0
    e=json.loads(p.stdout);assert e["ok"] and e["execution"]["backend"]=="driver:godot"
    print(json.dumps({"label":label,"request_id":e["request_id"],"data":e["data"]},ensure_ascii=True),flush=True)
    return e["data"]
sessions=op("sessions","session.list",{})
assert len(sessions["sessions"])==1 and sessions["sessions"][0]["project"]==r["project_id"]
sid=sessions["sessions"][0]["session"]
op("project","project.inspect",{"session":sid})
op("editor","editor.state",{"session":sid})
op("scene","scene.inspect",{"session":sid})
op("root","node.inspect",{"session":sid,"path":"."})
