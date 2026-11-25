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

def _native_path_probe(path):
    import ctypes
    from ctypes import wintypes

    class FileInfo(ctypes.Structure):
        _fields_ = [
            ("attributes", wintypes.DWORD),
            ("created", wintypes.FILETIME),
            ("accessed", wintypes.FILETIME),
            ("written", wintypes.FILETIME),
            ("volume", wintypes.DWORD),
            ("size_high", wintypes.DWORD),
            ("size_low", wintypes.DWORD),
            ("links", wintypes.DWORD),
            ("index_high", wintypes.DWORD),
            ("index_low", wintypes.DWORD),
        ]

    kernel = ctypes.WinDLL("kernel32.dll", use_last_error=True, winmode=0x00000800)
    create = kernel.CreateFileW
    create.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p,
                       wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    create.restype = wintypes.HANDLE
    info = kernel.GetFileInformationByHandle
    info.argtypes = [wintypes.HANDLE, ctypes.POINTER(FileInfo)]
    info.restype = wintypes.BOOL
    final = kernel.GetFinalPathNameByHandleW
    final.argtypes = [wintypes.HANDLE, wintypes.LPWSTR, wintypes.DWORD, wintypes.DWORD]
    final.restype = wintypes.DWORD
    close = kernel.CloseHandle
    close.argtypes = [wintypes.HANDLE]
    close.restype = wintypes.BOOL
    # FILE_READ_ATTRIBUTES, share all, OPEN_EXISTING, OPEN_REPARSE_POINT |
    # BACKUP_SEMANTICS. This opens only the existing requested DOS path.
    handle = create(str(path), 0x80, 7, None, 3, 0x02200000, None)
    if not handle or handle == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        metadata = FileInfo()
        if not info(handle, ctypes.byref(metadata)):
            raise ctypes.WinError(ctypes.get_last_error())
        if metadata.attributes & 0x400:
            raise PermissionError("Native path is a reparse point")
        buffer = ctypes.create_unicode_buffer(32768)
        # FILE_NAME_NORMALIZED | VOLUME_NAME_NT: no DOS drive translation.
        length = final(handle, buffer, len(buffer), 2)
        if not length:
            raise ctypes.WinError(ctypes.get_last_error())
        if length >= len(buffer):
            raise ValueError("Native physical name exceeds its bound")
        return (buffer.value, (metadata.volume, metadata.index_high, metadata.index_low),
                bool(metadata.attributes & 0x10), metadata.links)
    finally:
        close(handle)


class NativePathPins:
    """Strict existing-object resolver for the three Host-pinned mounts only."""
    def __init__(self, records, probe=_native_path_probe):
        self._probe = probe
        self._pins = {}
        if not isinstance(records, list) or len(records) != 3:
            raise ValueError("Native path pins require three mount roots")
        for item in records:
            role = item["role"]
            root, canonical = self._local_path(item["path"]), self._local_path(item["canonical"])
            physical = item["nt"]
            identity = tuple(item["identity"])
            if (role in self._pins or role not in {"workspace", "native-bootstrap", "blender-runtime"}
                    or not isinstance(physical, str) or not physical.startswith("\\Device\\")
                    or len(identity) != 3 or any(type(n) is not int or not 0 <= n <= 0xffffffff for n in identity)):
                raise ValueError("Invalid native mount pin")
            self._pins[role] = (root, canonical, physical, identity)

    @staticmethod
    def _component(name):
        if (not name or name in {".", ".."} or name.endswith((" ", "."))
                or any(ord(c) < 32 or ord(c) == 127 or c in '\\/:<>"|?*' for c in name)):
            raise PermissionError("Unsafe native path component")
        stem = name.split(".", 1)[0].rstrip(" ").upper()
        devices = {"CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$"}
        devices.update(prefix + digit for prefix in ("COM", "LPT") for digit in "123456789\u00b9\u00b2\u00b3")
        if stem in devices:
            raise PermissionError("Device path is not a native mount component")

    @classmethod
    def _local_path(cls, path):
        value = Path(path)
        drive = value.drive
        if (not value.is_absolute() or len(drive) != 2 or drive[1] != ":"
                or drive[0] not in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
                or value.parent == value):
            raise PermissionError("Native path must be an absolute local non-root path")
        for component in value.parts[1:]:
            cls._component(component)
        return value

    def resolve(self, path, role):
        value = self._local_path(path)
        root, canonical, physical, identity = self._pins[role]
        for spelling in (root, canonical):
            try:
                relative = value.relative_to(spelling)
            except ValueError:
                continue
            current = Path(*value.parts[:len(spelling.parts)])
            break
        else:
            raise PermissionError("Native path is outside its pinned mount")
        actual, observed, directory, _ = self._probe(current)
        if not directory or observed != identity or actual != physical:
            raise PermissionError("Native mount physical identity changed")
        result = canonical
        if str(result) != str(current) and self._probe(result)[:3] != (actual, observed, directory):
            raise PermissionError("Canonical native mount spelling changed identity")
        parent = physical
        for index, component in enumerate(relative.parts):
            current = current / component
            actual, observed, directory, links = self._probe(current)
            prefix = parent + "\\"
            if not actual.startswith(prefix) or observed[0] != identity[0]:
                raise PermissionError("Native path escaped its physical parent")
            # Exactly one normalized physical component, not a junction,
            # redirected volume, alternate stream or lexical fallback.
            name = actual[len(prefix):]
            self._component(name)
            if (index + 1 < len(relative.parts) and not directory) or (not directory and links != 1):
                raise PermissionError("Native path is not an ordinary directory or single-link file")
            result = result / name
            if str(result) != str(current) and self._probe(result) != (actual, observed, directory, links):
                raise PermissionError("Canonical native path spelling changed identity")
            parent = actual
        return result


import json

checkpoint("workspace_resolve")
pins = NativePathPins(json.loads(os.environ.pop("SEMWRIGHT_NATIVE_PATH_PINS")))
try:
    workspace = pins.resolve(workspace, "workspace")
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
checkpoint("workspace_nt_verified")
checkpoint("code_resolve")
code = pins.resolve(code, "native-bootstrap")
checkpoint("runtime_resolve")
runtime = pins.resolve(runtime, "blender-runtime")
resources = runtime / "5.2"
checkpoint("resource_env_check")
if pins.resolve(os.environ.get("BLENDER_SYSTEM_RESOURCES", ""), "blender-runtime") != resources:
    raise SystemExit(4)
checkpoint("bpy_import")
import bpy
checkpoint("version_background_check")
if tuple(bpy.app.version) != (5, 2, 1) or not bpy.app.background:
    raise SystemExit(4)
checkpoint("binary_path_check")
if pins.resolve(bpy.app.binary_path, "blender-runtime") != runtime / "blender.exe":
    raise SystemExit(4)
checkpoint("local_resources_check")
if pins.resolve(bpy.utils.resource_path("LOCAL"), "blender-runtime") != resources:
    raise SystemExit(4)
sys.dont_write_bytecode = True
sys.path.insert(0, str(code))
for short in ("commands", "validation", "semantic", "authoring_native", "export_scope"):
    checkpoint("module_" + short)
    module = importlib.import_module("semwright_blender_runtime." + short)
    if pins.resolve(module.__file__, "native-bootstrap") != code / "semwright_blender_runtime" / (short + ".py"):
        raise SystemExit(4)
sys.argv = sys.argv[:marker + 1] + [transport, str(workspace), str(code)]
checkpoint("bridge_load")
runpy.run_path(str(pins.resolve(code / "bridge.py", "native-bootstrap")),
               init_globals={"_native_workspace_resolve": lambda path: pins.resolve(path, "workspace")},
               run_name="__main__")
