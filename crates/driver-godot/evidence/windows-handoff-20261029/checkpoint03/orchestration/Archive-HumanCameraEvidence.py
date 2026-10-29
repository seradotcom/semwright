"""Freeze real human-camera continuity and independent persistence evidence."""
from pathlib import Path
import base64,datetime,hashlib,json
from godot_handoff_guard import digest,inventory,is_reparse,require,save_new
A=Path(r"E:\Projects\worldwright-archipelago");S=Path(r"E:\Projects\semwright-blender52")
D=A/"evidence/integration/20261029-godot-broker/godot-bridge01"
O=S/"crates/driver-godot/evidence/windows-handoff-20261029/checkpoint03"
def read(p):return json.loads(p.read_text())
p=read(D/"prepared.json");proof=read(D/"human-camera-continuity01/receipt.json");native=read(D/"human-camera-persistence02/receipt.json")
require(proof["passed"] and native["passed"] and native["source_files_unchanged"],"Physical proof incomplete")
require(read(D/"human-camera-continuity01/source-after.json")==read(D/"human-camera-persistence02/source-before.json"),"Native source baseline changed")
secret=Path(p["secret_file"]).read_bytes().strip();needles={secret,base64.b64encode(secret)}
try:needles.add(bytes.fromhex(secret.decode()))
except (ValueError,UnicodeError):pass
needles={n for n in needles if len(n)>=24}
O.mkdir(parents=True,exist_ok=False)
(O/".gitattributes").write_text("** -text\n",encoding="utf-8",newline="\n")
files=[]
def copy(src,rel):
    require(src.is_file() and not is_reparse(src),"Unsafe archive source")
    raw=src.read_bytes();require(len(raw)<16*1024**2 and not any(n in raw for n in needles),"Archive byte/credential check")
    dest=O/rel;dest.parent.mkdir(parents=True,exist_ok=True)
    with dest.open("xb") as f:f.write(raw)
    files.append({"source":str(src),"archive":rel,"bytes":len(raw),"sha256":digest(dest)})
    return raw
audit=Path(p["paths"]["broker"])/"state/audit/audit.jsonl"
raw=audit.read_bytes();records=[];previous="";seq=0
H=lambda b:hashlib.sha256(b).hexdigest()
for line in raw.splitlines():
    if not line:continue
    row=json.loads(line);require(row["sequence"]==seq+1 and row["previous"]==previous,"Audit chain order")
    expected=row["hash"];row["hash"]=""
    require(H(json.dumps(row,ensure_ascii=False,separators=(",",":"),allow_nan=False).encode())==expected,"Audit hash")
    row["hash"]=expected;records.append(row);previous=expected;seq=row["sequence"]
require(copy(audit,"broker-audit-snapshot.jsonl")==raw and audit.read_bytes()==raw,"Audit snapshot changed")
index={}
for row in records:index.setdefault(row["request_id"],[]).append(row)
correlated=[];preexecution=[]
for name in ["human-camera-resume-20261029T060215Z","human-camera-continuity01","human-camera-persistence01","human-camera-persistence02"]:
    for rel in inventory(D/name):
        source=D/name/rel
        payload=copy(source,"proof/"+name+"/"+rel+(".txt" if source.suffix==".log" else ""))
        if not source.name.endswith(".envelope.json"):continue
        e=json.loads(payload);events=index.get(e.get("request_id"),[])
        start=[r for r in events if r["phase"]=="start"];finish=[r for r in events if r["phase"]=="finish"]
        if not start and not finish:
            require(e.get("ok") is False and e["error"].get("outcome_known") is True
                    and e["execution"]["backend"]=="unselected","Missing executed request audit")
            preexecution.append({"path":name+"/"+rel,"request_id":e["request_id"],"error":e["error"]})
            continue
        require(len(start)==len(finish)==1 and start[0]["command"]==finish[0]["command"]==e["command"],"Audit request identity")
        require(finish[0]["ok"]==e["ok"] and finish[0]["backend"]==e["execution"]["backend"]
                and finish[0]["decision"]==e["execution"]["policy_decision"],"Audit result mismatch")
        correlated.append({"path":name+"/"+rel,"request_id":e["request_id"],"command":e["command"],"start":start[0]["sequence"],"finish":finish[0]["sequence"],"ok":e["ok"]})
for name in ["Run-HumanCameraContinuity.py","Validate-HumanCameraPersistence.py","godot_handoff_guard.py","Archive-HumanCameraEvidence.py"]:
    copy(A/"scripts"/name,"orchestration/"+name)
for rel in ["proofs/semwright_godot_bridge01_02.tscn","main.tscn","addons/semwright/plugin.gd","addons/semwright/ops/scene_save_ops.gd"]:
    copy(A/"game"/rel,"native-project/"+rel)
report={"schema":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"files":files,
"audit":{"records":len(records),"sha256":H(raw),"tail_hash":previous,"consistent":True,"live_snapshot":True,"same_owner_tamper_resistance_claimed":False},
"correlated_envelopes":correlated,"preexecution_rejections_without_audit":preexecution,
"physical_scope":{"human_FOV_before":60,"human_FOV_observed":70,"preserved_cameras":["ProofCamera","ProofCamera2"],"managed_translation_m":[.5,0,0],
"node_patch_and_save":"PASS","independent_native_persistence":"PASS","source_preservation":"PASS","native_camera_properties":"FULL_EXPECTED_PROJECTION",
"native_first_observer":"FAIL_TYPED_NULL_RESOURCE_CODEC_ONLY; preserved","native_second_observer":"PASS",
"main_change_before_agent":"Native UID metadata saved externally; preserved","main_live_projection_this_run":"NOT_REOBSERVED_INACTIVE"},
"mutations":proof["mutations"],"historical_runner_outcome":"UNKNOWN_UNCHANGED",
"first_party_handoff_obligation":"BOUNDED_PROJECT_CASE_PASS","G6":"NOT_COMPLETE","art_status":"REVISE",
"Linux_4_5_14":"NOT_RUN","Linux_5_2_1":"NOT_RUN","external_publication":False}
save_new(O/"manifest.json",report)
summary={"archive":str(O),"payloads":len(files),"bytes":sum(f["bytes"] for f in files),"audit_records":len(records),
"correlated_envelopes":len(correlated),"preexecution_rejections_without_audit":len(preexecution),"manifest_sha256":digest(O/"manifest.json"),
"continuity_receipt_sha256":digest(D/"human-camera-continuity01/receipt.json"),"persistence_receipt_sha256":digest(D/"human-camera-persistence02/receipt.json")}
save_new(D/"human-camera-archive03.json",summary)
print(json.dumps(summary,indent=2))
