#!/usr/bin/env python3
"""Build exact development SWDPs from already-built audio binaries and pinned runtimes."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import zipfile

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

SOURCE_BUNDLE_PATHS = [
    Path(".github/workflows/audio-diagnostics.yml"),
    Path("Cargo.toml"),
    Path("Cargo.lock"),
    Path("LICENSE-MIT"),
    Path("LICENSE-APACHE"),
    Path("NOTICE"),
    Path("rust-toolchain.toml"),
    Path("crates/audio-authoring"),
    Path("crates/audio-domain"),
    Path("crates/driver-ardour-audio"),
    Path("crates/driver-faust-audio"),
    Path("docs/audio"),
    Path("docs/audio-domain.md"),
    Path("fixtures/audio"),
    Path("integrations/audio"),
    Path("scripts/audio"),
    Path("skills/semwright-audio-production"),
    Path("fuzz/Cargo.toml"),
    Path("fuzz/fuzz_targets/audio_domain_edit.rs"),
    Path("fuzz/fuzz_targets/audio_domain_model.rs"),
    Path("fuzz/fuzz_targets/audio_wav.rs"),
    Path("fuzz/corpus/audio_domain_edit"),
    Path("fuzz/corpus/audio_domain_model"),
    Path("fuzz/corpus/audio_wav"),
]

def source_files():
    files = set()
    for item in SOURCE_BUNDLE_PATHS:
        path = ROOT / item
        if not path.exists():
            raise RuntimeError(f"source bundle input is missing: {item}")
        candidates = [path] if path.is_file() else path.rglob("*")
        for candidate in candidates:
            if candidate.is_symlink():
                raise RuntimeError(f"source bundle rejects symlink: {candidate}")
            if candidate.is_file():
                relative = candidate.relative_to(ROOT)
                if any(part in {".git", "target", "__pycache__"} for part in relative.parts):
                    continue
                files.add(relative)
    return sorted(files)

def build_source_bundle(output: Path, tested_sha: str):
    entries = []
    files = source_files()
    total = 0
    for relative in files:
        path = ROOT / relative
        size = path.stat().st_size
        if size > 8 * 1024 * 1024:
            raise RuntimeError(f"source bundle file exceeds budget: {relative}")
        total += size
        if total > 64 * 1024 * 1024:
            raise RuntimeError("source bundle exceeds aggregate budget")
        entries.append({
            "path": relative.as_posix(),
            "bytes": size,
            "sha256": digest(path),
        })
    manifest_bytes = (json.dumps({
        "schema_version": 1,
        "tested_source_sha": tested_sha,
        "file_count": len(entries),
        "source_bytes": total,
        "files": entries,
    }, sort_keys=True, indent=2) + "\n").encode()
    with zipfile.ZipFile(output, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for relative in files:
            path = ROOT / relative
            info = zipfile.ZipInfo(relative.as_posix(), (1980, 1, 1, 0, 0, 0))
            info.external_attr = (0o755 if path.stat().st_mode & 0o111 else 0o644) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, path.read_bytes())
        info = zipfile.ZipInfo("SOURCE_BUNDLE_MANIFEST.json", (1980, 1, 1, 0, 0, 0))
        info.external_attr = 0o644 << 16
        info.compress_type = zipfile.ZIP_DEFLATED
        archive.writestr(info, manifest_bytes)
    return {
        "id": "semwright-audio-source",
        "package_sha256": digest(output),
        "package_bytes": output.stat().st_size,
        "source_file_count": len(entries),
        "runtime_model": "deterministic_source_backup_non_authoritative",
    }

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
    total_library_bytes = 0
    for path in sorted(args.faust_libraries.rglob("*.lib")):
        if not path.is_file():
            continue
        relative = path.relative_to(args.faust_libraries).as_posix()
        if len(relative) > 512 or len(Path(relative).parts) > 33:
            raise RuntimeError(f"Faust library path exceeds runtime bounds: {relative}")
        total_library_bytes += path.stat().st_size
        if total_library_bytes > 256 * 1024 * 1024:
            raise RuntimeError("Faust library tree exceeds runtime size budget")
        libs[relative] = digest(path)
        if len(libs) > 2048:
            raise RuntimeError("Faust library tree exceeds runtime file budget")
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
        "schema_version": 1,
        "ardour_version": "8.4.0",
        "allowed_plugins": [{
            "id": "ace-inline-scope",
            "native_name": "ACE Inline Scope",
            "kind": "lua",
            "preset": "",
            "unique_id": None,
        }],
    }, indent=2) + "\n")

    specs = [
        ("faust-audio.manifest.template.json", args.faust_driver,
         {"faust-interpreter": args.faust_helper}, "faust-audio", faust_runtime),
        ("audio-analysis.manifest.template.json", args.analysis_driver,
         {"audio-meter": args.audio_meter}, "audio-analysis", None),
        ("ardour-audio.manifest.template.json", args.ardour_driver,
         {"ardour-runtime-runner": args.ardour_driver.with_name("semwright-ardour-runtime-runner"),
          "ardour-lua": args.ardour_lua, "ardour-new-session": args.ardour_create,
          "ardour-export": args.ardour_export}, "ardour-audio", ardour_runtime),
    ]
    tested_sha = run(["git", "rev-parse", "HEAD"]).strip()
    receipts = []
    index_entries = []
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
        metadata = inspected["metadata"]
        packaged_manifest = metadata["manifest"]
        index_entries.append({
            "id": packaged_manifest["id"],
            "version": packaged_manifest["version"],
            "publisher": packaged_manifest["publisher"],
            "package": package.name,
            "package_sha256": inspected["package_sha256"],
            "package_bytes": package.stat().st_size,
            "semwright": metadata["semwright"],
            "application_versions": packaged_manifest["application"]["supported_versions"],
        })
        receipts.append({
            "id": name,
            "manifest_sha256": digest(mf),
            "driver_sha256": digest(executable),
            "package_sha256": digest(package),
            "package_bytes": package.stat().st_size,
            "inspect_package_sha256": inspected["package_sha256"],
            "runtime_model": "owner_provisioned_sealed_tools",
            "install_verified": False,
        })

    index = out / "index.json"
    index.write_text(json.dumps({
        "index_version": 1,
        "drivers": index_entries,
    }, indent=2) + "\n")
    run([str(args.semwright), "--json", "driver", "index", "validate", str(index)])

    install_data = out / "install-data"
    install_config = out / "install-config"
    install_data.mkdir(mode=0o700)
    install_config.mkdir(mode=0o700)
    for entry in index_entries:
        common = [
            "driver", "install", str(index), entry["id"],
            "--version", entry["version"],
            "--data-dir", str(install_data),
            "--config-dir", str(install_config),
        ]
        if entry["application_versions"]:
            # Package-time Faust is the Noble 2.70.3 baseline; Ardour has one exact version.
            # Select the newest declared exact application version rather than an older compatible one.
            selected_application_version = entry["application_versions"][-1]
            common += ["--application-version", selected_application_version]
        else:
            selected_application_version = None
        dry = json.loads(run([str(args.semwright), "--json", "--dry-run", *common]))
        if dry.get("installed") is not False or dry.get("policy_grants_changed") is not False:
            raise RuntimeError(f"driver dry-run install changed authority for {entry['id']}")
        installed = json.loads(run([str(args.semwright), "--json", *common]))
        if installed.get("installed") is not True or installed.get("policy_grants_changed") is not False:
            raise RuntimeError(f"driver install receipt is invalid for {entry['id']}")
        receipt = installed["receipt"]
        executable_path = Path(receipt["executable_path"])
        manifest_path = Path(receipt["manifest_path"])
        if not executable_path.is_file() or not manifest_path.is_file():
            raise RuntimeError(f"installed driver files are missing for {entry['id']}")
        if digest(executable_path) != receipt["executable_sha256"]:
            raise RuntimeError(f"installed executable digest mismatch for {entry['id']}")
        if receipt["package_sha256"] != entry["package_sha256"]:
            raise RuntimeError(f"installed package digest mismatch for {entry['id']}")
        for row in receipts:
            if row["id"] == entry["id"]:
                row["install_verified"] = True
                row["install_application_version"] = selected_application_version
                row["install_receipt_sha256"] = hashlib.sha256(
                    json.dumps(receipt, sort_keys=True, separators=(",", ":")).encode()
                ).hexdigest()
                break
        run([
            str(args.semwright), "--json", "driver", "remove",
            entry["id"], entry["version"],
            "--data-dir", str(install_data),
            "--config-dir", str(install_config),
        ])
        if executable_path.exists() or manifest_path.exists():
            raise RuntimeError(f"driver removal did not clean installed files for {entry['id']}")

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
    source_bundle = out / "semwright-audio-source.zip"
    receipts.append(build_source_bundle(source_bundle, tested_sha))
    (out / "PACKAGES.json").write_text(json.dumps({
        "schema_version": 1,
        "tested_source_sha": tested_sha,
        "packages": receipts,
        "note": "SWDP installs drivers/data companions; sealed executable tools remain explicit owner runtime grants."
    }, indent=2) + "\n")
    return 0

if __name__ == "__main__":
    sys.exit(main())
