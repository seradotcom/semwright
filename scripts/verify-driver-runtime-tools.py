#!/usr/bin/env python3
"""Reject new ambient executable discovery in production application-driver source."""

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
)
# Existing persistent-runtime debt. Exact counts prevent the baseline from growing.
LEGACY_COUNTS = {
    ("crates/driver-libreoffice/src/main.rs", "/usr/bin/python3"): 1,
    ("crates/driver-libreoffice/src/main.rs", "/usr/bin/soffice"): 1,
    ("crates/driver-libreoffice/src/main.rs", "/usr/bin/sh"): 1,
}

issues: list[str] = []
legacy_seen: Counter[tuple[str, str]] = Counter()
for path in sorted((ROOT / "crates").glob("driver-*/src/**/*.rs")):
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
                    issues.append(f"{rel}:{line_no}: ambient executable discovery: {token}")

for key, expected in sorted(LEGACY_COUNTS.items()):
    actual = legacy_seen[key]
    if actual != expected:
        issues.append(f"legacy baseline changed: {key[0]}: {key[1]} expected={expected} actual={actual}")
if issues:
    raise SystemExit("driver runtime-tool boundary failed:\n" + "\n".join(issues))
print(f"driver runtime-tool boundary: PASS; tracked legacy entries={sum(legacy_seen.values())}")
