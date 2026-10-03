#!/usr/bin/env python3
"""Validate the bounded documentary/evidence surface, never authorize a release."""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[2]
DOCS = ("README.md", "docs/installation.md", "docs/architecture.md", "docs/platforms.md",
        "SECURITY.md", "docs/security.md", "CONTRIBUTING.md", "docs/development.md",
        "docs/compatibility.md", "VERIFY.md", "RELEASE_BLOCKERS.md", "SUPPORT.md")
SHA = re.compile(r"[0-9a-f]{40}")


def duplicate_keys(pairs: list[tuple]) -> dict:
    obj = {}
    for key, value in pairs:
        if key in obj:
            raise ValueError(f"duplicate key: {key}")
        obj[key] = value
    return obj


def links(root: Path, relative: str) -> list[str]:
    path = root / relative
    if not path.is_file():
        return [f"missing document: {relative}"]
    errors = []
    text = re.sub(r"```.*?```", "", path.read_text(), flags=re.S)
    for value in re.findall(r"\[[^\]]*\]\(([^\s)]+)\)", text):
        parsed = urlsplit(value)
        if parsed.scheme or parsed.netloc or not parsed.path:
            continue
        target = (path.parent / unquote(parsed.path)).resolve()
        if not target.is_relative_to(root.resolve()) or not target.exists():
            errors.append(f"{relative}: missing/outside local link: {value}")
    return errors


def evidence_errors(record: dict) -> list[str]:
    errors = []
    for key in ("review_target_sha", "final_source_sha"):
        if not isinstance(record.get(key), str) or not SHA.fullmatch(record[key]):
            errors.append(f"missing full {key}")
    if record.get("r16_closed") is not False:
        errors.append("this R review must not declare R16 closed")
    if record.get("external_audit") is not False:
        errors.append("this R review is not an external audit")
    expected = {f"R16-{n:02}" for n in range(1, 13)}
    rows = record.get("areas", [])
    if len(rows) != 12 or {row.get("id") for row in rows} != expected:
        errors.append("exactly twelve distinct review areas required")
    for row in rows:
        if not row.get("limitations") or not row.get("source_locations"):
            errors.append(f"{row.get('id')}: explicit source locations and limitations required")
    for item in record.get("executions", []):
        if item.get("result") == "PASS" and (not item.get("run_id") or not SHA.fullmatch(item.get("source_sha", ""))):
            errors.append("execution PASS needs run identity and source SHA")
        if item.get("result") == "SKIPPED" and item.get("executed") is not False:
            errors.append("skipped execution cannot count as executed")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--require-report", action="store_true")
    args = parser.parse_args()
    errors = [error for name in DOCS for error in links(ROOT, name)]
    install = (ROOT / "docs/installation.md").read_text()
    for block in re.findall(r"```sh\n(.*?)```", install, re.S):
        if "scripts/dev/bootstrap.sh" in block:
            errors.append("ordinary installation must not invoke lock-initialization bootstrap")
    if not (ROOT / "Cargo.lock").is_file():
        errors.append("committed lockfile is required")
    record = ROOT / "verification/r16-closeout/R16_EVIDENCE_MANIFEST.json"
    if record.exists():
        errors.extend(evidence_errors(json.loads(record.read_text(), object_pairs_hook=duplicate_keys)))
    elif args.require_report:
        errors.append("review evidence manifest absent")
    print(json.dumps({"validator": "r16-documentary-surface", "result": "FAIL" if errors else "PASS",
                      "documents_checked": len(DOCS), "errors": errors,
                      "external_links_checked": False, "security_verdict": False}, indent=2))
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
