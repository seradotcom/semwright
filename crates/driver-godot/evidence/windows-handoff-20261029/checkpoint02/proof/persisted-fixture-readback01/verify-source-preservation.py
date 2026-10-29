"""Verify source and owner-process preservation after independent native readback."""
from pathlib import Path
import datetime,json,sys
A=Path(r"E:\Projects\worldwright-archipelago")
sys.path.insert(0,str(A/"scripts"))
from godot_handoff_guard import inventory,digest,require,save_new,process_identity,check_process_identity
D=A/"evidence/integration/20261029-godot-broker/godot-bridge01"
O=D/"persisted-fixture-readback01"
def read(p):return json.loads(p.read_text())
r={"schema":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"passed":False,
   "read_only":True,"new_mutations_submitted":0,"original_runner_outcome_retained":"UNKNOWN",
   "G6":"NOT_COMPLETE","human_edit_preservation_claimed":False}
try:
    reconciliation=read(D/"operator-handoff-reconciliation01/receipt.json")
    native=read(O/"receipt.json")
    require(reconciliation["observed_postconditions_passed"] and native["passed"],
            "Required scoped observations missing")
    runner=read(D/"operator-handoff-attempt01.runner.json")
    baseline=read(Path(runner["preflight_receipt"]))
    for key in ("broker","editor"):
        check_process_identity(baseline[key],process_identity(baseline[key]["pid"]))
    sources=inventory(A/"game")
    require(sources==read(D/"operator-handoff-reconciliation01/source-files.json"),
            "Independent reader changed project source files")
    require(runner["outcome"]=="UNKNOWN" and runner["passed"] is False,
            "Historical runner outcome changed")
    for path,sha in reconciliation["original_evidence"].items():
        require(digest(A/path)==sha,"Historical evidence changed")
    require(digest(A/"game/proofs/semwright_godot_bridge01_02.tscn")
            ==native["source_scene_sha256"],"Persisted scene changed")
    r.update(passed=True,source_file_count=len(sources),project_source_changes_from_reader=[],
             native_receipt_sha256=digest(O/"receipt.json"),
             reconciliation_receipt_sha256=digest(D/"operator-handoff-reconciliation01/receipt.json"),
             original_runner_sha256=digest(D/"operator-handoff-attempt01.runner.json"),
             owner_processes_preserved=baseline["broker"]["pid"]==27564 and baseline["editor"]["pid"]==39036)
except Exception as exc:
    r["error"]=type(exc).__name__+": "+str(exc)
r["script_sha256"]=digest(Path(__file__))
save_new(O/"post-readback-preservation.json",r)
print(json.dumps(r,indent=2))
raise SystemExit(0 if r["passed"] else 3)
