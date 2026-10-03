#!/usr/bin/env python3
"""Build/check the deterministic post-revalidation R16 evidence backup."""
from __future__ import annotations

import argparse
import hashlib
import io
import json
from pathlib import Path
import zipfile

ROOT = Path(__file__).resolve().parents[2]
CLOSEOUT = ROOT / "verification/r16-closeout"
DELIVERY = CLOSEOUT / "delivery"
ZIP_NAME = "semwright-r16-closeout-closed.zip"
ZIP_PATH = DELIVERY / ZIP_NAME
SHA_PATH = DELIVERY / f"{ZIP_NAME}.sha256"
DELIVERY_MANIFEST = DELIVERY / "DELIVERY_MANIFEST_CLOSED.json"

REVIEW_TARGET_SHA = "6491c0d838fa066938a494524d69ed507aa0dbe8"
FINAL_SOURCE_SHA = "868446205df36826356483e93c53e5060c46e8aa"
PRODUCT_FIX_SHA = "4ef9a06e486cd8d2e3851c298e244435ecef3232"
MERGED_MAIN_SHA = "9954c1f95f68305f32f153fe5ab302441845b7ed"
PR_HEAD_SHA = "b58544326c7422dd61b3e5d672625a06e7642c64"
PRE_REVALIDATION_SHA = DELIVERY / "semwright-r16-closeout-final.zip.sha256"

CLOSEOUT_FILES = (
    "CLAIMS_EVIDENCE_MATRIX.json",
    "CLOSEOUT_STATUS.json",
    "PR_BRANCH_DISPOSITION.json",
    "R16_EVIDENCE_MANIFEST.json",
    "R16_FINDINGS.json",
    "R16_REVIEW_REPORT.md",
    "README.md",
    "SHA256SUMS",
    "evidence/G_RECEIPT_VERIFICATION.json",
    "evidence/INDEPENDENT_R16_REVALIDATION_2026-10-03.json",
    "evidence/I_INTEGRATED_ACCEPTANCE.json",
    "evidence/R_CURRENT_SOURCE_VALIDATION.json",
    "evidence/R_DIRECT_REVIEW_12_AREAS.json",
    "evidence/R_DOCUMENTARY_SOURCE_CHECKS.json",
    "evidence/R_FROZEN_SOURCE_SMOKE.json",
)
REPOSITORY_FILES = (
    "README.md",
    "docs/installation.md",
    "V1_ENGINEERING_CLOSEOUT.md",
    "POST_V1_BACKLOG.md",
    "release-readiness.json",
    "verification/v1-engineering-closeout.json",
    "verification/v1-engineering-closeout-revalidation.json",
)
TOOL_FILES = (
    ".github/workflows/r16-closeout.yml",
    "scripts/review/validate_r16.py",
    "scripts/review/package_r16_closeout.py",
    "tests/python/test_r16_closeout.py",
)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _read(path: Path) -> bytes:
    if not path.is_file():
        raise FileNotFoundError(path)
    return path.read_bytes()


def _entry_manifest(entries: dict[str, bytes]) -> list[dict[str, object]]:
    return [
        {"path": name, "bytes": len(data), "sha256": sha256(data)}
        for name, data in sorted(entries.items())
    ]


def collect_entries() -> dict[str, bytes]:
    entries: dict[str, bytes] = {}
    for relative in CLOSEOUT_FILES:
        entries[f"closeout/{relative}"] = _read(CLOSEOUT / relative)
    for relative in REPOSITORY_FILES:
        entries[f"repository/{relative}"] = _read(ROOT / relative)
    for relative in TOOL_FILES:
        entries[f"tools/{relative}"] = _read(ROOT / relative)

    entries["REVIEW_TARGET_SHA"] = f"{REVIEW_TARGET_SHA}\n".encode()
    entries["FINAL_SOURCE_SHA"] = f"{FINAL_SOURCE_SHA}\n".encode()
    entries["PRODUCT_FIX_SHA"] = f"{PRODUCT_FIX_SHA}\n".encode()
    entries["MERGED_MAIN_SHA"] = f"{MERGED_MAIN_SHA}\n".encode()

    readme = """# Semwright R16 post-revalidation closed backup

This deterministic evidence backup records the state after separate R16 revalidation and after
maintainer merge of PR #207. It is not a release archive and contains no runtime binaries.

Current status encoded by the repository:
- R16: CLOSED
- R06: OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT
- R18: OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT
- V1_ENGINEERING_CLOSEOUT: COMPLETE
- RELEASE_READINESS: BLOCKED_DEVELOPMENT_SOURCE

The earlier semwright-r16-closeout-final.zip is preserved unchanged as the pre-revalidation snapshot.
Its REVALIDATION_PENDING state is historical, not the current R16 decision.

Run scripts/review/package_r16_closeout.py --check from the matching repository tree to verify that
this ZIP and its delivery manifest are reproducible byte-for-byte.
"""
    entries["PACKAGE_README.md"] = readme.encode()

    package_manifest = {
        "schema_version": 1,
        "kind": "R16_POST_REVALIDATION_CLOSED_EVIDENCE_BACKUP",
        "contains_runtime_binaries": False,
        "release_archive": False,
        "r16": "CLOSED",
        "r06": "OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT",
        "r18": "OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT",
        "v1_engineering_closeout": "COMPLETE",
        "release_readiness": "BLOCKED_DEVELOPMENT_SOURCE",
        "review_target_sha": REVIEW_TARGET_SHA,
        "final_source_sha": FINAL_SOURCE_SHA,
        "product_fix_sha": PRODUCT_FIX_SHA,
        "pr": 207,
        "pr_head_sha": PR_HEAD_SHA,
        "merged_main_sha": MERGED_MAIN_SHA,
        "r16_post_merge_run": 37146328471,
        "v1_distribution_post_merge_run": 37146331051,
        "historical_pre_revalidation_zip_sha256": PRE_REVALIDATION_SHA.read_text().split()[0],
        "files": _entry_manifest(entries),
    }
    entries["PACKAGE_MANIFEST.json"] = (
        json.dumps(package_manifest, indent=2, ensure_ascii=False, sort_keys=True) + "\n"
    ).encode()
    return entries


def build_zip_bytes() -> bytes:
    entries = collect_entries()
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w", compression=zipfile.ZIP_STORED, strict_timestamps=True) as archive:
        for name, data in sorted(entries.items()):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            archive.writestr(info, data)
    return stream.getvalue()


def delivery_manifest(zip_bytes: bytes) -> bytes:
    old_digest = PRE_REVALIDATION_SHA.read_text().split()[0]
    record = {
        "schema_version": 1,
        "kind": "R16_POST_REVALIDATION_CLOSED_BACKUP",
        "contains_runtime_binaries": False,
        "release_published": False,
        "r16_closed": True,
        "r06": "OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT",
        "r18": "OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT",
        "v1_engineering_closeout": "COMPLETE",
        "release_readiness": "BLOCKED_DEVELOPMENT_SOURCE",
        "review_target_sha": REVIEW_TARGET_SHA,
        "final_source_sha": FINAL_SOURCE_SHA,
        "product_fix_sha": PRODUCT_FIX_SHA,
        "pr": 207,
        "pr_head_sha": PR_HEAD_SHA,
        "merge_commit_sha": MERGED_MAIN_SHA,
        "merge_performed_by_role_r": False,
        "r16_post_merge_run": 37146328471,
        "v1_distribution_post_merge_run": 37146331051,
        "historical_pre_revalidation_backup": {
            "path": "verification/r16-closeout/delivery/semwright-r16-closeout-final.zip",
            "sha256": old_digest,
            "preserved_unchanged": True,
        },
        "zip_path": f"verification/r16-closeout/delivery/{ZIP_NAME}",
        "zip_sha256": sha256(zip_bytes),
        "zip_bytes": len(zip_bytes),
        "zip_integrity": "PASS",
        "deterministic_double_build": "PASS",
        "containing_git_commit": "EXTERNAL_IDENTITY_AFTER_GENERATION",
    }
    return (json.dumps(record, indent=2, ensure_ascii=False, sort_keys=True) + "\n").encode()


def expected_outputs() -> tuple[bytes, bytes, bytes]:
    first = build_zip_bytes()
    second = build_zip_bytes()
    if first != second:
        raise RuntimeError("deterministic double build mismatch")
    digest = sha256(first)
    sha_file = f"{digest}  {ZIP_NAME}\n".encode()
    manifest = delivery_manifest(first)
    return first, sha_file, manifest


def write_outputs() -> None:
    zip_bytes, sha_file, manifest = expected_outputs()
    DELIVERY.mkdir(parents=True, exist_ok=True)
    ZIP_PATH.write_bytes(zip_bytes)
    SHA_PATH.write_bytes(sha_file)
    DELIVERY_MANIFEST.write_bytes(manifest)
    print(json.dumps({
        "result": "WROTE",
        "zip": str(ZIP_PATH.relative_to(ROOT)),
        "sha256": sha256(zip_bytes),
        "bytes": len(zip_bytes),
    }, indent=2))


def check_outputs() -> int:
    expected_zip, expected_sha, expected_manifest = expected_outputs()
    errors = []
    for path, expected in (
        (ZIP_PATH, expected_zip),
        (SHA_PATH, expected_sha),
        (DELIVERY_MANIFEST, expected_manifest),
    ):
        if not path.is_file():
            errors.append(f"missing: {path.relative_to(ROOT)}")
        elif path.read_bytes() != expected:
            errors.append(f"drift: {path.relative_to(ROOT)}")

    if ZIP_PATH.is_file():
        with zipfile.ZipFile(ZIP_PATH) as archive:
            names = archive.namelist()
            if any("${" in name for name in names):
                errors.append("archive contains unexpanded shell path")
            manifest = json.loads(archive.read("PACKAGE_MANIFEST.json"))
            for item in manifest["files"]:
                data = archive.read(item["path"])
                if len(data) != item["bytes"] or sha256(data) != item["sha256"]:
                    errors.append(f"package manifest mismatch: {item['path']}")

    print(json.dumps({
        "result": "FAIL" if errors else "PASS",
        "zip": str(ZIP_PATH.relative_to(ROOT)),
        "errors": errors,
    }, indent=2))
    return 1 if errors else 0


def main() -> int:
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--write", action="store_true")
    group.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.write:
        write_outputs()
        return 0
    return check_outputs()


if __name__ == "__main__":
    raise SystemExit(main())
