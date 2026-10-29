"""Bounded physical Godot proof. V2 uses a new path after an unanswered MCP write."""
from pathlib import Path
import argparse,datetime,hashlib,json,os,re,struct,subprocess,time
p=argparse.ArgumentParser();p.add_argument("--candidate",default="godot-bridge01");p.add_argument("--attempt",default="01");p.add_argument("--build-attempt",default="02");a=p.parse_args()
assert a.candidate.startswith("godot-bridge") and a.candidate[12:].isdigit() and a.attempt.isdigit() and a.build_attempt.isdigit()
A=Path(r"E:\Projects\worldwright-archipelago");S=Path(r"E:\Projects\semwright-blender52")
D=A/"evidence/integration/20261029-godot-broker"/a.candidate
r=json.loads((D/"prepared.json").read_text())
normalization=json.loads((D/"startup-config-reconciliation.json").read_text())
assert normalization["only_reviewed_formatting_and_default_elision"] and normalization["diagnostic_exit_code"]==0
project_baseline=normalization["normalized_project_sha256"]
H=lambda q:hashlib.sha256(Path(q).read_bytes()).hexdigest()
name="handoff-attempt"+a.attempt
for q in [D/name,D/(name+".json"),D/(name+".log"),D/(name+".runner.json")]:
    if q.exists():raise RuntimeError("Preserve prior attempt: "+str(q))
build=S/(".local/logs/native-profile-20261028-godot-handoff-proof-build-attempt"+a.build_attempt)
report=json.loads((build/"run.json").read_text())
assert report["exit_code"]==0 and report["sources_unchanged"]
source="crates/driver-blender/tests/windows_broker_godot_handoff.rs"
assert H(S/source)==report["source_after"][source]
matches=re.findall(r"Executable.*?\(([^\r\n]+windows_broker_godot_handoff-[a-f0-9]+\.exe)\)",(build/"cargo.log").read_text())
assert len(matches)==1,matches
binary=Path(matches[0])
if not binary.is_absolute():binary=S/binary
assert binary.is_file()
protected={k:H(Path(r["project_root"])/k) for k in r["protected_project_files"]}
assert protected==r["protected_project_files"]
assert H(Path(r["project_root"])/"project.godot")==project_baseline
assets=[]
for revision,instance in [("20261029-semwright-rock-r22","FaroRock"),("20261029-semwright-threshold-r23","FaroThreshold")]:
    admission=json.loads((A/"evidence/art_review"/revision/"asset-admission.json").read_text())
    row=next(x for x in admission["source_files"] if x["destination"].endswith(".glb"))
    q=A/row["destination"];assert H(q)==row["sha256"]
    raw=q.read_bytes()
    magic,version,total=struct.unpack_from("<III",raw);assert magic==0x46546C67 and version==2 and total==len(raw)
    n,kind=struct.unpack_from("<II",raw,12);assert kind==0x4E4F534A
    glb=json.loads(raw[20:20+n].decode().rstrip(" \x00"))
    assert all("uri" not in b for b in glb.get("buffers",[]))
    meshes=[]
    for node in glb["nodes"]:
        if "mesh" not in node:continue
        assert all(key not in node for key in ["matrix","translation","rotation","scale"])
        mesh=glb["meshes"][node["mesh"]];prims=mesh["primitives"]
        bounds=[glb["accessors"][x["attributes"]["POSITION"]] for x in prims]
        meshes.append({"name":node["name"],"min":[min(x["min"][i] for x in bounds) for i in range(3)],
                       "max":[max(x["max"][i] for x in bounds) for i in range(3)],"surface_count":len(prims),
                       "glb_position_count":sum(x["count"] for x in bounds)})
    rel=q.relative_to(A/"game").as_posix()
    assets.append({"path":"res://"+rel,"sha256":H(q),"bytes":len(raw),"instance":instance,"meshes":meshes,
                   "blender_export_request":admission.get("export_request",admission.get("export_request_id"))})
scene="res://proofs/semwright_"+a.candidate.replace("-","_")+"_"+a.attempt+".tscn"
(A/"game/proofs").mkdir(exist_ok=True)
assert not (A/"game"/scene[6:]).exists()
baseline_editor=json.loads((D/"startup-inspection01/editor.envelope.json").read_text())["data"]["data"]
baseline_scene=json.loads((D/"startup-inspection01/scene.envelope.json").read_text())["data"]["data"]
cfg={"project_root":r["project_root"],"project_id":r["project_id"],"supporting_scene":scene,"assets":assets,
     "initial_scene":baseline_scene["scene"],"initial_scene_nodes":baseline_scene["nodes"],"initial_unsaved_scenes":baseline_editor["unsaved_scenes"]}
input_path=D/(name+".json")
with input_path.open("x",encoding="utf-8") as f:json.dump(cfg,f,indent=2);f.write("\n")
env=os.environ.copy()
env.update(SEMWRIGHT_TEST_BROKER_PIPE=r["socket"],SEMWRIGHT_TEST_RECEIPTS=str(D/name),
SEMWRIGHT_GODOT_PROOF_CONFIG=str(input_path),SEMWRIGHT_GODOT_PROOF_CONFIG_SHA256=H(input_path))
cmd=[str(binary),"--ignored","--exact","windows_broker_godot_handoff","--nocapture","--test-threads=1"]
stamp=datetime.datetime.now(datetime.timezone.utc).isoformat();start=time.monotonic()
with (D/(name+".log")).open("xb") as log:
    proc=subprocess.Popen(cmd,cwd=S,env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT)
    print(json.dumps({"pid":proc.pid,"proof":name}),flush=True)
    try:code=proc.wait(timeout=210)
    except subprocess.TimeoutExpired:
        proc.kill();code=proc.wait(timeout=10)
        print("PROOF_CLIENT_DEADLINE_UNKNOWN; no mutation replay",flush=True)
after={k:H(Path(r["project_root"])/k) for k in protected}
result={"schema":1,"utc_start":stamp,"seconds":time.monotonic()-start,"exit_code":code,
"binary":str(binary),"binary_sha256":H(binary),"build_report":str(build/"run.json"),"build_report_sha256":H(build/"run.json"),
"source_sha256":H(S/source),"config_sha256":H(input_path),"protected_before":protected,"protected_after":after,
"protected_files_unchanged":protected==after,"project_config_unchanged":H(A/"game/project.godot")==project_baseline,"project_config_baseline_sha256":project_baseline,
"log_sha256":H(D/(name+".log")),"editor_isolated":False,"art_status":"REVISE","G6":"NOT_COMPLETE"}
with (D/(name+".runner.json")).open("x",encoding="utf-8") as f:json.dump(result,f,indent=2);f.write("\n")
print(json.dumps({k:v for k,v in result.items() if k not in ["protected_before","protected_after"]},indent=2))
if code:print((D/(name+".log")).read_text(errors="replace")[-10000:])
raise SystemExit(code or (0 if protected==after and result["project_config_unchanged"] else 3))
