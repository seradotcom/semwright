#!/usr/bin/env python3
"""Isolated native oracle. Called ONLY by the compiled native consumer example.
No client receipts, expressions, arbitrary commands or external project paths.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import time

CASES = {"godot": {"baseline", "external-mutant", "observation-mutant", "readback-fault"},
         "blender": {"baseline", "external-mutant", "membership-mutant", "readback-fault"}}
if os.environ.get("GITHUB_ACTIONS") != "true":
    raise SystemExit("native probes are GitHub-hosted CI only")
if len(sys.argv) != 3 or sys.argv[1] not in CASES or sys.argv[2] not in CASES[sys.argv[1]]:
    raise SystemExit("invalid fixed native case")
backend, case = sys.argv[1:]
source = Path(__file__).resolve().parents[2]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def projection(value):
    return digest(json.dumps(value,sort_keys=True,separators=(",", ":"),allow_nan=False).encode())


def inventory(root):
    # Explicit isolated root; bounded recursion, no symlink following or HOME scan.
    result = {}
    pending = [(root, 0)]
    while pending:
        directory, depth = pending.pop()
        if depth > 4: raise RuntimeError("fixture directory depth exceeded")
        for entry in sorted(directory.iterdir()):
            if entry.is_symlink(): raise RuntimeError("symlink in bounded fixture root")
            relative = str(entry.relative_to(root))
            if entry.is_dir():
                result[relative + "/"] = "directory"
                pending.append((entry, depth + 1))
            elif entry.is_file():
                if entry.stat().st_size > 16 * 1024 * 1024: raise RuntimeError("fixture file budget exceeded")
                result[relative] = digest(entry.read_bytes())
            else: raise RuntimeError("special file in fixture root")
            if len(result) > 32: raise RuntimeError("fixture inventory budget exceeded")
    return result


def launch(work, native_args, phase, expect_failure=False):
    runtime_root = Path("/opt/semwright-effect-runtimes")
    executable = Path(native_args[0]).resolve()
    try:
        relative_executable = executable.relative_to(runtime_root)
    except ValueError as error:
        raise RuntimeError("native executable escaped the pinned runtime root") from error
    sandbox_executable = Path("/plugin/tools/runtimes") / relative_executable
    native_args = [str(sandbox_executable), *native_args[1:]]
    args = ["bwrap", "--die-with-parent", "--new-session", "--unshare-all", "--clearenv",
            "--cap-drop", "ALL", "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp",
            "--dir", "/home", "--dir", "/home/native", "--dir", "/etc",
            "--dir", "/plugin", "--dir", "/plugin/tools",
            "--ro-bind", str(source), "/src", "--bind", str(work), "/work",
            "--ro-bind", str(runtime_root), "/plugin/tools/runtimes",
            "--setenv", "HOME", "/home/native", "--setenv", "XDG_CACHE_HOME", "/tmp/cache",
            "--setenv", "XDG_CONFIG_HOME", "/tmp/config", "--setenv", "PATH", "/usr/local/bin:/usr/bin:/bin",
            "--setenv", "LANG", "C.UTF-8", "--chdir", "/work"]
    for runtime in ("/usr", "/bin", "/lib", "/lib64"):
        if Path(runtime).exists(): args += ["--ro-bind", runtime, runtime]
    for config in ("/etc/ld.so.cache", "/etc/fonts"):
        if Path(config).exists(): args += ["--ro-bind", config, config]
    args += ["--"] + native_args
    started = time.monotonic_ns()
    proc = subprocess.Popen(args,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,
                            env={"PATH":"/usr/local/bin:/usr/bin:/bin","LANG":"C.UTF-8"})
    try:
        output = proc.communicate(timeout=90)[0]
    except subprocess.TimeoutExpired:
        proc.kill(); proc.communicate(); raise RuntimeError("native observer deadline exceeded")
    if len(output) > 1024 * 1024: raise RuntimeError("native log budget exceeded")
    text = output.decode(errors="replace")
    path = work / (phase + ".json")
    if expect_failure:
        if proc.returncode == 0:
            raise RuntimeError("native fault injection unexpectedly succeeded")
        if path.exists():
            raise RuntimeError("faulted observer produced a measurement receipt")
        return {"fault_log_digest": digest(output)}, f"host-pid:{proc.pid}:started:{started}"
    if proc.returncode or any(marker in text for marker in ("SCRIPT ERROR:", "Parse Error:", "ERROR:")):
        sys.stderr.write(text[-16000:]); raise RuntimeError("native process failed")
    if not path.is_file() or path.stat().st_size > 65536: raise RuntimeError("missing/oversized native measurement")
    result = json.loads(path.read_text())
    if result.get("schema_version") != 1 or result.get("phase") != phase: raise RuntimeError("native measurement version/phase mismatch")
    return result, f"host-pid:{proc.pid}:started:{started}"


def glb_members(path):
    data = path.read_bytes()
    if len(data) < 20 or len(data) > 16*1024*1024: raise RuntimeError("GLB budget/header")
    if struct.unpack_from("<4sII",data) != (b"glTF",2,len(data)): raise RuntimeError("invalid GLB header")
    size,kind = struct.unpack_from("<II",data,12)
    if kind != 0x4E4F534A or size > len(data)-20: raise RuntimeError("invalid GLB JSON chunk")
    doc = json.loads(data[20:20+size])
    nodes=doc.get("nodes",[])
    if len(nodes)>4096: raise RuntimeError("GLB node budget")
    names=[node["name"] for node in nodes]
    if len(names)!=len(set(names)): raise RuntimeError("duplicate native GLB identities")
    return sorted(names)


with tempfile.TemporaryDirectory(prefix="semwright-effects-",dir=os.environ["RUNNER_TEMP"]) as temp:
    work=Path(temp); data=work/"data"; data.mkdir()
    (data/"sentinel.txt").write_text("excluded synthetic canary v1\n")
    if backend == "godot":
        (work/"project.godot").write_text('config_version=5\n[application]\nconfig/name="Effect Conformance Fixture"\n')
        shutil.copytree(source/"integrations/godot/addons/semwright",work/"addons/semwright")
        shutil.copyfile(source/"scripts/effects/godot_probe.gd",work/"probe.gd")
        (data/"scene.tscn").write_text('[gd_scene format=3]\n\n[node name="Root" type="Node3D"]\n')
        (data/"shared.tres").write_text('[gd_resource type="StandardMaterial3D" format=3]\n\n[resource]\nroughness = 0.7\n')
        (data/"idle.tres").write_text('[gd_resource type="Animation" format=3]\n\n[resource]\nresource_name = "idle"\nlength = 1.0\n')
        def args(phase): return [os.environ["GODOT_BIN"],"--headless","--path","/work","--script","res://probe.gd","--",phase]
        persisted=data/"scene.tscn"
    else:
        def args(phase): return [os.environ["BLENDER_BIN"],"--background","--factory-startup","--disable-autoexec","--python-exit-code","1","--python","/src/scripts/effects/blender_probe.py","--",phase,case]
        persisted=data/"scene.blend"
    before=inventory(data)
    written,writer=launch(work,args("write"),"write")
    saved_digest=digest(persisted.read_bytes())
    if case == "external-mutant":
        # Explicit fault injection; changes resources while scene/GLB stays intact.
        (data/"sentinel.txt").write_text("injected excluded-resource violation\n")
        if backend == "godot":
            p=data/"shared.tres"; p.write_text(p.read_text().replace("0.7","0.3"))
            p=data/"idle.tres"; p.write_text(p.read_text().replace("1.0","0.5"))
    if backend == "godot" and case == "observation-mutant":
        text=persisted.read_text(); old="Vector3(1, 2, 3)"
        if old not in text: raise RuntimeError("mutation target absent; no negative evidence")
        persisted.write_text(text.replace(old,"Vector3(9, 2, 3)",1))
    if backend == "blender" and case == "membership-mutant":
        launch(work,args("mutant"),"mutant")
    if case == "readback-fault":
        fault_args = args("read")
        missing = "res://missing_probe.gd" if backend == "godot" else "/src/scripts/effects/missing_probe.py"
        fault_args[fault_args.index("res://probe.gd" if backend == "godot" else "/src/scripts/effects/blender_probe.py")] = missing
        _,reader=launch(work,fault_args,"readback-fault",expect_failure=True)
        after=inventory(data)
        print(json.dumps({"schema_version":1,"backend":backend,"case":case,"runtime":written["runtime"],
            "before_digest":projection(before),"after_digest":projection(after),"values":{},
            "writer_process":writer,"reader_process":reader,"readback_fault":True,
            "isolation":"bubblewrap-unshare-all-clearenv-disposable-root",
            "limits":{"native_timeout_seconds":90,"root_files":32,"file_bytes":16777216},
            "route":"native-product-adapter-not-broker","crash_durability":"NOT_TESTED"},sort_keys=True))
        raise SystemExit(0)
    reopened,reader=launch(work,args("read"),"read")
    after=inventory(data)
    allowed = {"scene.tscn"} if backend == "godot" else {"scene.blend", "scope.glb"}
    excluded_before = {k:v for k,v in before.items() if k not in allowed}
    excluded_after = {k:v for k,v in after.items() if k not in allowed}
    values={"inventory":{"kind":"preservation","before":projection(excluded_before),"after":projection(excluded_after)},"persistence":{"kind":"reopened","writer_process":writer,"reader_process":reader,
        "before_projection":projection(written["projection"]),"after_projection":projection(reopened["projection"]),
        "saved_digest":saved_digest,"reopened_digest":digest(persisted.read_bytes())},
        "sentinel":{"kind":"preservation","before":before["sentinel.txt"],"after":after["sentinel.txt"]}}
    if backend == "godot":
        values.update(position={"kind":"number","value":reopened["projection"]["observed"]["position"][0],"units":"metre"},
            material={"kind":"preservation","before":before["shared.tres"],"after":after["shared.tres"]},
            animation={"kind":"preservation","before":before["idle.tres"],"after":after["idle.tres"]},
            relation={"kind":"relation","target":reopened["projection"]["observed"]["parent_path"],"binding":"parent"})
    else:
        members=glb_members(data/"scope.glb")
        values.update(position={"kind":"number","value":reopened["projection"]["hero"]["location"][0],"units":"metre"},
            membership={"kind":"members","values":members},
            artifact={"kind":"preservation","before":written["export"]["sha256"],"after":after["scope.glb"]})
    print(json.dumps({"schema_version":1,"backend":backend,"case":case,"runtime":reopened["runtime"],
        "before_digest":projection(before),"after_digest":projection(after),"values":values,
        "writer_process":writer,"reader_process":reader,"readback_fault":False,
        "isolation":"bubblewrap-unshare-all-clearenv-disposable-root",
        "limits":{"native_timeout_seconds":90,"root_files":32,"file_bytes":16777216},
        "route":"native-product-adapter-not-broker","crash_durability":"NOT_TESTED"},sort_keys=True))
