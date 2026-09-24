#!/usr/bin/env python3
"""Normalize CycloneDX output for reproducible GitHub SBOM attestations."""
from __future__ import annotations

import argparse
import json
import re
import uuid
from pathlib import Path

TARGET_PROPERTY = "cdx:rustc:sbom:target:triple"
COMMIT_RE = re.compile(r"^[0-9a-f]{40,64}$")
NAME_RE = re.compile(r"^[A-Za-z0-9._-]{1,128}$")


def target_triple(document: dict, name: str) -> str:
    metadata = document.get("metadata") or {}
    component = metadata.get("component") or {}
    if component.get("type") not in {"application", "library"}:
        raise ValueError(f"{name}: missing top-level component")
    for prop in metadata.get("properties") or []:
        if prop.get("name") == TARGET_PROPERTY and prop.get("value"):
            return str(prop["value"])
    raise ValueError(f"{name}: missing Rust target triple")


def deterministic_serial(name: str, source_commit: str, target: str) -> str:
    if not NAME_RE.fullmatch(name):
        raise ValueError("invalid SBOM logical name")
    if not COMMIT_RE.fullmatch(source_commit):
        raise ValueError("source commit must be a lowercase Git object id")
    identity = (
        "https://github.com/seradotcom/semwright/sbom/v1/"
        f"{source_commit}/{target}/{name}"
    )
    return f"urn:uuid:{uuid.uuid5(uuid.NAMESPACE_URL, identity)}"


def normalize(path: Path, name: str, source_commit: str) -> str:
    document = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict):
        raise ValueError(f"{name}: SBOM root must be an object")
    if document.get("bomFormat") != "CycloneDX":
        raise ValueError(f"{name}: not CycloneDX")
    if document.get("specVersion") != "1.5":
        raise ValueError(f"{name}: unexpected spec {document.get('specVersion')}")
    target = target_triple(document, name)
    serial = deterministic_serial(name, source_commit, target)
    document["serialNumber"] = serial
    path.write_text(
        json.dumps(document, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    return serial


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--path", type=Path, required=True)
    parser.add_argument("--name", required=True)
    parser.add_argument("--source-commit", required=True)
    args = parser.parse_args()
    serial = normalize(args.path, args.name, args.source_commit)
    print(f"{args.name}: {serial}")


if __name__ == "__main__":
    main()
