#!/usr/bin/env python3
"""Regenerate MLT catalog goldens from the single static source catalog."""
from __future__ import annotations
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DRIVER = ROOT / "crates/driver-mlt-video"
SOURCE = DRIVER / "src/catalog.json"
EXPECTED = DRIVER / "fixtures/expected"
SUMS = DRIVER / "fixtures/SHA256SUMS.json"
DESCRIPTOR_FIELDS = [
    "name", "version", "description", "input_schema", "output_schema",
    "requires", "risk", "idempotency", "timeout_ms", "dry_run",
    "interactive_consent", "backends",
]

def compact(value) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))

def ordered_descriptor(descriptor: dict) -> str:
    if set(descriptor) != set(DESCRIPTOR_FIELDS):
        raise SystemExit(f"unexpected descriptor fields for {descriptor.get('name')}")
    return "{" + ",".join(
        json.dumps(key) + ":" + compact(descriptor[key]) for key in DESCRIPTOR_FIELDS
    ) + "}"

def item_wire(item: dict) -> str:
    if set(item) != {"descriptor", "aliases", "tags", "object_types"}:
        raise SystemExit("unexpected catalog item fields")
    descriptor = ordered_descriptor(item["descriptor"])
    return "{" + ",".join([
        json.dumps("descriptor") + ":" + descriptor,
        json.dumps("aliases") + ":" + compact(item["aliases"]),
        json.dumps("tags") + ":" + compact(item["tags"]),
        json.dumps("object_types") + ":" + compact(item["object_types"]),
    ]) + "}"

def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def main() -> None:
    catalog = json.loads(SOURCE.read_text())
    names = [item["descriptor"]["name"] for item in catalog]
    if names != sorted(names) or len(names) != len(set(names)):
        raise SystemExit("MLT catalog must stay uniquely sorted by capability name")
    expected_bytes = (json.dumps(catalog, indent=2, ensure_ascii=False) + "\n").encode()
    SOURCE.write_bytes(expected_bytes)
    (EXPECTED / "capabilities.json").write_bytes(expected_bytes)

    wires = [item_wire(item) for item in catalog]
    wire = ("[" + ",".join(wires) + "]").encode()
    (EXPECTED / "catalog-wire.json").write_bytes(wire)

    descriptors = {
        item["descriptor"]["name"]: sha(ordered_descriptor(item["descriptor"]).encode())
        for item in catalog
    }
    digests = {
        "catalog_sha256": sha(wire),
        "descriptor_sha256": descriptors,
    }
    (EXPECTED / "digests.json").write_text(
        json.dumps(digests, indent=2, sort_keys=False) + "\n"
    )

    sums = json.loads(SUMS.read_text())
    for relative in [
        "expected/capabilities.json",
        "expected/catalog-wire.json",
        "expected/digests.json",
    ]:
        sums[relative] = sha((DRIVER / "fixtures" / relative).read_bytes())
    SUMS.write_text(json.dumps(dict(sorted(sums.items())), indent=2) + "\n")
    print(json.dumps({
        "capabilities": len(catalog),
        "catalog_sha256": digests["catalog_sha256"],
        "source_sha256": sha(SOURCE.read_bytes()),
    }, sort_keys=True))

if __name__ == "__main__":
    main()
