"""Explicit synchronization; build.rs never writes source files."""
from pathlib import Path
import argparse,hashlib,json
C=Path(__file__).resolve().parents[1]
MAPPING=[('bridge.py', '../driver-blender/src/bridge.py', 'bridge.py'), ('commands.py', '../../adapters/blender/semwright_blender/commands.py', 'semwright_blender_runtime/commands.py'), ('commands.json', '../../adapters/blender/semwright_blender/commands.json', 'semwright_blender_runtime/commands.json'), ('validation.py', '../../adapters/blender/semwright_blender/validation.py', 'semwright_blender_runtime/validation.py'), ('semantic.py', '../driver-blender/src/semantic.py', 'semwright_blender_runtime/semantic.py'), ('authoring_native.py', '../driver-blender/src/authoring_native.py', 'semwright_blender_runtime/authoring_native.py'), ('export_scope.py', '../../adapters/blender/semwright_blender/export_scope.py', 'semwright_blender_runtime/export_scope.py')]
p=argparse.ArgumentParser();p.add_argument("--check",action="store_true");a=p.parse_args()
D=C/"src/windows_blender52_runtime"
if a.check and not D.is_dir():raise SystemExit("Runtime bundle directory is missing")
if not a.check:D.mkdir(exist_ok=True)
rows=[]
for name,canonical,target in MAPPING:
 data=(C/canonical).read_bytes()
 if a.check:
  if (D/name).read_bytes()!=data:raise SystemExit("Runtime drift: "+name)
 else:(D/name).write_bytes(data)
 rows.append({"file":name,"canonical":canonical,"private_target":target,"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()})
bootstrap=(C/"src/windows_blender52_bootstrap.py").read_bytes()
manifest={"schema":1,"origin":"first-party canonical source byte copies; not an external signature","files":rows,
 "host_local_bootstrap":{"file":"src/windows_blender52_bootstrap.py","private_target":"bootstrap.py","bytes":len(bootstrap),"sha256":hashlib.sha256(bootstrap).hexdigest()},
 "empty_package":{"private_target":"semwright_blender_runtime/__init__.py","bytes":0,"sha256":hashlib.sha256(b"").hexdigest()}}
data=(json.dumps(manifest,indent=2)+"\n").encode("utf-8")
if a.check:
 if (D/"manifest.json").read_bytes()!=data:raise SystemExit("Runtime manifest drift")
else:(D/"manifest.json").write_bytes(data)
print("First-party runtime bundle matches all declared sources.")
