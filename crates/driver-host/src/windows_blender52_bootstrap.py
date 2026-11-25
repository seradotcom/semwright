"""Fixed first-party bootstrap for Host-owned Windows Blender 5.2.1 sessions."""
import os
import sys

marker = sys.argv.index("--")
transport, workspace, code, runtime = sys.argv[marker + 1:]
if os.name != "nt" or not transport.startswith("stdio:") or len(transport) != 38:
    raise SystemExit(2)
if any(c not in "0123456789abcdef" for c in transport[6:]):
    raise SystemExit(2)
# Best-effort fixed labels on the private pre-readiness pipe. No raw error,
# path, object data or new filesystem write. These cannot publish readiness.
def checkpoint(phase):
    try:
        os.write(sys.stdout.fileno(), b"\0SEMWRIGHT-BLENDER-STARTUP-v1:"
                 + transport[6:].encode("ascii") + b":" + phase.encode("ascii") + b"\0")
    except (OSError, ValueError, AttributeError):
        pass


checkpoint("bootstrap_imports")
import importlib
from pathlib import Path
import runpy

checkpoint("workspace_resolve")
try:
    workspace = Path(workspace).resolve(strict=True)
except OSError as error:
    # Only fixed classifications leave the native process, never a path or
    # exception message. Do not continue after an unresolved workspace.
    winerror = getattr(error, "winerror", None)
    if winerror == 5:
        checkpoint("workspace_access_denied")
    elif winerror in (2, 3):
        checkpoint("workspace_path_missing")
    elif winerror in (32, 33):
        checkpoint("workspace_sharing_denied")
    elif winerror == 123:
        checkpoint("workspace_invalid_name")
    else:
        checkpoint("workspace_resolve_os_error")
    raise
checkpoint("code_resolve")
code = Path(code).resolve(strict=True)
checkpoint("runtime_resolve")
runtime = Path(runtime).resolve(strict=True)
resources = runtime / "5.2"
checkpoint("resource_env_check")
if Path(os.environ.get("BLENDER_SYSTEM_RESOURCES", "")).resolve(strict=True) != resources:
    raise SystemExit(4)
checkpoint("bpy_import")
import bpy
checkpoint("version_background_check")
if tuple(bpy.app.version) != (5, 2, 1) or not bpy.app.background:
    raise SystemExit(4)
checkpoint("binary_path_check")
if Path(bpy.app.binary_path).resolve(strict=True) != runtime / "blender.exe":
    raise SystemExit(4)
checkpoint("local_resources_check")
if Path(bpy.utils.resource_path("LOCAL")).resolve(strict=True) != resources:
    raise SystemExit(4)
sys.dont_write_bytecode = True
sys.path.insert(0, str(code))
for short in ("commands", "validation", "semantic", "authoring_native", "export_scope"):
    checkpoint("module_" + short)
    module = importlib.import_module("semwright_blender_runtime." + short)
    if Path(module.__file__).resolve(strict=True) != code / "semwright_blender_runtime" / (short + ".py"):
        raise SystemExit(4)
sys.argv = sys.argv[:marker + 1] + [transport, str(workspace), str(code)]
checkpoint("bridge_load")
runpy.run_path(str(code / "bridge.py"), run_name="__main__")
