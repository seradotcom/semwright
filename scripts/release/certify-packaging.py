#!/usr/bin/env python3
"""Development-only packaging certification. Never grants release admission."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import stat
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(Path(__file__).resolve().parent))
from bundle_contract import BINS, add_bundle_files, write_checksums, verify_checksums  # noqa: E402,F401


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()
def load_packager():
    path = ROOT / "scripts/release/package_lib.py"
    spec = importlib.util.spec_from_file_location("semwright_packager", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("Cannot load package implementation")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def assert_staging_admitted() -> None:
    result = subprocess.run(
        [sys.executable, "-I", "-S", str(ROOT / "scripts/release/assert-ready.py"), "--mode", "staging"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=10,
    )
    if result.returncode != 0 or "NOT PUBLIC RELEASE AUTHORIZATION" not in result.stdout:
        raise RuntimeError(
            "Package certification requires engineering staging admission (not public authorization)"
        )


def safe_tar_members(path: Path, epoch: int) -> list[str]:
    with tarfile.open(path, "r:gz") as archive:
        members = archive.getmembers()
        if not members:
            raise RuntimeError("Empty source tarball")
        root = members[0].name.split("/", 1)[0]
        for member in members:
            candidate = Path(member.name)
            if candidate.is_absolute() or ".." in candidate.parts:
                raise RuntimeError("Unsafe tar member")
            if member.name != root and not member.name.startswith(root + "/"):
                raise RuntimeError("Tarball escaped its single root")
            if member.issym() or member.islnk() or not (member.isfile() or member.isdir()):
                raise RuntimeError("Tarball contains a non-regular entry")
            if member.uid != 0 or member.gid != 0 or member.mtime != epoch:
                raise RuntimeError("Tarball metadata is not normalized")
        return [member.name for member in members]


def user_install_roundtrip(bin_dir: Path, root: Path) -> dict[str, bool]:
    home = root / "home"
    home.mkdir(parents=True, mode=0o700)
    environment = dict(os.environ)
    environment["HOME"] = str(home)
    install = ROOT / "packaging/install/install.py"
    uninstall = ROOT / "packaging/install/uninstall.py"
    subprocess.run(
        [sys.executable, str(install), "--bin-dir", str(bin_dir)],
        check=True,
        env=environment,
    )
    installed = home / ".local/bin"
    for name in BINS:
        path = installed / name
        if not path.is_file() or path.is_symlink():
            raise RuntimeError(f"Installed binary is invalid: {name}")
        if not path.stat().st_mode & stat.S_IXUSR:
            raise RuntimeError(f"Installed binary is not executable: {name}")
    subprocess.run(
        [str(installed / "semwright"), "--help"],
        check=True,
        env=environment,
        stdout=subprocess.DEVNULL,
    )
    subprocess.run(
        [sys.executable, str(uninstall)],
        check=True,
        env=environment,
    )
    if any((installed / name).exists() for name in BINS):
        raise RuntimeError("Uninstall left a managed executable behind")
    return {"install": True, "execute": True, "uninstall": True}


def tamper_refusal(bin_dir: Path, root: Path) -> bool:
    home = root / "tamper-home"
    home.mkdir(parents=True, mode=0o700)
    environment = dict(os.environ)
    environment["HOME"] = str(home)
    install = ROOT / "packaging/install/install.py"
    uninstall = ROOT / "packaging/install/uninstall.py"
    subprocess.run(
        [sys.executable, str(install), "--bin-dir", str(bin_dir)],
        check=True,
        env=environment,
    )
    target = home / ".local/bin/semwright"
    with target.open("ab") as stream:
        stream.write(b"tampered")
    result = subprocess.run(
        [sys.executable, str(uninstall)],
        env=environment,
        capture_output=True,
        text=True,
        timeout=10,
    )
    combined = result.stdout + result.stderr
    if result.returncode == 0 or "File changed; refusing to remove" not in combined:
        raise RuntimeError("Uninstaller did not fail closed on a modified executable")
    if not target.exists():
        raise RuntimeError("Tampered executable was unexpectedly deleted")
    return True

def inspect_deb(path: Path, bin_dir: Path, expected_arch: str) -> dict[str, str]:
    fields = {}
    for field in ("Package", "Version", "Architecture"):
        fields[field] = subprocess.check_output(
            ["dpkg-deb", "--field", str(path), field],
            text=True,
        ).strip()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    expected = {
        "Package": "semwright",
        "Version": version.replace("-", "~"),
        "Architecture": expected_arch,
    }
    if fields != expected:
        raise RuntimeError(f"Unexpected Debian metadata: {fields!r}")
    with tempfile.TemporaryDirectory(prefix="semwright-deb-inspect-") as temp:
        extracted = Path(temp) / "root"
        subprocess.run(["dpkg-deb", "-x", str(path), str(extracted)], check=True)
        for name in BINS:
            packaged = extracted / "usr/bin" / name
            if packaged.is_symlink() or not packaged.is_file():
                raise RuntimeError(f"Debian payload is invalid: {name}")
            if digest(packaged) != digest(bin_dir / name):
                raise RuntimeError(f"Debian payload digest mismatch: {name}")
        quickstart = extracted / "usr/share/doc/semwright/QUICKSTART.md"
        if not quickstart.is_file() or "semwright setup" not in quickstart.read_text():
            raise RuntimeError("Debian payload is missing the bundled quick start")
        subprocess.run(
            [str(extracted / "usr/bin/semwright"), "--help"],
            check=True,
            stdout=subprocess.DEVNULL,
        )
    return fields


def certify(bin_dir: Path, arch: str) -> dict[str, object]:
    packager = load_packager()
    epoch = packager.source_date_epoch(ROOT)
    with tempfile.TemporaryDirectory(prefix="semwright-packaging-cert-") as temp:
        temp_root = Path(temp)
        first = temp_root / "first"
        second = temp_root / "second"
        first_artifacts = packager.build_packages(
            ROOT, bin_dir, arch, first, deb=True, epoch=epoch
        )
        second_artifacts = packager.build_packages(
            ROOT, bin_dir, arch, second, deb=True, epoch=epoch
        )
        if first_artifacts != second_artifacts:
            raise RuntimeError("Repeated package builds are not reproducible")
        if (first / "SHA256SUMS").read_bytes() != (second / "SHA256SUMS").read_bytes():
            raise RuntimeError("Repeated checksum manifests differ")
        version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
        tar_path = first / f"semwright-{version}-{arch}.tar.gz"
        members = safe_tar_members(tar_path, epoch)
        package_root = f"semwright-{version}-{arch}"
        required = {f"{package_root}/bin/{name}" for name in BINS}
        required.update({
            f"{package_root}/README.md", f"{package_root}/INSTALL.md",
            f"{package_root}/docs/quickstart.md",
            f"{package_root}/LICENSE-MIT", f"{package_root}/LICENSE-APACHE",
            f"{package_root}/SECURITY.md", f"{package_root}/SUPPORT.md",
            f"{package_root}/SHA256SUMS",
        })
        if not required.issubset(set(members)):
            raise RuntimeError("Tarball is missing required executables or release documents")
        deb_arch = packager.DEB_ARCH[arch]
        deb_path = first / f"semwright_{version}_{deb_arch}.deb"
        deb_fields = inspect_deb(deb_path, bin_dir, deb_arch)
        # Exercise the helper and bytes shipped inside the archive, not the source bin directory.
        extracted = temp_root / 'extracted'
        extracted.mkdir()
        with tarfile.open(tar_path, 'r:gz') as archive:
            archive.extractall(extracted, filter='data')
        stage = extracted / package_root
        verify_checksums(stage)
        from certify_install import certify_install
        install = certify_install(stage, 'linux')
        legacy_install = user_install_roundtrip(stage / 'bin', temp_root / 'install')
        tamper = tamper_refusal(bin_dir, temp_root / "tamper")
        return {
            "status": "PASS",
            "release_admission": False,
            "platform": "linux",
            "source_sha": subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip(),
            "internal_checksums_verified": True,
            "binary_architecture": "PASS",
            "legacy_user_install": legacy_install,
            "arch": arch,
            "source_date_epoch": epoch,
            "artifacts": first_artifacts,
            "tar_members": len(members),
            "deb": deb_fields,
            "user_install": install,
            "tamper_refusal": tamper,
            "reproducible": True,
        }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--arch", choices=("x86_64", "aarch64"), required=True)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    bin_dir = args.bin_dir.resolve()
    assert_staging_admitted()
    report = certify(bin_dir, args.arch)
    body = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.report is not None:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(body)
    print(body, end="")


if __name__ == "__main__":
    main()
