"""Prepare the first-party Windows Godot bridge, private secrets, and exact companion files."""
from pathlib import Path
import argparse,datetime,hashlib,json,secrets,socket,subprocess
P=argparse.ArgumentParser();P.add_argument("--candidate",default="godot-bridge01");args=P.parse_args()
candidate=args.candidate
assert candidate.startswith("godot-bridge") and candidate[12:].isdigit()
A=Path(r"E:\Projects\worldwright-archipelago");S=Path(r"E:\Projects\semwright-blender52")
D=A/"evidence/integration/20261029-godot-broker"/candidate
areas={"bin":A/"tools"/("semwright-"+candidate),"config":A/".local"/(candidate+"-config"),
       "broker":A/".local"/(candidate+"-state"),"editor_env":A/".local"/(candidate+"-editor-env")}
for p in [D,*areas.values()]:
    if p.exists():raise RuntimeError("Preserve previous attempt: "+str(p))
game=A/"game";project_file=game/"project.godot"
addon=game/"addons/semwright"
if addon.exists():raise RuntimeError("Existing companion requires explicit review")
baseline=project_file.read_bytes()
assert b"[editor_plugins]" not in baseline
H=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
godot=A/"tools/godot-4.7.2-win64/Godot_v4.7.2-stable_win64_console.exe"
assert H(godot)=="c8f0a6bc45a19b33541501e57f6f7cd972ab18453743266339d495cbbe846643"
def ps(code):
    p=subprocess.run(["powershell.exe","-NoProfile","-Command",code],capture_output=True,text=True,check=True)
    return p.stdout.strip()
existing=ps("Get-Process -Name '*godot*' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id")
if existing:raise RuntimeError("Godot already running; inspect owned/editor state before preparing")
sid=ps("[Security.Principal.WindowsIdentity]::GetCurrent().User.Value")
assert sid.startswith("S-1-5-21-")
for p in areas.values():
    p.mkdir(parents=True,exist_ok=False)
    if p.is_symlink() or getattr(p.lstat(),"st_file_attributes",0)&0x400:raise RuntimeError("Reparse point rejected")
    subprocess.run(["icacls.exe",str(p),"/inheritance:r","/grant:r","*"+sid+":(OI)(CI)(F)",
                    "*S-1-5-18:(OI)(CI)(F)","*S-1-5-32-544:(OI)(CI)(F)"],capture_output=True,check=True)
D.mkdir(parents=True,exist_ok=False)
def write_new(path,raw):
    path.parent.mkdir(parents=True,exist_ok=True)
    with path.open("xb") as f:f.write(raw)
def json_new(path,value):write_new(path,(json.dumps(value,indent=2)+"\n").encode())
compiled=S/".local/target/x86_64-pc-windows-gnu/debug"
expected={"semwright.exe":"b6c00c4109146f69e1be5c100df572dc561975e20234f1d0652dd7ee57e1c0dc",
"semwrightd.exe":"0c0e8a17945be2ed682eb44c17284aaa2686562ce8efae8d29782ea2926bd286",
"semwright-godot-driver.exe":"e3bd5d7a0ff1a83c7125881a113aba1a351024fa27f9ec4ddacca2bbe956dd33"}
files=[]
for name,digest in expected.items():
    src=compiled/name
    if H(src)!=digest:raise RuntimeError("Unexpected built binary: "+name)
    dst=areas["bin"]/name;write_new(dst,src.read_bytes());assert H(dst)==digest
    files.append({"name":name,"source":str(src),"path":str(dst),"sha256":digest,"bytes":dst.stat().st_size})
project_id=secrets.token_hex(32)
secret_path=areas["config"]/"pairing.secret"
write_new(secret_path,(secrets.token_hex(32)+"\n").encode())
driver_config=areas["config"]/"godot-config";driver_config.mkdir()
with socket.socket() as listener:
    listener.bind(("127.0.0.1",0));port=listener.getsockname()[1]
conf={"port":port,"development_mode":False,"projects":[{"project":project_id,"mount":"godot-project","secret_name":"godot-pairing"}],"runner":None,"authoring":None}
json_new(driver_config/"config.json",conf)
manifest=json.loads((S/"crates/driver-godot/driver.manifest.example.json").read_text())
manifest.update(protocol=8,executable=str(areas["bin"]/"semwright-godot-driver.exe"),
sha256=expected["semwright-godot-driver.exe"],network=False,nvidia_gpu=False,loopback_port=port,
mounts=[{"root":"godot-config","read_only":True,"execute":False},{"root":"godot-project","read_only":True,"execute":False}],
system_config=[],tools=[])
manifest["resources"]["processes"]=8
manifest["interfaces"]["host_tools"]=False
manifest["application"]["process_names"]=[godot.name]
manifest_path=areas["config"]/"godot.manifest.json";json_new(manifest_path,manifest)
config_lines=[f"drivers = ['{manifest_path}']","driver_network = false","driver_nvidia_gpu = false",
"driver_windows_blender52_initial = false","audit_max_bytes = 67108864","audit_retention = 4",
"","[policy]","profile = 'observe'","allow = ['driver:godot']","confirm_mutations = false"]
for name,path in [("godot-config",driver_config),("godot-project",game),("godot-pairing",secret_path)]:
    config_lines += ["","[[policy.filesystem]]",f"name = '{name}'",f"path = '{path}'","read = true","write = false"]
config_path=areas["config"]/"candidate.toml"
write_new(config_path,("\n".join(config_lines)+"\n").encode())
registry=areas["config"]/"mcp-upstreams.toml";write_new(registry,b"version = 1\nupstreams = []\n")
write_new(D/"project.godot.before.source",baseline)
protected={}
for rel in ["main.tscn","assets/native_bundle.json","scripts/harbor_review.gd","scripts/review_walker.gd","config/review_faro_roundtrip.json"]:
    protected[rel]=H(game/rel)
companions=S/"crates/driver-godot/companions.list"
copied=[]
for line in companions.read_text().splitlines():
    if not line or line.startswith("#"):continue
    rel,src_rel=line.split("=",1)
    assert rel.startswith("addons/semwright/") and ".." not in Path(rel).parts
    src=(companions.parent/src_rel).resolve()
    assert src.is_relative_to(S/"integrations/godot/addons/semwright")
    dst=game/rel;write_new(dst,src.read_bytes())
    copied.append({"source":str(src),"destination":rel,"sha256":H(dst),"bytes":dst.stat().st_size})
new_project=baseline.rstrip()+b'\n\n[editor_plugins]\nenabled=PackedStringArray("res://addons/semwright/plugin.cfg")\n'
assert project_file.read_bytes()==baseline
project_file.write_bytes(new_project)
assert all(H(game/rel)==digest for rel,digest in protected.items())
pipe=r"\\.\pipe\semwright-"+sid.replace("-","_")+"-archipelago-"+candidate
report={"schema":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"candidate":candidate,
"source_root":str(S),"source_head":subprocess.check_output(["git","-C",str(S),"rev-parse","HEAD"],text=True).strip(),
"paths":{k:str(v) for k,v in areas.items()},"project_root":str(game),"project_id":project_id,"port":port,
"config":str(config_path),"manifest":str(manifest_path),"mcp_registry":str(registry),"socket":pipe,
"secret_file":str(secret_path),"secret_recorded":False,"candidate_files":files,"companions":copied,
"project_before_sha256":hashlib.sha256(baseline).hexdigest(),"project_after_sha256":H(project_file),
"protected_project_files":protected,"godot":{"path":str(godot),"sha256":H(godot)},
"driver_project_mount":"read_only_no_execute","driver_network":False,"driver_capabilities":[],
"editor":"Owner process for this trusted project; not isolated; not started yet",
"status":"PREPARED_NOT_EXECUTED","human_edit_preservation":"NOT_DEMONSTRATED","G6":"NOT_COMPLETE"}
json_new(D/"prepared.json",report)
json_new(D/"driver-config.redacted.json",conf)
json_new(D/"driver.manifest.json",manifest)
print(json.dumps({"candidate":candidate,"receipt":str(D/"prepared.json"),"port":port,"companions":len(copied),
"driver_sha256":expected["semwright-godot-driver.exe"],"status":report["status"]},indent=2))
