#!/usr/bin/env python3
"""Commit-scoped maintainer secret precheck; publish metadata, never matched bodies."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import secrets
import string
import subprocess
import tarfile
import tempfile
from pathlib import Path

VERSION = "8.30.1"
ROOT = Path(__file__).resolve().parents[2]


def sanitize_findings(findings: list, snapshot: Path) -> list[dict]:
    result = []
    for finding in findings:
        if not isinstance(finding, dict):
            raise ValueError("Invalid scanner finding")
        path = str(finding.get("File", ""))
        prefix = str(snapshot) + "/"
        if path.startswith(prefix):
            path = path[len(prefix):]
        # Do not publish Match, Secret, Description, Author, Email or commit Message.
        result.append({
            "rule_id": str(finding.get("RuleID", "")),
            "file": path,
            "start_line": finding.get("StartLine"),
            "end_line": finding.get("EndLine"),
            "commit": str(finding.get("Commit", "")),
        })
    return result


def triage_metadata(finding: dict, source_line: str, entries: list[dict]) -> dict | None:
    # A new value in the same file must not inherit a reviewed exception.
    if finding.get("start_line") != finding.get("end_line"):
        return None
    digest = hashlib.sha256(source_line.strip().encode()).hexdigest()
    for entry in entries:
        if (finding.get("file") == entry["file"]
                and finding.get("rule_id") == entry["rule_id"]
                and digest == entry["line_sha256"]):
            return {"classification": "REVIEWED_NON_SECRET", "line_sha256": digest,
                    "reason": entry["reason"]}
    return None


def classify_findings(findings: list[dict], snapshot: Path, source_sha: str) -> list[dict]:
    entries = json.loads((ROOT / "scripts/ci/pre-r16-secret-triage.json").read_text())["entries"]
    for finding in findings:
        path = Path(finding["file"])
        line = finding.get("start_line")
        commit = finding.get("commit") or source_sha
        if (path.is_absolute() or ".." in path.parts or not path.parts
                or not isinstance(line, int) or isinstance(line, bool) or line < 1
                or not re.fullmatch(r"[0-9a-f]{40}", commit)):
            raise ValueError("Invalid finding source identity")
        if finding.get("commit"):
            source = subprocess.check_output(["git", "show", f"{commit}:{path.as_posix()}"],
                                             cwd=ROOT, text=True, timeout=60)
        else:
            source = (snapshot / path).read_text()
        lines = source.splitlines()
        if line > len(lines):
            raise ValueError("Finding line outside recorded source")
        triage = triage_metadata(finding, lines[line - 1], entries)
        finding["triage"] = triage or {"classification": "UNTRIAGED"}
    return findings


def scan_status(code: int, findings: list) -> str:
    if code == 0 and not findings:
        return "PASS"
    if code == 1 and findings:
        return "FINDINGS"
    return "ERROR"


def checked(*args: str) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True, timeout=60).strip()


def execute(binary: str, mode: str, source: Path, private: Path,
            config: Path, ignore: Path, log_opts: str | None = None) -> tuple[int, list]:
    report = private / (mode + "-" + secrets.token_hex(8) + ".json")
    command = [binary, mode, str(source), "--config", str(config),
               "--gitleaks-ignore-path", str(ignore), "--ignore-gitleaks-allow",
               "--redact=100", "--no-banner", "--no-color",
               "--report-format=json", "--report-path", str(report)]
    if log_opts is not None:
        command += ["--log-opts=" + log_opts]
    # Diagnostics stay in disposable private files even with full scanner redaction.
    # A scanner timeout/error cannot be interpreted as a zero-finding success.
    with (private / "scanner-diagnostics.txt").open("ab") as diagnostics:
        process = subprocess.run(command, cwd=ROOT, stdout=diagnostics, stderr=diagnostics,
                                 timeout=600, check=False)
    findings = json.loads(report.read_text())
    if not isinstance(findings, list):
        raise ValueError("Scanner report must be an array")
    return process.returncode, findings


def run(binary: str, output: Path) -> int:
    output.mkdir(mode=0o700, parents=False, exist_ok=False)
    summary = {
        "schema_version": 1, "status": "ERROR", "maintainer_precheck_only": True,
        "independent_review": False, "scanner": "gitleaks", "version": VERSION,
        "history_scope": "HEAD ancestry; not unpushed or unrelated worktree branches",
        "content_scope": "git archive of HEAD; no untracked files or working-tree caches",
        "non_claims": ["Not proof of absence of all secrets", "No nested archive or encoded-content scan"],
        "results": [],
    }
    code = 2
    try:
        summary["source_sha"] = checked("git", "rev-parse", "HEAD")
        if checked("git", "rev-parse", "--is-shallow-repository") != "false":
            raise ValueError("Complete HEAD history is required")
        if checked("git", "diff", "--name-only", "HEAD"):
            raise ValueError("Tracked working tree must match HEAD")
        summary["history_commits"] = int(checked("git", "rev-list", "--count", "HEAD"))
        summary["tracked_files"] = len(checked("git", "ls-tree", "-r", "--name-only", "HEAD").splitlines())
        if summary["history_commits"] < 1 or summary["tracked_files"] < 1:
            raise ValueError("Empty source history is not scan evidence")
        binary_path = Path(binary).resolve(strict=True)
        summary["scanner_binary_sha256"] = hashlib.sha256(binary_path.read_bytes()).hexdigest()
        if checked(str(binary_path), "version") != VERSION:
            raise ValueError("Unexpected scanner version")
        with tempfile.TemporaryDirectory(prefix="semwright-secret-precheck-") as temp:
            private = Path(temp)
            config = private / "config.toml"
            config.write_text("[extend]\nuseDefault = true\n")
            ignore = private / "empty-ignore"
            ignore.write_text("")
            sentinel = private / "sentinel"
            sentinel.mkdir(mode=0o700)
            # Synthetic, nonfunctional token: no credential is fetched or validated online.
            token = "gh" + "p_" + "".join(secrets.choice(string.ascii_letters + string.digits) for _ in range(36))
            (sentinel / "fixture.txt").write_text("GITHUB_TOKEN=" + token + "\n")
            sentinel_code, found = execute(str(binary_path), "dir", sentinel, private, config, ignore)
            if sentinel_code != 1 or not any(item.get("RuleID") == "github-pat" for item in found):
                raise ValueError("Positive scanner self-test failed")
            summary["positive_self_test"] = "PASS"
            archive = private / "source.tar"
            subprocess.run(["git", "archive", "--format=tar", "--output", str(archive),
                            summary["source_sha"]], cwd=ROOT, check=True, timeout=60)
            snapshot = private / "snapshot"
            snapshot.mkdir(mode=0o700)
            with tarfile.open(archive) as source:
                source.extractall(snapshot, filter="data")
            for label, mode, source, opts in [
                ("committed_tree", "dir", snapshot, None),
                ("head_history", "git", ROOT, "HEAD"),
            ]:
                scan_code, findings = execute(str(binary_path), mode, source, private, config, ignore, opts)
                sanitized = classify_findings(sanitize_findings(findings, snapshot), snapshot,
                                              summary["source_sha"])
                untriaged = sum(item["triage"]["classification"] == "UNTRIAGED" for item in sanitized)
                summary["results"].append({
                    "scope": label, "exit_code": scan_code, "status": scan_status(scan_code, findings),
                    "finding_count": len(findings), "untriaged_findings": untriaged,
                    "findings": sanitized,
                })
        states = [item["status"] for item in summary["results"]]
        if "ERROR" in states:
            summary["status"] = "ERROR"
        elif any(item["untriaged_findings"] for item in summary["results"]):
            summary["status"] = "FINDINGS"
        elif "FINDINGS" in states:
            summary["status"] = "PASS_WITH_TRIAGED_NON_SECRETS"
        else:
            summary["status"] = "PASS"
        code = 0 if summary["status"].startswith("PASS") else 1 if summary["status"] == "FINDINGS" else 2
    except (OSError, ValueError, subprocess.SubprocessError, tarfile.TarError) as error:
        # Exceptions may include path/content arguments. Publish only the class.
        summary["status"] = "ERROR"
        summary["error_class"] = type(error).__name__
    (output / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"status": summary["status"], "report": "summary.json"}))
    return code


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    raise SystemExit(run(arguments.binary, arguments.output))
