#!/usr/bin/env python3
"""Build or verify the deterministic Agent-A Composition development bundle.

This is a source/development handoff artifact, not a Semwright release package.
It never installs dependencies, executes drivers, changes policy, or downloads tools.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "packaging/composition-development/manifest.json"
PREFIX = "semwright-composition-development"
GENERATED_METADATA = f"{PREFIX}/DEVELOPMENT_PACKAGE.json"
MAX_ENTRIES = 4096
BLOCKED_PARTS = {
    ".git",
    "target",
    "node_modules",
    "__pycache__",
    "verification",
    "dist",
}


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def source_sha(root: Path, explicit: str | None) -> str:
    value = explicit
    if value is None:
        value = subprocess.check_output(
            ["git", "-C", str(root), "rev-parse", "HEAD"],
            text=True,
        ).strip()
    if len(value) != 40 or any(c not in "0123456789abcdef" for c in value):
        raise ValueError("source SHA must be a full lowercase Git SHA")
    return value


def source_epoch(root: Path, explicit: int | None) -> int:
    if explicit is not None:
        value = explicit
    elif os.environ.get("SOURCE_DATE_EPOCH"):
        raw = os.environ["SOURCE_DATE_EPOCH"]
        if not raw.isascii() or not raw.isdigit():
            raise ValueError("SOURCE_DATE_EPOCH must be an unsigned integer")
        value = int(raw)
    else:
        value = int(
            subprocess.check_output(
                ["git", "-C", str(root), "show", "-s", "--format=%ct", "HEAD"],
                text=True,
            ).strip()
        )
    # ZIP timestamps cannot represent dates before 1980.
    if value < 315532800 or value > 0x7FFF_FFFF:
        raise ValueError("source epoch must be representable and >= 1980-01-01")
    return value


def zip_datetime(epoch: int) -> tuple[int, int, int, int, int, int]:
    moment = dt.datetime.fromtimestamp(epoch, tz=dt.timezone.utc)
    # ZIP stores seconds in two-second increments.
    return (moment.year, moment.month, moment.day, moment.hour, moment.minute, moment.second // 2 * 2)


def safe_relative(value: str) -> PurePosixPath:
    path = PurePosixPath(value)
    if (
        path.is_absolute()
        or not path.parts
        or any(part in {"", ".", ".."} for part in path.parts)
        or any(part in BLOCKED_PARTS for part in path.parts)
        or "\\" in value
        or ":" in value
    ):
        raise ValueError(f"unsafe package source path: {value}")
    return path


def load_manifest() -> dict:
    value = json.loads(MANIFEST.read_text())
    if set(value) != {
        "schema_version",
        "name",
        "scope",
        "audio_integration",
        "max_file_bytes",
        "max_total_bytes",
        "include",
    }:
        raise ValueError("development package manifest has unknown/missing keys")
    if value["schema_version"] != 1 or value["name"] != PREFIX:
        raise ValueError("development package manifest identity/version mismatch")
    if value["scope"] != "agent-a" or value["audio_integration"] != "pending-b-ready-handoff":
        raise ValueError("Agent-A development package must not claim B integration")
    if not isinstance(value["include"], list) or not 1 <= len(value["include"]) <= 128:
        raise ValueError("development package include list is outside bounds")
    if not isinstance(value["max_file_bytes"], int) or not 1 <= value["max_file_bytes"] <= 64 * 1024 * 1024:
        raise ValueError("invalid development package max_file_bytes")
    if not isinstance(value["max_total_bytes"], int) or not value["max_file_bytes"] <= value["max_total_bytes"] <= 256 * 1024 * 1024:
        raise ValueError("invalid development package max_total_bytes")
    for item in value["include"]:
        if not isinstance(item, str):
            raise ValueError("development package include path must be text")
        safe_relative(item)
    return value


def collect(root: Path, manifest: dict) -> list[tuple[str, Path]]:
    files: dict[str, Path] = {}
    max_file = manifest["max_file_bytes"]
    for requested in manifest["include"]:
        relative = safe_relative(requested)
        source = root.joinpath(*relative.parts)
        if source.is_symlink():
            raise ValueError(f"package input is a symlink: {requested}")
        if not source.exists():
            raise ValueError(f"package input is missing: {requested}")
        candidates = [source] if source.is_file() else sorted(source.rglob("*"))
        for item in candidates:
            rel = item.relative_to(root)
            if any(part in BLOCKED_PARTS for part in rel.parts):
                continue
            rel_posix = rel.as_posix()
            safe_relative(rel_posix)
            metadata = item.lstat()
            if stat.S_ISLNK(metadata.st_mode):
                raise ValueError(f"package tree contains symlink: {rel_posix}")
            if stat.S_ISDIR(metadata.st_mode):
                continue
            if not stat.S_ISREG(metadata.st_mode):
                raise ValueError(f"unsupported package source entry: {rel_posix}")
            if metadata.st_size > max_file:
                raise ValueError(f"package source exceeds per-file limit: {rel_posix}")
            files[rel_posix] = item
            if len(files) > MAX_ENTRIES:
                raise ValueError("development package entry budget exceeded")
    ordered = sorted(files.items())
    total = sum(path.stat().st_size for _, path in ordered)
    if total > manifest["max_total_bytes"]:
        raise ValueError("development package total source bytes exceed budget")
    return ordered


def zip_info(name: str, epoch: int, mode: int = 0o644) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(name, date_time=zip_datetime(epoch))
    info.compress_type = zipfile.ZIP_DEFLATED
    info.external_attr = (stat.S_IFREG | mode) << 16
    info.create_system = 3
    return info


def build(output: Path, root: Path, explicit_sha: str | None, explicit_epoch: int | None) -> dict:
    root = root.resolve()
    manifest = load_manifest()
    sha = source_sha(root, explicit_sha)
    epoch = source_epoch(root, explicit_epoch)
    files = collect(root, manifest)
    entries = []
    for relative, path in files:
        data = path.read_bytes()
        entries.append(
            {
                "path": relative,
                "sha256": sha256_bytes(data),
                "bytes": len(data),
            }
        )
    metadata = {
        "schema_version": 1,
        "package": PREFIX,
        "scope": manifest["scope"],
        "audio_integration": manifest["audio_integration"],
        "source_sha": sha,
        "source_date_epoch": epoch,
        "cargo_lock_sha256": sha256_file(root / "Cargo.lock"),
        "runtime_lock_sha256": sha256_file(
            root / "integrations/motion-canvas/runtime/package-lock.json"
        ),
        "manifest_sha256": sha256_file(MANIFEST),
        "entries": entries,
        "limitations": [
            "source/development bundle only; no executable authority or release admission",
            "audio implementation is intentionally absent until AUDIO_READY_FOR_INTEGRATION",
            "runtime.json is generated only after pinned native tools are installed in CI",
        ],
    }
    metadata_bytes = (json.dumps(metadata, indent=2, sort_keys=True) + "\n").encode()
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists() or output.is_symlink():
        output.unlink()
    with zipfile.ZipFile(output, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        archive.writestr(zip_info(GENERATED_METADATA, epoch), metadata_bytes)
        for relative, path in files:
            archive.writestr(
                zip_info(f"{PREFIX}/{relative}", epoch),
                path.read_bytes(),
            )
    digest = sha256_file(output)
    checksum = output.with_suffix(output.suffix + ".sha256")
    checksum.write_text(f"{digest}  {output.name}\n")
    return {
        "zip": str(output),
        "sha256": digest,
        "checksum": str(checksum),
        "entries": len(entries),
        "source_sha": sha,
        "source_date_epoch": epoch,
    }


def validate_member(name: str) -> PurePosixPath:
    path = PurePosixPath(name)
    if path.is_absolute() or not path.parts or path.parts[0] != PREFIX:
        raise ValueError(f"archive member outside package prefix: {name}")
    if any(part in {"", ".", ".."} for part in path.parts):
        raise ValueError(f"unsafe archive member: {name}")
    return path


def verify(archive_path: Path) -> dict:
    seen: set[str] = set()
    with zipfile.ZipFile(archive_path, "r") as archive:
        infos = archive.infolist()
        if not 2 <= len(infos) <= MAX_ENTRIES + 1:
            raise ValueError("development package archive entry count outside bounds")
        for info in infos:
            validate_member(info.filename)
            if info.filename in seen:
                raise ValueError("duplicate archive member")
            seen.add(info.filename)
            if info.is_dir():
                raise ValueError("development package contains unexpected directory member")
            if info.file_size > 64 * 1024 * 1024:
                raise ValueError("archive member exceeds verification byte limit")
        if GENERATED_METADATA not in seen:
            raise ValueError("development package metadata missing")
        metadata = json.loads(archive.read(GENERATED_METADATA))
        if metadata.get("schema_version") != 1 or metadata.get("package") != PREFIX:
            raise ValueError("development package metadata identity mismatch")
        if metadata.get("audio_integration") != "pending-b-ready-handoff":
            raise ValueError("Agent-A package falsely claims audio integration")
        entries = metadata.get("entries")
        if not isinstance(entries, list) or len(entries) + 1 != len(infos):
            raise ValueError("development package metadata entry count mismatch")
        expected = set()
        total = 0
        for entry in entries:
            if set(entry) != {"path", "sha256", "bytes"}:
                raise ValueError("development package entry metadata malformed")
            relative = safe_relative(entry["path"])
            name = f"{PREFIX}/{relative.as_posix()}"
            expected.add(name)
            data = archive.read(name)
            total += len(data)
            if (
                len(data) != entry["bytes"]
                or sha256_bytes(data) != entry["sha256"]
            ):
                raise ValueError(f"development package entry digest mismatch: {name}")
        if expected | {GENERATED_METADATA} != seen:
            raise ValueError("development package contains unmanifested members")
    return {
        "valid": True,
        "archive": str(archive_path),
        "sha256": sha256_file(archive_path),
        "entries": len(seen) - 1,
        "source_sha": metadata["source_sha"],
        "source_bytes": total,
        "audio_integration": metadata["audio_integration"],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    build_parser = sub.add_parser("build")
    build_parser.add_argument("--output", type=Path, required=True)
    build_parser.add_argument("--root", type=Path, default=ROOT)
    build_parser.add_argument("--source-sha")
    build_parser.add_argument("--epoch", type=int)
    verify_parser = sub.add_parser("verify")
    verify_parser.add_argument("archive", type=Path)
    args = parser.parse_args()
    if args.command == "build":
        result = build(args.output.resolve(), args.root, args.source_sha, args.epoch)
    else:
        result = verify(args.archive.resolve())
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
