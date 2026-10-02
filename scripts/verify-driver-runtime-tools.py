#!/usr/bin/env python3
"""Reject ambient runtime discovery in production drivers and built-in adapters."""

from __future__ import annotations

import re
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PATTERNS = (
    re.compile(r"/usr/(?:local/)?bin/[A-Za-z0-9_.+-]+"),
    re.compile(r"(?i)\bcommand\s+-v\b"),
    re.compile(r'Command::new\(\s*"(?:which|where(?:\.exe)?)"\s*\)'),
    re.compile(r"(?i)[A-Z]:\\\\Program Files\\\\"),
    re.compile(r"/Applications/[^\s\"']+\.app/"),
    re.compile(r"\btool_path\("),
    re.compile(r'"runtime\.json"'),
    re.compile(r"/plugin/tools/"),
)

# Existing runtime debt. Exact counts make this a ratchet: entries may disappear
# as drivers migrate to generic primitives, but they may never grow silently.
LEGACY_COUNTS = {}

issues: list[str] = []
legacy_seen: Counter[tuple[str, str]] = Counter()
SOURCE_SUFFIXES = {".rs", ".py", ".gd", ".ts", ".js", ".mjs", ".sh", ".go"}

def add_sources(paths: set[Path], source_root: Path) -> None:
    if not source_root.is_dir():
        return
    paths.update(
        path
        for path in source_root.rglob("*")
        if path.is_file() and path.suffix in SOURCE_SUFFIXES
    )

production_paths: set[Path] = set()
for driver_root in (ROOT / "crates").glob("driver-*"):
    add_sources(production_paths, driver_root / "src")
    add_sources(production_paths, driver_root / "plugin" / "src")
add_sources(production_paths, ROOT / "crates" / "adapters" / "src")
for source_root in (ROOT / "integrations").glob("*/driver/src"):
    add_sources(production_paths, source_root)
for path in sorted(production_paths):
    rel = str(path.relative_to(ROOT))
    if rel.startswith("crates/driver-host/") or rel.startswith("crates/driver-sdk/"):
        continue
    production = path.read_text().split("#[cfg(test)]", 1)[0]
    for line_no, line in enumerate(production.splitlines(), 1):
        for pattern in PATTERNS:
            for match in pattern.finditer(line):
                token = match.group(0)
                key = (rel, token)
                if key in LEGACY_COUNTS:
                    legacy_seen[key] += 1
                else:
                    issues.append(
                        f"{rel}:{line_no}: ambient runtime resolution is not allowed: {token}"
                    )

for key, expected in sorted(LEGACY_COUNTS.items()):
    actual = legacy_seen[key]
    if actual != expected:
        issues.append(
            f"legacy runtime baseline changed: {key[0]}: {key[1]} "
            f"expected={expected} actual={actual}"
        )

if issues:
    raise SystemExit("driver runtime-tool boundary failed:\n" + "\n".join(issues))
print(
    "driver runtime-tool boundary: PASS; "
    f"tracked legacy entries={sum(legacy_seen.values())}"
)
