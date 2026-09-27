#!/usr/bin/env python3
"""Reject environment-specific development traces from the public OSS surface."""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

ALLOWED_HOME_NAMES = {
    "YOUR_USER",
    "user",
    "runner",
    "semwright",
    "owner",
    "test",
    "tester",
    "alice",
    "bob",
}

UNIX_HOME = re.compile(r"/(?:home|Users)/([^/\s\"']+)/")
WINDOWS_HOME = re.compile(r"(?i)[A-Z]:\\Users\\([^\\/\s\"']+)\\")

# Build these expressions compositionally so the guard does not publish the
# environment-specific phrases that it rejects.
SITUATIONAL_MARKERS = [
    re.compile(r"\bthis\s+" + r"hand" + r"off\b", re.I),
    re.compile(r"\bsource\s+" + r"dr" + r"op\b", re.I),
    re.compile(r"\b(?:owner|developer)\s+work\s*station\b", re.I),
    re.compile(r"\blocal\s+development\s+machine\b", re.I),
    re.compile(r"\bnot\s+run\s+locally\b", re.I),
    re.compile(r"\bno\s+compiler\s+ran\b", re.I),
    re.compile(r"\bno\s+coverage\s+value\b", re.I),
    re.compile(r"\bavailable\s+for\s+this\s+closeout\b", re.I),
    re.compile(
        r"(?mi)^(?:#{1,6}\s+)?local\s+(?:verification|validation|checks)\b"
    ),
    re.compile(
        r"(?i)\blocal\b.{0,48}\b(?:filesystem\s+filled|disk\s+usage|compile\s+is\s+blocked)\b"
    ),
]


def tracked_files() -> list[Path]:
    raw = subprocess.check_output(["git", "-C", str(ROOT), "ls-files", "-z"])
    return [ROOT / value.decode() for value in raw.split(b"\0") if value]


def text_file(path: Path) -> str | None:
    data = path.read_bytes()
    if b"\0" in data:
        return None
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return None


def scan_text(path: Path, text: str) -> list[str]:
    rel = path.relative_to(ROOT)
    issues: list[str] = []
    for pattern in SITUATIONAL_MARKERS:
        match = pattern.search(text)
        if match:
            issues.append(f"{rel}: environment-specific execution wording")
    for pattern, label in ((UNIX_HOME, "Unix home"), (WINDOWS_HOME, "Windows user home")):
        for match in pattern.finditer(text):
            name = match.group(1)
            if name not in ALLOWED_HOME_NAMES:
                issues.append(f"{rel}: non-placeholder {label} path")
    return issues


def head_message_issues() -> list[str]:
    try:
        message = subprocess.check_output(
            ["git", "-C", str(ROOT), "log", "-1", "--format=%B", "HEAD"],
            text=True,
        )
    except subprocess.CalledProcessError:
        return []
    pseudo = ROOT / "HEAD_COMMIT_MESSAGE"
    return [
        issue.replace("HEAD_COMMIT_MESSAGE", "HEAD commit message")
        for issue in scan_text(pseudo, message)
    ]


def main() -> int:
    issues: list[str] = []
    for path in tracked_files():
        text = text_file(path)
        if text is not None:
            issues.extend(scan_text(path, text))
    issues.extend(head_message_issues())
    if issues:
        print("OSS hygiene validation failed:", file=sys.stderr)
        for issue in sorted(set(issues)):
            print(f"- {issue}", file=sys.stderr)
        return 1
    print("OSS hygiene: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
