"""Fixed first-party bootstrap for Host-owned Windows Blender 5.2.1 sessions."""
import importlib
import os
from pathlib import Path
import runpy
import sys

marker = sys.argv.index("--")
transport, workspace, code, runtime = sys.argv[marker + 1:]
if os.name != "nt" or not transport.startswith("stdio:") or len(transport) != 38:
    raise SystemExit(2)
if any(c not in "0123456789abcdef" for c in transport[6:]):
    raise SystemExit(2)
workspace = Path(workspace).resolve(strict=True)
code = Path(code).resolve(strict=True)
runtime = Path(runtime).resolve(strict=True)
resources = runtime / "5.2"
if Path(os.environ.get("BLENDER_SYSTEM_RESOURCES", "")).resolve(strict=True) != resources:
    raise SystemExit(4)
import bpy
if (tuple(bpy.app.version) != (5, 2, 1) or not bpy.app.background
        or Path(bpy.app.binary_path).resolve(strict=True) != runtime / "blender.exe"
        or Path(bpy.utils.resource_path("LOCAL")).resolve(strict=True) != resources):
    raise SystemExit(4)
sys.dont_write_bytecode = True
sys.path.insert(0, str(code))
for short in ("commands", "validation", "semantic", "authoring_native", "export_scope"):
    module = importlib.import_module("semwright_blender_runtime." + short)
    if Path(module.__file__).resolve(strict=True) != code / "semwright_blender_runtime" / (short + ".py"):
        raise SystemExit(4)
sys.argv = sys.argv[:marker + 1] + [transport, str(workspace), str(code)]
runpy.run_path(str(code / "bridge.py"), run_name="__main__")
