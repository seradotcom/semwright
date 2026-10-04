"""Public metadata validation; never executes or publishes received source."""
from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path, PurePosixPath

EXPECTED = {
    "source": ("d5f337af548010e4adcd2578834ff9319ae709fca3ac7b00354fff60166c97c4", 61),
    "validation": ("aff4949137d162e47ee329a78d8e2656f519a68f22696f36ae18e90c69b9c12c", 51),
    "master": ("b440d45fe28e3c66c57e3fdff69265e1289164a4af579bfb6defe822bdf97df8", 9),
}
BASELINE = "4f121b3cd1d469c556adfa54159878dc96022148"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def valid_hash(value: object) -> bool:
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def validate_source_lock(data: dict) -> int:
    require(data.get("schema_version") == 1, "source-lock schema")
    require(data.get("canonical_baseline") == BASELINE, "immutable baseline")
    require(data.get("public_distribution_permitted") is False, "unresolved license gate")
    sources = data.get("sources", [])
    require(len(sources) == 3, "three archives required")
    seen = set()
    total = 0
    for archive in sources:
        kind = archive.get("kind")
        require(kind in EXPECTED and kind not in seen, "unique archive kind")
        seen.add(kind)
        digest, count = EXPECTED[kind]
        require(archive.get("sha256") == digest, "immutable archive identity")
        files = archive.get("files", [])
        require(archive.get("file_count") == count == len(files), "complete member inventory")
        require(archive.get("internal_hashes_verified") == count - 1, "checksum coverage")
        paths = set()
        for member in files:
            name = member.get("path", "")
            path = PurePosixPath(name)
            require(bool(name) and not path.is_absolute() and ".." not in path.parts,
                    "relative member path")
            require("\\" not in name and name not in paths, "unique canonical member path")
            paths.add(name)
            require(valid_hash(member.get("sha256")), "member hash")
            size = member.get("bytes")
            require(type(size) is int and 0 <= size <= 8_000_000, "member size")
        total += len(files)
    return total


def validate_acceptance(data: dict) -> int:
    rows = data.get("scenarios", [])
    require(data.get("schema_version") == 1 and len(rows) == 30, "acceptance coverage")
    require({row["id"] for row in rows} == {f"NATIVE-{i:02}" for i in range(1, 31)},
            "unique acceptance IDs")
    for row in rows:
        require(row["status"] in {"NOT_RUN", "BLOCKED", "PARTIAL", "PASS", "FAIL"},
                "explicit acceptance status")
        if row["status"] == "PASS":
            require(bool(row.get("evidence")), "PASS requires evidence")
            for proof in row["evidence"]:
                require(proof.get("ci_provider") == "github-actions", "final Actions evidence")
                require(re.fullmatch(r"[0-9a-f]{40}", proof.get("sha", "")) is not None,
                        "exact tested SHA")
                require(type(proof.get("tests")) is int and proof["tests"] > 0,
                        "positive executed test count")
                require(proof.get("skipped", 0) == 0, "skips cannot certify a scenario")
    return len(rows)


def file_digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def audit(root: Path) -> dict:
    docs = root / "docs/native-sdk"
    return {
        "members": validate_source_lock(json.loads((docs / "SOURCE_LOCK.json").read_text())),
        "scenarios": validate_acceptance(json.loads((docs / "ACCEPTANCE_MATRIX.json").read_text())),
        "scope": "metadata-only-not-native-acceptance",
    }
