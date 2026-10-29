"""Native-human handoff: --preflight only observes; --submit requires the owner at the console."""
from pathlib import Path
import argparse,json,hashlib,subprocess,os,re,time,datetime
from godot_handoff_guard import run_preflight, inventory as guarded_inventory, classify_requests
if not __debug__:
    raise SystemExit("Optimized Python disables legacy assertions; use ordinary python.")
p=argparse.ArgumentParser();mode=p.add_mutually_exclusive_group(required=True);mode.add_argument("--prepare",action="store_true");mode.add_argument("--preflight",action="store_true");mode.add_argument("--submit",action="store_true");args=p.parse_args()
A=Path(r"E:\Projects\worldwright-archipelago");S=Path(r"E:\Projects\semwright-blender52")
D=A/"evidence/integration/20261029-godot-broker/godot-bridge01";C=D/"operator-console03"
name="operator-handoff-attempt01";config=D/(name+".json");preparation=D/(name+".prepared.json")
H=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
if not args.prepare:
    preflight=run_preflight(A,S,D,C,name)
    if args.preflight or not preflight["passed"]:
        print(json.dumps(preflight,indent=2))
        raise SystemExit(0 if preflight["passed"] else 4)
# --submit still requires personal responses in the native broker console.
r=json.loads((D/"prepared.json").read_text());console=json.loads((C/"receipt.json").read_text())
assert console["ready_for_human"] and console["approval_prompts_submitted"]==0
build=S/".local/logs/native-profile-20261028-godot-handoff-proof-build-attempt07"
report=json.loads((build/"run.json").read_text());assert report["exit_code"]==0 and report["sources_unchanged"]
source="crates/driver-blender/tests/windows_broker_godot_handoff.rs";assert H(S/source)==report["source_after"][source]
matches=re.findall(r"Executable.*?\(([^)\r\n]+windows_broker_godot_handoff-[a-f0-9]+\.exe)\)",(build/"cargo.log").read_text())
assert len(matches)==1
binary=Path(matches[0])
if not binary.is_absolute():binary=S/binary
assert binary.is_file()
def inventory():
    return guarded_inventory(A/"game")
def save(path,value):
    with path.open("x",encoding="utf-8") as file:json.dump(value,file,indent=2);file.write("\n")
if args.prepare:
    assert not config.exists() and not preparation.exists()
    cfg=json.loads((D/"handoff-attempt02.json").read_text())
    scene=json.loads((C/"after-scene.envelope.json").read_text())["data"]
    camera=json.loads((C/"after-camera.envelope.json").read_text())["data"]
    before=inventory()
    recovery=A/".local/godot-bridge01-fixture-save-backup01"
    original={row["path"]:{key:row[key] for key in ("bytes","sha256")} for row in json.loads((D/"scene-save-reconciliation01/source-files.json").read_text())}
    assert before==original
    for rel,row in before.items():assert H(recovery/rel)==row["sha256"]
    cfg.update(editor_session=console["sessions"]["sessions"][0]["session"],reconciled_stamp=scene["stamp"],
        reconciled_camera_data=camera["data"],fixture_before_sha256=before[cfg["supporting_scene"][6:]]["sha256"],
        perform_approved_reimport=True,native_operator_approval_required=True,
        approval_response_must_be_human=True)
    save(config,cfg);save(D/(name+".source-before.json"),before)
    sequence=[{"command":"driver.godot.asset.reimport","path":asset["path"],"source_sha256":asset["sha256"]} for asset in cfg["assets"]]
    sequence += [{"command":"driver.godot.scene.open","path":path} for path in [cfg["initial_scene"],cfg["supporting_scene"],cfg["initial_scene"]]]
    plan={"schema":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"state":"PREPARED_NOT_SUBMITTED",
        "project_root":cfg["project_root"],"broker_pid":console["new_broker"]["pid"],"editor_pid":console["editor"]["pid"],
        "native_approval_requests":sequence,"native_approval_timeout_seconds":120,
        "other_effects":["Save owned supporting scene with existing external-resource mode","Reject one stale camera configure request","Read back geometry/camera/editor state"],
        "protected_source_glbs":[{"path":x["path"],"sha256":x["sha256"]} for x in cfg["assets"]],
        "permitted_source_delta":[cfg["supporting_scene"][6:]]+[x["path"][6:]+".import" for x in cfg["assets"]],
        "preexisting_unsaved_main":"Must retain original live projection and unsaved flag; never saved or discarded",
        "build_report_sha256":H(build/"run.json"),"binary":str(binary),"binary_sha256":H(binary),
        "source_sha256":H(S/source),"config_sha256":H(config),"source_inventory_sha256":H(D/(name+".source-before.json")),
        "recovery_copy":str(recovery),"approval_input_automation":False,"automatic_retry":False,
        "G6":"NOT_COMPLETE","human_camera_claimed":False,"art_status":"REVISE"}
    save(preparation,plan);print(json.dumps(plan,indent=2));raise SystemExit(0)
plan=json.loads(preparation.read_text());assert plan["state"]=="PREPARED_NOT_SUBMITTED"
assert H(config)==plan["config_sha256"] and H(binary)==plan["binary_sha256"] and H(S/source)==plan["source_sha256"]
before=json.loads((D/(name+".source-before.json")).read_text());assert inventory()==before
for path in [D/name,D/(name+".log"),D/(name+".runner.json")]:assert not path.exists(),"Preserve earlier submission; no retry"
cfg=json.loads(config.read_text())
env=os.environ.copy();env.update(SEMWRIGHT_TEST_BROKER_PIPE=r["socket"],SEMWRIGHT_TEST_RECEIPTS=str(D/name),
    SEMWRIGHT_GODOT_PROOF_CONFIG=str(config),SEMWRIGHT_GODOT_PROOF_CONFIG_SHA256=H(config))
from godot_handoff_guard import execute_client_once, aggregate_outcomes
start=time.monotonic()
save(D/(name+".submission.json"),{
    "utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "preflight_receipt":str(Path(preflight["evidence_directory"])/"receipt.json"),
    "native_approval_response_must_be_human":True,"automatic_retry":False,
    "binary_sha256":H(binary),"config_sha256":H(config)})
summary={"schema":2,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "binary_sha256":H(binary),"config_sha256":H(config),"source_sha256":H(S/source),
    "preparation_sha256":H(preparation),"preflight_receipt":str(Path(preflight["evidence_directory"])/"receipt.json"),
    "native_approval_input_automated":False,"automatic_retry":False,
    "G6":"NOT_COMPLETE","human_camera_claimed":False,"art_status":"REVISE"}
execution={"client_started":False,"exit_code":None,"execution_error":None}
try:
    execution=execute_client_once(
        [str(binary),"--ignored","--exact","windows_broker_godot_finalize_known_fixture","--nocapture"],
        S,env,D/(name+".log"))
except (Exception,KeyboardInterrupt) as exc:
    execution["execution_error"]=type(exc).__name__+": "+str(exc)
finally:
    summary.update(execution)
    summary["seconds"]=time.monotonic()-start
    summary["proof_passed"]=False
    try:
        proof=json.loads((D/name/"receipt.json").read_text())
        summary["proof_passed"]=proof.get("passed") is True and proof.get("config_sha256")==H(config)
    except Exception as exc:
        summary["proof_receipt_error"]=type(exc).__name__+": "+str(exc)
    try:
        after=inventory()
        delta={k:{"before":before.get(k),"after":after.get(k)} for k in sorted(before.keys()|after.keys()) if before.get(k)!=after.get(k)}
        within=set(delta)<=set(plan["permitted_source_delta"]) and before.keys()==after.keys()
        changed=cfg["supporting_scene"][6:] in delta
        glbs=all(H(A/"game"/asset["path"][6:])==asset["sha256"] for asset in cfg["assets"])
        summary.update(source_delta=delta,source_delta_within_allowed_paths=within,
            fixture_changed_as_required_for_success=changed,protected_glbs_unchanged=glbs,
            permitted_source_delta_only=within and changed and glbs)
    except Exception as exc:
        summary["preservation_check_error"]=type(exc).__name__+": "+str(exc)
        summary["permitted_source_delta_only"]=False
    try:
        summary["request_outcomes"]=classify_requests(D/name)
    except Exception as exc:
        summary["request_outcomes"]=[]
        summary["classification_error"]=type(exc).__name__+": "+str(exc)
    summary["outcome"]=aggregate_outcomes(summary["request_outcomes"],execution,summary["proof_passed"])
    if summary.get("preservation_check_error") or not summary.get("source_delta_within_allowed_paths",False):
        summary["outcome"]="UNKNOWN"
    summary["passed"]=(execution["exit_code"]==0 and not execution["execution_error"]
        and summary["proof_passed"] and summary["permitted_source_delta_only"]
        and summary["outcome"]=="KNOWN")
    summary["requires_read_only_reconciliation"]=not summary["passed"]
    try:
        if (D/(name+".log")).exists():summary["log_sha256"]=H(D/(name+".log"))
    except Exception as exc:
        summary["log_digest_error"]=type(exc).__name__+": "+str(exc)
        summary["passed"]=False
        summary["requires_read_only_reconciliation"]=True
    save(D/(name+".runner.json"),summary)
print(json.dumps(summary,indent=2))
raise SystemExit(0 if summary["passed"] else 3)
