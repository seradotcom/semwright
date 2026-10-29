"""Launch only a prepared owned broker or trusted editor; secrets stay in private process environment."""
from pathlib import Path
import argparse,datetime,hashlib,json,os,subprocess
p=argparse.ArgumentParser();p.add_argument("component",choices=["broker","editor"]);p.add_argument("--candidate",default="godot-bridge01");a=p.parse_args()
A=Path(r"E:\Projects\worldwright-archipelago")
assert a.candidate.startswith("godot-bridge") and a.candidate[12:].isdigit()
D=A/"evidence/integration/20261029-godot-broker"/a.candidate
r=json.loads((D/"prepared.json").read_text())
H=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
for row in r["candidate_files"]:
    assert H(row["path"])==row["sha256"]
for row in r["companions"]:
    assert H(Path(r["project_root"])/row["destination"])==row["sha256"]
assert H(Path(r["project_root"])/"project.godot")==r["project_after_sha256"]
out=D/(a.component+"-launch.json")
logs=[D/(a.component+".stdout.log"),D/(a.component+".stderr.log")]
assert not out.exists() and all(not x.exists() for x in logs)
env=os.environ.copy()
for key in list(env):
    if key.startswith("SEMWRIGHT_GODOT_"):env.pop(key)
if a.component=="broker":
    cmd=[str(Path(r["paths"]["bin"])/"semwrightd.exe"),"--config",r["config"],"--socket",r["socket"],
         "--data-dir",r["paths"]["broker"],"--mcp-upstreams",r["mcp_registry"],"--log-format","json"]
else:
    assert (D/"broker-launch.json").is_file() and H(r["godot"]["path"])==r["godot"]["sha256"]
    secret=Path(r["secret_file"]).read_text().strip()
    assert len(secret)==64 and all(c in "0123456789abcdef" for c in secret)
    env.update(SEMWRIGHT_GODOT_PROJECT=r["project_id"],SEMWRIGHT_GODOT_PORT=str(r["port"]),SEMWRIGHT_GODOT_SECRET=secret)
    private=Path(r["paths"]["editor_env"])
    for key,leaf in [("APPDATA","roaming"),("LOCALAPPDATA","local"),("TMP","tmp"),("TEMP","tmp")]:
        q=private/leaf;q.mkdir(exist_ok=True);env[key]=str(q)
    cmd=[r["godot"]["path"],"--editor","--path",r["project_root"],"--resolution","1600x900","--windowed"]
started=datetime.datetime.now(datetime.timezone.utc).isoformat()
with logs[0].open("xb") as stdout,logs[1].open("xb") as stderr:
    proc=subprocess.Popen(cmd,cwd=r["project_root"],env=env,stdin=subprocess.DEVNULL,stdout=stdout,stderr=stderr)
ps=subprocess.run(["powershell.exe","-NoProfile","-Command",f"(Get-Process -Id {proc.pid} -ErrorAction Stop).StartTime.ToUniversalTime().ToString('o')"],capture_output=True,text=True)
receipt={"schema":1,"component":a.component,"pid":proc.pid,"utc_before":started,"process_creation_utc":ps.stdout.strip(),
         "argv":cmd,"stdout":str(logs[0]),"stderr":str(logs[1]),"environment_secret_logged":False,
         "status":"LAUNCHED_READINESS_NOT_YET_OBSERVED","editor_isolated":False}
with out.open("x",encoding="utf-8") as f:json.dump(receipt,f,indent=2);f.write("\n")
print(json.dumps(receipt,indent=2))
