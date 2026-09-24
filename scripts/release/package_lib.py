#!/usr/bin/env python3
"""Deterministic package builder shared by release admission and pre-release certification."""
from __future__ import annotations

import gzip
import hashlib
import os
import stat
import subprocess
import tarfile
import tomllib
from pathlib import Path

BINS = ("semwright", "semwrightd", "semwright-mcp", "semwright-inspect", "semwright-sandbox")
MACHINES = {"x86_64": 62, "aarch64": 183}
DEB_ARCH = {"x86_64": "amd64", "aarch64": "arm64"}
MAX_BINARY = 256 * 1024 * 1024


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def source_date_epoch(root: Path) -> int:
    raw = os.environ.get("SOURCE_DATE_EPOCH")
    if raw is None:
        try:
            raw = subprocess.check_output(
                ["git", "-C", str(root), "show", "-s", "--format=%ct", "HEAD"],
                text=True,
                stderr=subprocess.DEVNULL,
            ).strip()
        except (OSError, subprocess.CalledProcessError):
            raw = None
    if raw is None or not raw.isascii() or not raw.isdigit():
        raise ValueError("SOURCE_DATE_EPOCH must be set when Git commit time is unavailable")
    epoch = int(raw)
    if epoch < 0 or epoch > 0x7FFF_FFFF_FFFF_FFFF:
        raise ValueError("SOURCE_DATE_EPOCH is outside the supported range")
    return epoch


def _validate_elf(path: Path, arch: str) -> bytes:
    metadata = path.lstat()
    if (
        not stat.S_ISREG(metadata.st_mode)
        or metadata.st_mode & 0o022
        or not metadata.st_mode & 0o111
        or metadata.st_size <= 20
        or metadata.st_size > MAX_BINARY
    ):
        raise ValueError(f"Expected bounded non-writable executable: {path}")
    body = path.read_bytes()
    machine = int.from_bytes(body[18:20], "little")
    if body[:4] != b"\x7fELF" or machine != MACHINES[arch]:
        raise ValueError(f"ELF architecture mismatch: {path}")
    return body


def _write_regular(path: Path, body: bytes, mode: int, epoch: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as stream:
        stream.write(body)
    path.chmod(mode)
    os.utime(path, (epoch, epoch), follow_symlinks=False)


def _copy_regular(source: Path, destination: Path, mode: int, epoch: int) -> None:
    metadata = source.lstat()
    if not stat.S_ISREG(metadata.st_mode):
        raise ValueError(f"Package source must be a regular file: {source}")
    _write_regular(destination, source.read_bytes(), mode, epoch)


def _copy_tree(source: Path, destination: Path, epoch: int) -> None:
    if source.is_symlink() or not source.is_dir():
        raise ValueError(f"Package source tree is invalid: {source}")
    destination.mkdir(parents=True)
    destination.chmod(0o755)
    for item in sorted(source.rglob("*"), key=lambda p: p.relative_to(source).as_posix()):
        relative = item.relative_to(source)
        target = destination / relative
        metadata = item.lstat()
        if stat.S_ISLNK(metadata.st_mode):
            raise ValueError(f"Package source tree contains a symlink: {item}")
        if stat.S_ISDIR(metadata.st_mode):
            target.mkdir()
            target.chmod(0o755)
            os.utime(target, (epoch, epoch), follow_symlinks=False)
        elif stat.S_ISREG(metadata.st_mode):
            mode = 0o755 if metadata.st_mode & 0o111 else 0o644
            _copy_regular(item, target, mode, epoch)
        else:
            raise ValueError(f"Unsupported package source entry: {item}")
    os.utime(destination, (epoch, epoch), follow_symlinks=False)


def _tar_info(archive: tarfile.TarFile, source: Path, arcname: str, epoch: int) -> tarfile.TarInfo:
    info = archive.gettarinfo(str(source), arcname)
    info.uid = 0
    info.gid = 0
    info.uname = "root"
    info.gname = "root"
    info.mtime = epoch
    if info.isdir():
        info.mode = 0o755
    elif info.isfile():
        info.mode = 0o755 if source.stat().st_mode & 0o111 else 0o644
    else:
        raise ValueError(f"Unsupported archive entry: {source}")
    return info


def _write_tar(stage: Path, destination: Path, epoch: int) -> None:
    with destination.open("xb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=epoch) as zipped:
            with tarfile.open(fileobj=zipped, mode="w", format=tarfile.PAX_FORMAT) as archive:
                info = _tar_info(archive, stage, stage.name, epoch)
                archive.addfile(info)
                for source in sorted(stage.rglob("*"), key=lambda p: p.relative_to(stage).as_posix()):
                    arcname = f"{stage.name}/{source.relative_to(stage).as_posix()}"
                    info = _tar_info(archive, source, arcname, epoch)
                    if info.isfile():
                        with source.open("rb") as stream:
                            archive.addfile(info, stream)
                    else:
                        archive.addfile(info)


def _normalize_tree_times(root: Path, epoch: int) -> None:
    for path in sorted(root.rglob("*"), key=lambda p: len(p.parts), reverse=True):
        if path.is_symlink():
            raise ValueError(f"Package staging contains a symlink: {path}")
        if path.is_dir():
            path.chmod(0o755)
        os.utime(path, (epoch, epoch), follow_symlinks=False)
    root.chmod(0o755)
    os.utime(root, (epoch, epoch), follow_symlinks=False)


def build_packages(
    root: Path,
    bin_dir: Path,
    arch: str,
    output: Path,
    *,
    deb: bool,
    epoch: int | None = None,
) -> dict[str, str]:
    if arch not in MACHINES:
        raise ValueError("Unsupported package architecture")
    root = root.resolve()
    bin_dir = bin_dir.resolve()
    output = output.resolve()
    epoch = source_date_epoch(root) if epoch is None else epoch
    if epoch < 0:
        raise ValueError("Negative SOURCE_DATE_EPOCH is unsupported")

    version = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    output.mkdir(parents=True, exist_ok=True)

    expected_names = {
        f"semwright-{version}-{arch}.tar.gz",
        *({f"semwright_{version}_{DEB_ARCH[arch]}.deb"} if deb else set()),
    }
    for name in expected_names | {"SHA256SUMS"}:
        path = output / name
        if path.exists() or path.is_symlink():
            path.unlink()

    with __import__("tempfile").TemporaryDirectory(prefix="semwright-package-") as temp:
        temp_root = Path(temp)
        stage = temp_root / f"semwright-{version}-{arch}"
        (stage / "bin").mkdir(parents=True)
        (stage / "bin").chmod(0o755)
        checks: dict[str, str] = {}
        for name in BINS:
            source = bin_dir / name
            body = _validate_elf(source, arch)
            _write_regular(stage / "bin" / name, body, 0o755, epoch)
            checks[name] = hashlib.sha256(body).hexdigest()

        for name in ("README.md", "VERIFY.md", "LICENSE-MIT", "LICENSE-APACHE", "SECURITY.md"):
            _copy_regular(root / name, stage / name, 0o644, epoch)
        _copy_tree(root / "packaging", stage / "packaging", epoch)
        _copy_tree(root / "config", stage / "config", epoch)
        sums = "".join(f"{digest}  bin/{name}\n" for name, digest in checks.items())
        _write_regular(stage / "SHA256SUMS", sums.encode(), 0o644, epoch)
        _normalize_tree_times(stage, epoch)

        tar_path = output / f"{stage.name}.tar.gz"
        _write_tar(stage, tar_path, epoch)

        if deb:
            debroot = temp_root / "deb"
            (debroot / "DEBIAN").mkdir(parents=True)
            (debroot / "DEBIAN").chmod(0o755)
            (debroot / "usr/bin").mkdir(parents=True)
            (debroot / "usr/bin").chmod(0o755)
            docs = debroot / "usr/share/doc/semwright"
            docs.mkdir(parents=True)
            docs.chmod(0o755)
            for name in BINS:
                _copy_regular(stage / "bin" / name, debroot / "usr/bin" / name, 0o755, epoch)
            for name in ("LICENSE-MIT", "LICENSE-APACHE", "README.md"):
                _copy_regular(root / name, docs / name, 0o644, epoch)
            control = (
                "Package: semwright\n"
                f"Version: {version.replace('-', '~')}\n"
                f"Architecture: {DEB_ARCH[arch]}\n"
                "Maintainer: Semwright contributors\n"
                "Depends: libc6\n"
                "Description: Policy-scoped semantic automation runtime\n"
                " No service is enabled by this package. Optional desktop dependencies are documented.\n"
            )
            _write_regular(debroot / "DEBIAN/control", control.encode(), 0o644, epoch)
            _normalize_tree_times(debroot, epoch)
            environment = os.environ.copy()
            environment["SOURCE_DATE_EPOCH"] = str(epoch)
            subprocess.run(
                [
                    "dpkg-deb",
                    "--root-owner-group",
                    "--build",
                    str(debroot),
                    str(output / f"semwright_{version}_{DEB_ARCH[arch]}.deb"),
                ],
                check=True,
                env=environment,
            )

    artifacts = {
        path.name: sha256(path)
        for path in sorted(output.iterdir())
        if path.name in expected_names
    }
    (output / "SHA256SUMS").write_text(
        "".join(f"{digest}  {name}\n" for name, digest in sorted(artifacts.items()))
    )
    (output / "SHA256SUMS").chmod(0o644)
    os.utime(output / "SHA256SUMS", (epoch, epoch), follow_symlinks=False)
    return artifacts
