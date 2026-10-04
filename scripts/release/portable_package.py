#!/usr/bin/env python3
"""Build and certify deterministic Semwright portable archives for Windows/macOS."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import shutil
import struct
import subprocess
import tarfile
import tempfile
import time
import tomllib
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
from bundle_contract import BINS, add_bundle_files, write_checksums, verify_checksums  # noqa: E402,F401
PE_MACHINE = {"x86_64": 0x8664, "arm64": 0xAA64}
MACHO_CPU = {"x86_64": 0x01000007, "arm64": 0x0100000C}
MAX_BINARY = 256 * 1024 * 1024


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def source_date_epoch() -> int:
    raw = os.environ.get("SOURCE_DATE_EPOCH")
    if raw is None:
        raw = subprocess.check_output(
            ["git", "-C", str(ROOT), "show", "-s", "--format=%ct", "HEAD"], text=True
        ).strip()
    if not raw.isascii() or not raw.isdigit():
        raise ValueError("SOURCE_DATE_EPOCH must be an integer")
    return int(raw)


def version() -> str:
    return tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]


def binary_name(name: str, platform: str) -> str:
    return name + ".exe" if platform == "windows" else name


def validate_binary(path: Path, platform: str, arch: str) -> None:
    st = path.stat()
    if not path.is_file() or st.st_size <= 64 or st.st_size > MAX_BINARY:
        raise ValueError(f"invalid executable: {path}")
    body = path.read_bytes()[:4096]
    if platform == "windows":
        if body[:2] != b"MZ" or len(body) < 0x40:
            raise ValueError(f"expected PE executable: {path}")
        pe_offset = struct.unpack_from("<I", body, 0x3C)[0]
        if pe_offset + 6 > len(body):
            body = path.read_bytes()[: pe_offset + 64]
        if body[pe_offset : pe_offset + 4] != b"PE\0\0":
            raise ValueError(f"invalid PE signature: {path}")
        machine = struct.unpack_from("<H", body, pe_offset + 4)[0]
        if machine != PE_MACHINE[arch]:
            raise ValueError(f"PE architecture mismatch for {path}: {machine:#x}")
    elif platform == "macos":
        if body[:4] != b"\xcf\xfa\xed\xfe":
            raise ValueError(f"expected little-endian 64-bit Mach-O: {path}")
        cpu = struct.unpack_from("<I", body, 4)[0]
        if cpu != MACHO_CPU[arch]:
            raise ValueError(f"Mach-O architecture mismatch for {path}: {cpu:#x}")
    else:
        raise ValueError("portable packager supports only windows/macos")


def parse_macos_rpath_dependencies(output: str) -> tuple[str, ...]:
    dependencies: set[str] = set()
    for line in output.splitlines()[1:]:
        token = line.strip().split(" ", 1)[0]
        if token.startswith("@rpath/"):
            relative = token.removeprefix("@rpath/")
            path = Path(relative)
            if not relative or path.is_absolute() or ".." in path.parts:
                raise ValueError(f"unsafe @rpath dependency: {token}")
            dependencies.add(relative)
    return tuple(sorted(dependencies))


def macos_rpath_dependencies(binary: Path) -> tuple[str, ...]:
    output = subprocess.check_output(["otool", "-L", str(binary)], text=True)
    return parse_macos_rpath_dependencies(output)


def find_macos_runtime_library(bin_dir: Path, relative: str, arch: str) -> Path:
    name = Path(relative).name
    candidates = []
    direct = bin_dir / name
    if direct.is_file():
        candidates.append(direct)
    candidates.extend(sorted(bin_dir.glob(f"build/*/out/{name}")))
    valid = []
    for candidate in candidates:
        try:
            validate_binary(candidate, "macos", arch)
        except (OSError, ValueError):
            continue
        valid.append(candidate)
    if not valid:
        raise RuntimeError(f"missing packaged macOS runtime dependency: {relative}")
    digests = {sha256(candidate) for candidate in valid}
    if len(digests) != 1:
        raise RuntimeError(f"ambiguous macOS runtime dependency: {relative}")
    return sorted(valid, key=lambda p: p.as_posix())[0]


def copy_macos_runtime_libraries(stage: Path, bin_dir: Path, arch: str, epoch: int) -> tuple[str, ...]:
    dependencies: set[str] = set()
    for name in BINS:
        dependencies.update(macos_rpath_dependencies(bin_dir / binary_name(name, "macos")))
    if not dependencies:
        return ()
    frameworks = stage / "Frameworks"
    frameworks.mkdir(parents=True, exist_ok=True)
    copied = []
    for relative in sorted(dependencies):
        source = find_macos_runtime_library(bin_dir, relative, arch)
        destination = frameworks / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
        destination.chmod(0o755)
        os.utime(destination, (epoch, epoch))
        copied.append(relative)
    return tuple(copied)


def platform_notes(platform: str, arch: str) -> str:
    if platform == "windows":
        return f"""# Windows portable package

Architecture: {arch}

This archive is a per-user portable build. It is not MSI/MSIX and is not Authenticode-signed.
Windows SmartScreen or reputation warnings may appear. Do not represent this archive as signed.

`semwright-sandbox.exe` is intentionally a fail-closed compatibility sentinel on Windows.
Valid child isolation is enforced by the native secure-spawn/AppContainer host contracts.

Hosted native CI is not unlocked-desktop certification. R18 remains
`OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT` until the interactive matrix is executed.
"""
    return f"""# macOS portable archive

Architecture: {arch}

This is an unsigned, unnotarized command-line archive. It is not equivalent to a notarized app,
installer, LaunchAgent deployment, or TCC-certified desktop experience.

`semwright-sandbox` is intentionally a fail-closed compatibility sentinel on macOS.
Valid platform isolation remains in the native host contracts; no arbitrary-child sandbox is claimed.

Codesign/notarization, installed-service behavior and live TCC/multi-display acceptance remain outside
this portable package certification.
"""


def copy_docs(stage: Path, platform: str, arch: str, epoch: int) -> None:
    add_bundle_files(ROOT, stage, platform, epoch)
    notes = stage / "PLATFORM-NOTES.md"
    notes.write_text(platform_notes(platform, arch))
    os.utime(notes, (epoch, epoch))


def normalize_tree(stage: Path, epoch: int) -> None:
    for path in sorted(stage.rglob("*"), key=lambda p: len(p.parts), reverse=True):
        if path.is_symlink():
            raise ValueError(f"portable package cannot contain symlinks: {path}")
        os.utime(path, (epoch, epoch))
        if path.is_dir():
            path.chmod(0o755)
        else:
            executable_parent = path.parent.name in {"bin", "Frameworks"} or path.suffix == ".sh"
            path.chmod(0o755 if executable_parent else 0o644)
    stage.chmod(0o755)
    os.utime(stage, (epoch, epoch))


def write_internal_checksums(stage: Path, epoch: int) -> None:
    write_checksums(stage, epoch)


def zip_datetime(epoch: int) -> tuple[int, int, int, int, int, int]:
    tm = time.gmtime(max(epoch, 315532800))
    return tm[:6]


def write_zip(stage: Path, destination: Path, epoch: int) -> None:
    root = stage.name
    with zipfile.ZipFile(destination, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for path in [stage, *sorted(stage.rglob("*"), key=lambda p: p.relative_to(stage).as_posix())]:
            rel = "" if path == stage else path.relative_to(stage).as_posix()
            name = f"{root}/" + rel
            if path.is_dir() and not name.endswith("/"):
                name += "/"
            info = zipfile.ZipInfo(name, date_time=zip_datetime(epoch))
            info.create_system = 3
            info.external_attr = ((0o755 if path.is_dir() or path.parent.name == "bin" else 0o644) & 0xFFFF) << 16
            if path.is_dir():
                zf.writestr(info, b"")
            else:
                zf.writestr(info, path.read_bytes(), compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)


def tar_info(archive: tarfile.TarFile, path: Path, arcname: str, epoch: int) -> tarfile.TarInfo:
    info = archive.gettarinfo(str(path), arcname)
    info.uid = info.gid = 0
    info.uname = info.gname = "root"
    info.mtime = epoch
    info.mode = 0o755 if path.is_dir() or path.parent.name in {"bin", "Frameworks"} or path.suffix == ".sh" else 0o644
    return info


def write_tar(stage: Path, destination: Path, epoch: int) -> None:
    with destination.open("xb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=epoch) as gz:
            with tarfile.open(fileobj=gz, mode="w", format=tarfile.PAX_FORMAT) as tf:
                for path in [stage, *sorted(stage.rglob("*"), key=lambda p: p.relative_to(stage).as_posix())]:
                    arc = stage.name if path == stage else f"{stage.name}/{path.relative_to(stage).as_posix()}"
                    info = tar_info(tf, path, arc, epoch)
                    if path.is_file():
                        with path.open("rb") as stream:
                            tf.addfile(info, stream)
                    else:
                        tf.addfile(info)


def build_once(bin_dir: Path, platform: str, arch: str, output: Path, epoch: int) -> Path:
    ver = version()
    archive_name = (
        f"semwright-{ver}-windows-{arch}.zip"
        if platform == "windows"
        else f"semwright-{ver}-macos-{arch}.tar.gz"
    )
    output.mkdir(parents=True, exist_ok=True)
    destination = output / archive_name
    if destination.exists():
        destination.unlink()
    with tempfile.TemporaryDirectory(prefix="semwright-portable-") as temp:
        stage = Path(temp) / archive_name.removesuffix(".zip").removesuffix(".tar.gz")
        (stage / "bin").mkdir(parents=True)
        for name in BINS:
            src = bin_dir / binary_name(name, platform)
            validate_binary(src, platform, arch)
            dst = stage / "bin" / src.name
            shutil.copyfile(src, dst)
            dst.chmod(0o755)
            os.utime(dst, (epoch, epoch))
        if platform == "macos":
            copy_macos_runtime_libraries(stage, bin_dir, arch, epoch)
        copy_docs(stage, platform, arch, epoch)
        write_internal_checksums(stage, epoch)
        normalize_tree(stage, epoch)
        if platform == "windows":
            write_zip(stage, destination, epoch)
        else:
            write_tar(stage, destination, epoch)
    return destination


def safe_extract(archive: Path, platform: str, destination: Path) -> Path:
    if platform == "windows":
        with zipfile.ZipFile(archive) as zf:
            names = zf.namelist()
            if not names:
                raise RuntimeError("empty ZIP")
            root = names[0].split("/", 1)[0]
            for name in names:
                p = Path(name)
                if p.is_absolute() or ".." in p.parts or not (name == root + "/" or name.startswith(root + "/")):
                    raise RuntimeError("unsafe ZIP member")
            zf.extractall(destination)
    else:
        with tarfile.open(archive, "r:gz") as tf:
            members = tf.getmembers()
            if not members:
                raise RuntimeError("empty tar")
            root = members[0].name.split("/", 1)[0]
            for member in members:
                p = Path(member.name)
                if p.is_absolute() or ".." in p.parts or not (member.name == root or member.name.startswith(root + "/")):
                    raise RuntimeError("unsafe tar member")
                if member.issym() or member.islnk() or not (member.isfile() or member.isdir()):
                    raise RuntimeError("unsupported tar member")
            tf.extractall(destination)
    return destination / root


def run_help(bin_dir: Path, platform: str) -> dict[str, bool]:
    results = {}
    for name in ("semwright", "semwrightd", "semwright-mcp", "semwright-inspect"):
        exe = bin_dir / binary_name(name, platform)
        result = subprocess.run([str(exe), "--help"], capture_output=True, text=True, timeout=15)
        if result.returncode != 0 or not (result.stdout.strip() or result.stderr.strip()):
            raise RuntimeError(f"{name} --help failed: rc={result.returncode}")
        results[name] = True
    sentinel = bin_dir / binary_name("semwright-sandbox", platform)
    result = subprocess.run([str(sentinel)], capture_output=True, text=True, timeout=15)
    if result.returncode != 5 or "SandboxDenied" not in result.stderr:
        raise RuntimeError("non-Linux semwright-sandbox must fail closed as documented")
    results["semwright-sandbox-fail-closed"] = True
    return results


def write_private_owner_file(path: Path, text: str) -> None:
    path.write_text(text)
    path.chmod(0o600)


def doctor_smoke(bin_dir: Path, platform: str) -> bool:
    with tempfile.TemporaryDirectory(prefix="semwright-portable-doctor-") as temp:
        work = Path(temp)
        policy = work / "policy.toml"
        write_private_owner_file(policy, '[policy]\nprofile="desktop"\n')
        if platform == "windows":
            endpoint = rf"\\.\pipe\semwright-packaging-{os.getpid()}"
        else:
            endpoint = str(work / "broker.sock")
        daemon = bin_dir / binary_name("semwrightd", platform)
        cli = bin_dir / binary_name("semwright", platform)
        proc = subprocess.Popen(
            [str(daemon), "--fake", "--config", str(policy), "--socket", endpoint],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            last = None
            for _ in range(100):
                last = subprocess.run(
                    [str(cli), "--socket", endpoint, "--json", "doctor"],
                    capture_output=True,
                    text=True,
                    timeout=3,
                )
                if last.returncode == 0:
                    payload = json.loads(last.stdout)
                    if payload.get("ok") is True and payload.get("data", {}).get("fake") is True:
                        return True
                if proc.poll() is not None:
                    break
                time.sleep(0.05)
            out, err = proc.communicate(timeout=2)
            raise RuntimeError(
                f"portable fake doctor failed; cli={None if last is None else last.stderr!r}; "
                f"daemon_out={out!r}; daemon_err={err!r}"
            )
        finally:
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait(timeout=5)


def certify(bin_dir: Path, platform: str, arch: str, output: Path, report: Path) -> None:
    epoch = source_date_epoch()
    with tempfile.TemporaryDirectory(prefix="semwright-portable-build-a-") as a, tempfile.TemporaryDirectory(prefix="semwright-portable-build-b-") as b:
        first = build_once(bin_dir, platform, arch, Path(a), epoch)
        second = build_once(bin_dir, platform, arch, Path(b), epoch)
        if first.read_bytes() != second.read_bytes():
            raise RuntimeError("portable package is not reproducible")
        output.mkdir(parents=True, exist_ok=True)
        final = output / first.name
        shutil.copyfile(first, final)
    with tempfile.TemporaryDirectory(prefix="semwright-portable-extract-") as temp:
        stage = safe_extract(final, platform, Path(temp))
        packaged_bin = stage / "bin"
        for name in BINS:
            validate_binary(packaged_bin / binary_name(name, platform), platform, arch)
        internal = stage / "SHA256SUMS"
        if not internal.is_file():
            raise RuntimeError("internal SHA256SUMS missing")
        verify_checksums(stage)
        runtime_libraries = sorted(
            path.relative_to(stage).as_posix()
            for path in (stage / "Frameworks").rglob("*")
            if path.is_file()
        ) if (stage / "Frameworks").is_dir() else []
        for relative in runtime_libraries:
            validate_binary(stage / relative, platform, arch)
        checks = run_help(packaged_bin, platform)
        doctor = doctor_smoke(packaged_bin, platform)
        from certify_install import certify_install
        def installed_smoke(installed_bin):
            run_help(installed_bin, platform)
            doctor_smoke(installed_bin, platform)
        installation = certify_install(stage, platform, installed_smoke)
    manifest = output / "SHA256SUMS"
    manifest.write_text(f"{sha256(final)}  {final.name}\n")
    result = {
        "status": "PASS",
        "release_admission": False,
        "platform": platform,
        "arch": arch,
        "artifact": final.name,
        "artifact_sha256": sha256(final),
        "source_sha": subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip(),
        "source_date_epoch": epoch,
        "format": "PE/ZIP" if platform == "windows" else "Mach-O/tar.gz",
        "reproducible": True,
        "clean_extract": True,
        "internal_checksums_verified": True,
        "user_install": installation,
        "binary_architecture": "PASS",
        "help_contracts": checks,
        "runtime_libraries": runtime_libraries,
        "doctor_fake_smoke": doctor,
        "tui_startup_contract": checks["semwright-inspect"],
        "mcp_binary_contract": checks["semwright-mcp"],
        "signing": (
            "UNSIGNED_PORTABLE; MSI/MSIX/Authenticode/SmartScreen reputation not claimed"
            if platform == "windows"
            else "UNSIGNED_UNNOTARIZED_PORTABLE; notarized app/TCC experience not claimed"
        ),
    }
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps(result, indent=2, sort_keys=True))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--platform", choices=("windows", "macos"), required=True)
    parser.add_argument("--arch", choices=("x86_64", "arm64"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    certify(args.bin_dir.resolve(), args.platform, args.arch, args.output.resolve(), args.report.resolve())


if __name__ == "__main__":
    main()
