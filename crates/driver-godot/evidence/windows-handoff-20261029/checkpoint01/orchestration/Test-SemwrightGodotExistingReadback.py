"""Continue only acknowledged fixture state; reimport remains blocked by trusted-operator consent."""
from pathlib import Path
import hashlib,json,os,re,subprocess,time,datetime
A=Path(r"E:\Projects\worldwright-archipelago");S=Path(r"E:\Projects\semwright-blender52")
D=A/"evidence/integration/20261029-godot-broker/godot-bridge01"
r=json.loads((D/"prepared.json").read_text());H=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
denied=json.loads((D/"handoff-attempt02/14-asset0-reimport.envelope.json").read_text())
assert denied["error"]["code"]=="ConsentRequired" and denied["error"]["outcome_known"]
assert denied["execution"]["policy_decision"]=="require_confirmation"
name="existing-readback-attempt01"
for p in [D/name,D/(name+".log"),D/(name+".runner.json")]:assert not p.exists()
build=S/".local/logs/native-profile-20261028-godot-handoff-proof-build-attempt04"
report=json.loads((build/"run.json").read_text());assert report["exit_code"]==0 and report["sources_unchanged"]
source="crates/driver-blender/tests/windows_broker_godot_handoff.rs"
assert H(S/source)==report["source_after"][source]
paths=re.findall(r"Executable.*?\(([^\r\n]+windows_broker_godot_handoff-[a-f0-9]+\.exe)\)",(build/"cargo.log").read_text())
assert len(paths)==1
binary=Path(paths[0])
if not binary.is_absolute():binary=S/binary
config=D/"handoff-attempt02.json"
cfg=json.loads(config.read_text())
for asset in cfg["assets"]:assert H(A/"game"/asset["path"][6:])==asset["sha256"]
protected={k:H(A/"game"/k) for k in r["protected_project_files"]};assert protected==r["protected_project_files"]
project=H(A/"game/project.godot")
assert project==json.loads((D/"startup-config-reconciliation.json").read_text())["normalized_project_sha256"]
env=os.environ.copy();env.update(SEMWRIGHT_TEST_BROKER_PIPE=r["socket"],SEMWRIGHT_TEST_RECEIPTS=str(D/name),
SEMWRIGHT_GODOT_PROOF_CONFIG=str(config),SEMWRIGHT_GODOT_PROOF_CONFIG_SHA256=H(config))
start=time.monotonic()
with (D/(name+".log")).open("xb") as log:
    p=subprocess.run([str(binary),"--ignored","--exact","windows_broker_godot_existing_import_readback","--nocapture"],
                     cwd=S,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=150)
after={k:H(A/"game"/k) for k in protected}
result={"schema":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"exit_code":p.returncode,
"seconds":time.monotonic()-start,"binary_sha256":H(binary),"source_sha256":H(S/source),"build_report_sha256":H(build/"run.json"),
"config_sha256":H(config),"protected_files_unchanged":protected==after,"project_config_unchanged":project==H(A/"game/project.godot"),
"reimport":"BLOCKED_CONSENT_REQUIRED","blocked_request_id":denied["request_id"],"editor_isolated":False,
"human_camera_claimed":False,"G6":"NOT_COMPLETE","log_sha256":H(D/(name+".log"))}
with (D/(name+".runner.json")).open("x",encoding="utf-8") as f:json.dump(result,f,indent=2);f.write("\n")
print(json.dumps(result,indent=2))
if p.returncode:print((D/(name+".log")).read_text(errors="replace")[-10000:])
raise SystemExit(p.returncode or (0 if protected==after and result["project_config_unchanged"] else 3))
