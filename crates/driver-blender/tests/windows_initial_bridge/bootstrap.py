"""Fixed first-party test bootstrap; no caller-supplied code or script selection."""
import json
import os
import runpy
import sys
import traceback
from pathlib import Path

marker = sys.argv.index("--")
transport, workspace, code, runtime = sys.argv[marker + 1:]
workspace = Path(workspace).resolve(strict=True)
code = Path(code).resolve(strict=True)
runtime = Path(runtime).resolve(strict=True)
try:
    if os.name != "nt" or len(transport) != 38 or not transport.startswith("stdio:"):
        raise RuntimeError("Invalid diagnostic transport")
    if any(c not in "0123456789abcdef" for c in transport[6:]):
        raise RuntimeError("Invalid diagnostic nonce")
    import bpy
    observed_binary = Path(bpy.app.binary_path).resolve(strict=True)
    observed_resources = Path(bpy.utils.resource_path("LOCAL")).resolve(strict=True)
    if observed_binary != runtime / "blender.exe":
        raise RuntimeError("Unexpected native executable")
    if observed_resources != runtime / "5.2":
        raise RuntimeError("Unexpected native resource root")
    if tuple(bpy.app.version) != (5, 2, 1) or not bpy.app.background:
        raise RuntimeError("Unexpected native runtime")
    # The real resource path was independently queried above; this is not a
    # replacement runtime. Existing bridge validation expects the environment.
    os.environ["BLENDER_SYSTEM_RESOURCES"] = str(observed_resources)
    sys.dont_write_bytecode = True
    import importlib
    sys.path.insert(0, str(code))
    module_origins = {}
    for short in ("commands", "validation", "semantic", "authoring_native", "export_scope"):
        module = importlib.import_module("semwright_blender_runtime." + short)
        actual = Path(module.__file__).resolve(strict=True)
        if actual != code / "semwright_blender_runtime" / (short + ".py"):
            raise RuntimeError("Unexpected first-party module origin")
        module_origins[short] = str(actual)
    # An owner-readable diagnostic within the one authorized empty workspace.
    with (workspace / "bootstrap-observation.json").open("x", encoding="utf-8") as out:
        json.dump({"version": list(bpy.app.version), "binary": str(observed_binary),
                   "resources": str(observed_resources), "background": bpy.app.background,
                   "nonce": transport[6:], "broker_used": False,
                   "resources_env_set_after_startup": True,
                   "resources_env_basis": "verified bpy local resource path",
                   "module_origins": module_origins}, out)
    sys.argv = sys.argv[:marker + 1] + [transport, str(workspace), str(code)]
    runpy.run_path(str(code / "bridge.py"), run_name="__main__")
except BaseException:
    with (workspace / "bootstrap-error.txt").open("x", encoding="utf-8") as out:
        out.write(traceback.format_exc()[-8192:])
    raise
