#!/usr/bin/env python3
"""Run the fixed native observer against a project already authored by the product."""
import hashlib, json, os, pathlib, subprocess, sys
ROOT = pathlib.Path(__file__).resolve().parents[2]
if len(sys.argv) != 4:
    raise SystemExit("usage: native-run.py GODOT PRODUCT_PROJECT EVIDENCE_DIR")
godot = pathlib.Path(sys.argv[1]).resolve(); project = pathlib.Path(sys.argv[2]).resolve(); out = pathlib.Path(sys.argv[3]).resolve()
helper = (ROOT / "integrations/godot/authoring/native_observer.gd").resolve()
if not godot.is_file() or not (project / "project.godot").is_file() or not helper.is_file():
    raise SystemExit("native authoring prerequisites missing")
if (project / "addons").exists() or (project / "integrations").exists():
    raise SystemExit("product-authored project unexpectedly contains authoring tooling")
out.mkdir(parents=True, exist_ok=False)
manifest = project / "project.semwright.json"
if not manifest.is_file(): raise SystemExit("provider derivation manifest missing")
source = hashlib.sha256(manifest.read_bytes()).hexdigest()

def execute(label, mode, ticks=0, inputs=None, checkpoints=None, variables=None):
    request = {"version":1,"nonce":f"native_{label}_receipt_0001","source_fingerprint":source,"mode":mode,
        "scene":"res://scenes/arena.tscn","ticks":ticks,"inputs":inputs or [],"checkpoints":checkpoints or [],
        "variables":variables or [],"capture":False}
    req=out/f"request-{label}.json"; obs=out/f"observation-{label}.json"; log=out/f"godot-{label}.log"
    req.write_text(json.dumps(request,separators=(",",":")))
    command=[str(godot),"--headless","--path",str(project),"--script",str(helper),"--","--request",str(req),"--output",str(obs)]
    result=subprocess.run(command,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,check=False)
    log.write_text(result.stdout)
    if result.returncode != 0 or not obs.is_file(): raise SystemExit(f"native {label} failed ({result.returncode})\n{result.stdout[-8000:]}")
    data=json.loads(obs.read_text())
    if data.get("failures") != []: raise SystemExit(f"native {label} reported failures: {data.get('failures')}")
    return {"mode":mode,"request_sha256":hashlib.sha256(req.read_bytes()).hexdigest(),"observation_sha256":hashlib.sha256(obs.read_bytes()).hexdigest(),"bytes":obs.stat().st_size}

import_log=out/"godot-import.log"
imported=subprocess.run([str(godot),"--headless","--path",str(project),"--import"],stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,check=False)
import_log.write_text(imported.stdout)
if imported.returncode != 0: raise SystemExit("Godot import failed\n"+imported.stdout[-8000:])
runs=[]
runs.append(execute("inspect","inspect"))
runs.append(execute("save","save_candidate"))
runs.append(execute("reopen","reopen_candidate"))
runs.append(execute("play","play",10,[{"tick":1,"action":"start","pressed":True},{"tick":2,"action":"start","pressed":False}],[1,2,10],["score"]))
receipt={"schema_version":1,"role":"D","source_sha":subprocess.check_output(["git","rev-parse","HEAD"],cwd=ROOT,text=True).strip(),
    "github_sha":os.environ.get("GITHUB_SHA"),"engine":subprocess.check_output([str(godot),"--version"],text=True).strip(),
    "product_project":project.name,"derivation_manifest_sha256":source,"native_observer_sha256":hashlib.sha256(helper.read_bytes()).hexdigest(),"runs":runs}
(out/"native-run.json").write_text(json.dumps(receipt,indent=2)+"\n")
print(json.dumps(receipt,indent=2))
