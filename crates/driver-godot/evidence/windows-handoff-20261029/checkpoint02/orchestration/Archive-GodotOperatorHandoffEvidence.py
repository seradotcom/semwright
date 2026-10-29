"""Freeze the completed operator handoff without rewriting uncertain original receipts."""
from pathlib import Path
import base64,datetime,hashlib,json
from godot_handoff_guard import inventory,is_reparse
A=Path(r"E:\Projects\worldwright-archipelago")
S=Path(r"E:\Projects\semwright-blender52")
SOURCE=A/"evidence/integration/20261029-godot-broker/godot-bridge01"
DEST=S/"crates/driver-godot/evidence/windows-handoff-20261029/checkpoint02"
H=lambda raw:hashlib.sha256(raw).hexdigest()
def read(p):return json.loads(p.read_text())
reconciliation=read(SOURCE/"operator-handoff-reconciliation01/receipt.json")
native=read(SOURCE/"persisted-fixture-readback01/receipt.json")
post=read(SOURCE/"persisted-fixture-readback01/post-readback-preservation.json")
runner=read(SOURCE/"operator-handoff-attempt01.runner.json")
assert reconciliation["observed_postconditions_passed"] and native["passed"] and post["passed"]
assert runner["outcome"]=="UNKNOWN" and runner["passed"] is False
prepared=read(SOURCE/"prepared.json")
secret=Path(prepared["secret_file"]).read_bytes().strip()
assert len(secret)>=32
needles={secret,base64.b64encode(secret)}
try:
    decoded=bytes.fromhex(secret.decode("ascii"))
    needles|={decoded,base64.b64encode(decoded)}
except (ValueError,UnicodeError):pass
def collect_sensitive(v):
    if isinstance(v,dict):
        for key,value in v.items():
            if any(x in key.lower() for x in ("token","secret","ticket","key")) and isinstance(value,str) and len(value)>=24:
                needles.add(value.encode())
            collect_sensitive(value)
    elif isinstance(v,list):
        for value in v:collect_sensitive(value)
for path in Path(prepared["paths"]["config"]).glob("client-session*.json"):
    collect_sensitive(read(path))
needles={n for n in needles if len(n)>=24}
DEST.mkdir(parents=True,exist_ok=False)
(DEST/".gitattributes").write_text("** -text\n",encoding="utf-8",newline="\n")
manifest=[]
def copy(src,relative):
    assert not is_reparse(src) and src.is_file()
    raw=src.read_bytes()
    assert len(raw)<=16*1024**2
    assert not any(n in raw for n in needles),"Credential material detected; archive stopped"
    dst=DEST/relative
    dst.parent.mkdir(parents=True,exist_ok=True)
    with dst.open("xb") as f:f.write(raw)
    assert H(dst.read_bytes())==H(raw)
    manifest.append({"source":str(src),"archive":relative,"bytes":len(raw),"sha256":H(raw)})
    return raw

audit=Path(prepared["paths"]["broker"])/"state/audit/audit.jsonl"
audit_bytes=audit.read_bytes()
assert len(audit_bytes)<8*1024**2
records=[];previous="";sequence=0
for line in audit_bytes.splitlines():
    if not line:continue
    row=json.loads(line)
    assert row["sequence"]==sequence+1 and row["previous"]==previous
    value=row["hash"];row["hash"]=""
    assert H(json.dumps(row,ensure_ascii=False,separators=(",",":"),allow_nan=False).encode())==value
    row["hash"]=value;records.append(row);previous=value;sequence=row["sequence"]
assert copy(audit,"broker-audit-snapshot.jsonl")==audit_bytes and audit.read_bytes()==audit_bytes
audit_index={}
for row in records:audit_index.setdefault(row["request_id"],[]).append(row)

selected=set()
for name in ["operator-handoff-attempt01",
             "operator-handoff-attempt01.preflight-20261029T045250Z-f77a28cb",
             "operator-handoff-reconciliation01","persisted-fixture-readback01",
             "guard-validation02"]:
    for rel in inventory(SOURCE/name):
        selected.add(SOURCE/name/rel)
for name in ["prepared.json","operator-handoff-attempt01.json",
             "operator-handoff-attempt01.prepared.json","operator-handoff-attempt01.source-before.json",
             "operator-handoff-attempt01.submission.json","operator-handoff-attempt01.runner.json",
             "operator-handoff-attempt01.log"]:
    selected.add(SOURCE/name)
correlations=[];requests=[]
for src in sorted(selected):
    assert src.suffix.lower() not in {".secret",".key",".env"} and "client-session" not in src.name
    relative=src.relative_to(SOURCE).as_posix()
    raw=copy(src,"proof/"+relative+(".txt" if src.suffix==".log" else ""))
    if src.suffix!=".json":continue
    try:obj=json.loads(raw)
    except json.JSONDecodeError:continue
    if not isinstance(obj,dict) or not {"request_id","command"}<=obj.keys():continue
    if src.name.endswith(".request.json"):
        requests.append(obj["request_id"])
        assert src.with_name(src.name.replace(".request.json",".envelope.json")).is_file()
    if not {"ok","execution"}<=obj.keys():continue
    events=audit_index.get(obj["request_id"],[])
    start=[r for r in events if r["phase"]=="start"]
    finish=[r for r in events if r["phase"]=="finish"]
    assert len(start)==len(finish)==1,(relative,obj["request_id"])
    last=finish[0];execution=obj["execution"]
    assert start[0]["command"]==last["command"]==obj["command"]
    assert last["ok"]==obj["ok"] and last["backend"]==execution["backend"]
    assert last["decision"]==execution["policy_decision"] and last.get("provenance")==execution.get("provenance")
    assert last["error_code"]==obj.get("error",{}).get("code")
    correlations.append({"path":relative,"request_id":obj["request_id"],"command":obj["command"],
        "start_sequence":start[0]["sequence"],"finish_sequence":last["sequence"],"ok":obj["ok"]})
assert len(requests)==35 and set(requests)<={r["request_id"] for r in correlations}
for name in ["Run-GodotOperatorHandoff.py","godot_handoff_guard.py",
             "Reconcile-GodotOperatorHandoff.py","Validate-GodotPersistedFixture.py",
             "Archive-GodotOperatorHandoffEvidence.py"]:
    copy(A/"scripts"/name,"orchestration/"+name)
copy(A/"tests/test_godot_handoff_guard.py","orchestration/test_godot_handoff_guard.py")
for rel in ["addons/semwright/plugin.gd","addons/semwright/ops/rendering_ops.gd",
            "addons/semwright/ops/scene_save_ops.gd","proofs/semwright_godot_bridge01_02.tscn"]:
    copy(A/"game"/rel,"native-project/"+rel)
copy(S/"crates/driver-blender/tests/windows_broker_godot_handoff.rs",
     "proof-source/windows_broker_godot_handoff.rs")
report={"schema":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"files":manifest,
    "audit":{"sha256":H(audit_bytes),"records":len(records),"tail_hash":previous,
        "consistent":True,"snapshot_of_live_idle_broker":True,
        "malicious_same_owner_tamper_resistance_claimed":False},
    "correlated_envelopes":correlations,"explicit_request_pairs":len(requests),"unmatched_requests":0,
    "credential_scan":"Pairing secret and persisted token/key fields compared in memory; values not archived or printed",
    "prior_build_archive":"../checkpoint01",
    "prior_build_manifest_sha256":"17b9a7a6a649649acb26a65fd4c6356c224458203784c26e4e350b535c18dc5b",
    "physical_scope":{"native_operator_approvals":5,"broker_reimports":"PASS_FOR_UNCHANGED_R22_R23",
        "broker_supporting_scene_save":"PASS","independent_native_reopen":"PASS",
        "source_preservation":"PASS_ONLY_SUPPORT_SCENE_CHANGED",
        "stale_camera_raw_outcome":"UNKNOWN","stale_camera_observed_postcondition":"UNCHANGED",
        "original_runner_passed":False,"separate_read_only_reconciliation_passed":True,
        "human_authored_camera_preservation":"NOT_DEMONSTRATED","geometric_A_to_B_reimport":"NOT_RUN"},
    "Godot_editor_isolated":False,"Godot_driver_LPAC":True,"G6":"NOT_COMPLETE",
    "art_status":"REVISE","Linux_4_5_14":"NOT_RUN","Linux_5_2_1":"NOT_RUN",
    "external_publication":False}
with (DEST/"manifest.json").open("x",encoding="utf-8",newline="\n") as f:
    json.dump(report,f,indent=2,ensure_ascii=False);f.write("\n")
print(json.dumps({"archive":str(DEST),"files":len(manifest),"bytes":sum(x["bytes"] for x in manifest),
    "audit_records":len(records),"correlated_envelopes":len(correlations),
    "explicit_request_pairs":len(requests),"unmatched_requests":0,
    "manifest_sha256":H((DEST/"manifest.json").read_bytes())},indent=2))
