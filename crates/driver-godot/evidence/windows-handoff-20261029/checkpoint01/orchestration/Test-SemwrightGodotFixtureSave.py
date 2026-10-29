"""Save only the acknowledged supporting fixture; preserve recoverable source bytes and live main projection."""
from pathlib import Path
import hashlib,json,os,re,subprocess,time,datetime,shutil
A=Path(r"E:\Projects\worldwright-archipelago");S=Path(r"E:\Projects\semwright-blender52")
D=A/"evidence/integration/20261029-godot-broker/godot-bridge01"
r=json.loads((D/"prepared.json").read_text());H=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
name="fixture-save-attempt01"
for p in [D/name,D/(name+".log"),D/(name+".runner.json"),D/(name+".json")]:assert not p.exists()
recon=D/"scene-save-reconciliation01"
rec=json.loads((recon/"receipt.json").read_text())
assert rec["same_native_stamp_as_before_error"] and rec["fixture_still_unsaved"] and not rec["previous_failure_promoted_to_success"]
denied=json.loads((D/"handoff-attempt02/14-asset0-reimport.envelope.json").read_text())
assert denied["error"]["code"]=="ConsentRequired" and denied["error"]["outcome_known"]
build=S/".local/logs/native-profile-20261028-publication-contract-attempt01"
report=json.loads((build/"run.json").read_text());assert report["exit_code"]==0 and report["sources_unchanged"]
source="crates/driver-blender/tests/windows_broker_godot_handoff.rs"
assert H(S/source)==report["source_after"][source]
paths=re.findall(r"(?:Executable|Running).*?\(([^)\r\n]+windows_broker_godot_handoff-[a-f0-9]+\.exe)\)",(build/"cargo.log").read_text())
assert len(paths)==1,paths
binary=Path(paths[0])
if not binary.is_absolute():binary=S/binary
assert binary.is_file()
project=A/"game"
def inventory():
    rows={};total=0
    for p in sorted(project.rglob("*")):
        if not p.is_file() or ".godot" in p.relative_to(project).parts:continue
        assert not p.is_symlink() and not getattr(p.lstat(),"st_file_attributes",0)&0x400
        data=p.read_bytes();total+=len(data);assert total<2*1024**3 and len(rows)<10000
        rows[p.relative_to(project).as_posix()]={"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()}
    return rows
before=inventory()
prior={row["path"]:{k:row[k] for k in ("bytes","sha256")} for row in json.loads((recon/"source-files.json").read_text())}
assert before==prior,"Source changed since reconciliation"
backup=A/".local/godot-bridge01-fixture-save-backup01";backup.mkdir(exist_ok=False)
for rel,meta in before.items():
    src=project/rel;dest=backup/rel;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(src,dest)
    assert H(dest)==meta["sha256"]
assert inventory()==before
with (D/(name+".source-before.json")).open("x",encoding="utf-8") as f:json.dump(before,f,indent=2);f.write("\n")
cfg=json.loads((D/"handoff-attempt02.json").read_text())
scene=json.loads((recon/"scene.envelope.json").read_text())["data"]
camera=json.loads((recon/"camera.envelope.json").read_text())["data"]
sid=json.loads((D/"existing-readback-attempt01/00-sessions.envelope.json").read_text())["data"]["sessions"][0]["session"]
fixture=cfg["supporting_scene"][6:]
cfg.update(editor_session=sid,reconciled_stamp=scene["stamp"],reconciled_camera_data=camera["data"],
           fixture_before_sha256=before[fixture]["sha256"],new_save_mode="save_external_resources=true",
           scope="Owned root/camera and references to two existing imported GLBs; no resource editing capability was invoked")
config=D/(name+".json")
with config.open("x",encoding="utf-8") as f:json.dump(cfg,f,indent=2);f.write("\n")
env=os.environ.copy();env.update(SEMWRIGHT_TEST_BROKER_PIPE=r["socket"],SEMWRIGHT_TEST_RECEIPTS=str(D/name),
    SEMWRIGHT_GODOT_PROOF_CONFIG=str(config),SEMWRIGHT_GODOT_PROOF_CONFIG_SHA256=H(config))
start=time.monotonic()
with (D/(name+".log")).open("xb") as log:
    p=subprocess.run([str(binary),"--ignored","--exact","windows_broker_godot_finalize_known_fixture","--nocapture"],
                     cwd=S,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=150)
after=inventory()
delta={k:{"before":before.get(k),"after":after.get(k)} for k in sorted(before.keys()|after.keys()) if before.get(k)!=after.get(k)}
allowed=set(delta)=={fixture} and after.keys()==before.keys()
result={"schema":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"exit_code":p.returncode,
    "seconds":time.monotonic()-start,"binary_sha256":H(binary),"source_sha256":H(S/source),
    "build_report_sha256":H(build/"run.json"),"config_sha256":H(config),"source_files_before":len(before),
    "source_files_after":len(after),"source_delta":delta,"only_owned_fixture_source_changed":allowed,
    "recovery_copy":str(backup),"recovery_copy_verified":True,
    "godot_state_directory_excluded":".godot including editor state; no claim of unchanged editor cache",
    "reimport":"BLOCKED_CONSENT_REQUIRED","blocked_request_id":denied["request_id"],
    "editor_isolated":False,"human_camera_claimed":False,"G6":"NOT_COMPLETE","log_sha256":H(D/(name+".log"))}
with (D/(name+".runner.json")).open("x",encoding="utf-8") as f:json.dump(result,f,indent=2);f.write("\n")
print(json.dumps(result,indent=2))
if p.returncode:print((D/(name+".log")).read_text(errors="replace")[-10000:])
raise SystemExit(p.returncode or (0 if allowed else 3))
