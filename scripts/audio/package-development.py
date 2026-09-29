#!/usr/bin/env python3
"""Build exact development SWDPs from already-built audio binaries and pinned runtimes."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
TEMPLATES = ROOT / "integrations" / "audio" / "manifests"

def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(65536), b""):
            h.update(block)
    return h.hexdigest()

def run(argv):
    result = subprocess.run(argv, cwd=ROOT, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, check=False)
    print(result.stdout)
    if result.returncode:
        raise RuntimeError("command failed: " + " ".join(map(str, argv)))
    return result.stdout

def manifest(template: str, executable: Path, tools: dict[str, Path], output: Path):
    value = json.loads((TEMPLATES / template).read_text())
    value["executable"] = str(executable.resolve())
    value["sha256"] = digest(executable)
    for tool in value["tools"]:
        source = tools[tool["name"]]
        tool["sha256"] = digest(source)
    output.write_text(json.dumps(value, indent=2) + "\n")
    return value

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--semwright", required=True, type=Path)
    ap.add_argument("--faust-driver", required=True, type=Path)
    ap.add_argument("--analysis-driver", required=True, type=Path)
    ap.add_argument("--ardour-driver", required=True, type=Path)
    ap.add_argument("--faust-helper", required=True, type=Path)
    ap.add_argument("--audio-meter", required=True, type=Path)
    ap.add_argument("--ardour-lua", required=True, type=Path)
    ap.add_argument("--ardour-create", required=True, type=Path)
    ap.add_argument("--ardour-export", required=True, type=Path)
    ap.add_argument("--faust-libraries", required=True, type=Path)
    ap.add_argument("--output", required=True, type=Path)
    args = ap.parse_args()
    out = args.output
    out.mkdir(parents=True, exist_ok=False)

    libs = {}
    for path in sorted(args.faust_libraries.glob("*.lib")):
        if path.is_file():
            libs[path.name] = digest(path)
    if "stdfaust.lib" not in libs:
        raise RuntimeError("pinned Faust library root lacks stdfaust.lib")
    faust_runtime = out / "faust-runtime.json"
    faust_runtime.write_text(json.dumps({
        "schema_version": 1,
        "compiler_version": run([str(args.faust_helper), "version"]).strip(),
        "libraries": libs,
    }, indent=2) + "\n")
    # The helper reports JSON; use its strict compiler_version field.
    value = json.loads(faust_runtime.read_text())
    value["compiler_version"] = json.loads(value["compiler_version"])["compiler_version"]
    faust_runtime.write_text(json.dumps(value, indent=2) + "\n")

    ardour_runtime = out / "ardour-runtime.json"
    ardour_runtime.write_text(json.dumps({
        "schema_version": 1, "ardour_version": "8.4.0"
    }, indent=2) + "\n")

    specs = [
        ("faust-audio.manifest.template.json", args.faust_driver,
         {"faust-interpreter": args.faust_helper}, "faust-audio", faust_runtime),
        ("audio-analysis.manifest.template.json", args.analysis_driver,
         {"audio-meter": args.audio_meter}, "audio-analysis", None),
        ("ardour-audio.manifest.template.json", args.ardour_driver,
         {"ardour-lua": args.ardour_lua, "ardour-new-session": args.ardour_create,
          "ardour-export": args.ardour_export}, "ardour-audio", ardour_runtime),
    ]
    receipts = []
    for template, executable, tools, name, runtime_manifest in specs:
        mf = out / f"{name}.manifest.json"
        manifest(template, executable, tools, mf)
        run([str(args.semwright), "--json", "driver", "validate", str(mf)])
        package = out / f"{name}.swdp"
        command = [str(args.semwright), "--json", "driver", "package", "create",
                   str(mf), str(package), "--semwright", ">=0.9.0-dev.1"]
        if runtime_manifest:
            command += ["--companion", f"runtime/semwright-runtime.json={runtime_manifest}"]
        run(command)
        inspected = json.loads(run([
            str(args.semwright), "--json", "driver", "package", "inspect", str(package)
        ]))
        receipts.append({
            "id": name,
            "manifest_sha256": digest(mf),
            "driver_sha256": digest(executable),
            "package_sha256": digest(package),
            "package_bytes": package.stat().st_size,
            "inspect_package_sha256": inspected["package_sha256"],
            "runtime_model": "owner_provisioned_sealed_tools",
        })

    skill = ROOT / "skills" / "semwright-audio-production"
    run([str(args.semwright), "--json", "skill", "validate", str(skill)])
    run([str(args.semwright), "--json", "skill", "inspect", str(skill)])
    bundle = out / "semwright-audio-production.zip"
    run([str(args.semwright), "--json", "skill", "bundle", str(skill), str(bundle)])
    receipts.append({
        "id": "semwright-audio-production",
        "package_sha256": digest(bundle),
        "package_bytes": bundle.stat().st_size,
        "runtime_model": "skill_bundle_non_authoritative",
    })
    (out / "PACKAGES.json").write_text(json.dumps({
        "schema_version": 1,
        "tested_source_sha": run(["git", "rev-parse", "HEAD"]).strip(),
        "packages": receipts,
        "note": "SWDP installs drivers/data companions; sealed executable tools remain explicit owner runtime grants."
    }, indent=2) + "\n")
    return 0

if __name__ == "__main__":
    sys.exit(main())
