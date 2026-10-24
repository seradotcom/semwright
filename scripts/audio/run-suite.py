#!/usr/bin/env python3
"""Run one allowlisted audio suite with exact-SHA evidence and zero-test detection."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import stat
import sys

ROOT = Path(__file__).resolve().parents[2]
SUITES = {
    "portable": ["-p", "semwright-audio-authoring", "-p", "semwright-audio-domain", "-p", "semwright-faust-audio-driver", "-p", "semwright-ardour-audio-driver", "--tests"],
    "faust": ["-p", "semwright-faust-audio-driver", "--test", "live_faust"],
    "faust-host": ["-p", "semwright-faust-audio-driver", "--test", "host_conformance"],
    "analysis-host": ["-p", "semwright-faust-audio-driver", "--test", "analysis_host_conformance", "--test", "analysis_host_snapshot_conformance"],
    "ardour-host": ["-p", "semwright-ardour-audio-driver", "--test", "host_conformance"],
}


ANALYSIS_TESTS = {
    "analysis_host_conformance": "broker_host_meter_measures_digest_bound_wav_and_rejects_substitution",
    "analysis_host_snapshot_conformance": "replacement_profile_uses_host_owned_meter_snapshots",
}
ANALYSIS_BINS = (
    "semwright-audio-analysis-driver", "semwright-audio-alignment-driver",
    "semwright-audio-meter-snapshot",
)
ANALYSIS_REPORTS = {
    "analysis-host.json": "analysis-host-originals/legacy-analysis-host.json",
    "analysis-host-snapshot.json": "analysis-host-snapshot.json",
}


def analysis_file(path, keep_bytes=False, limit=512 * 1024 * 1024):
    path = Path(path).absolute()
    if ".." in path.parts:
        raise RuntimeError("Analysis evidence path has a parent alias")
    for item in (path, *path.parents):
        info = item.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
            raise RuntimeError("Analysis evidence path is linked/reparsed")
    first = path.lstat()
    if not stat.S_ISREG(first.st_mode) or first.st_size > limit:
        raise RuntimeError("Analysis input is nonregular or exceeds its byte limit")
    stamp = lambda info: [info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_nlink]
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_BINARY", 0)
    checksum, size, chunks = hashlib.sha256(), 0, []
    with os.fdopen(os.open(path, flags), "rb") as stream:
        if stamp(os.fstat(stream.fileno())) != stamp(first):
            raise RuntimeError("Analysis input replaced before read")
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            size += len(chunk)
            if size > limit:
                raise RuntimeError("Analysis input exceeded its byte limit during read")
            checksum.update(chunk)
            if keep_bytes:
                chunks.append(chunk)
        if stamp(os.fstat(stream.fileno())) != stamp(first):
            raise RuntimeError("Analysis input changed during read")
    if stamp(path.lstat()) != stamp(first) or size != first.st_size:
        raise RuntimeError("Analysis input replaced after read")
    return {"path": str(path), "bytes": size, "sha256": checksum.hexdigest(),
            "identity": stamp(first)}, b"".join(chunks) if keep_bytes else None


def analysis_preflight(out):
    if sys.platform != "linux":
        raise RuntimeError("Real analysis Host suite requires its existing Linux lane")
    package = ROOT / "crates/driver-faust-audio"
    fresh = [out / ("analysis-host" + suffix) for suffix in ("-list.log", ".log", ".json")]
    fresh += [out / "analysis-host-originals", out / "analysis-host-snapshot.json"]
    fresh += [package / "verification/audio" / name for name in ANALYSIS_REPORTS]
    for path in fresh:
        if os.path.lexists(path):
            raise RuntimeError("Stale analysis original exists; preserve it and do not retry: " + str(path))
        for parent in path.parents:
            if parent.exists() or parent.is_symlink():
                info = parent.lstat()
                if not stat.S_ISDIR(info.st_mode) or stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
                    raise RuntimeError("Analysis output ancestry is not an owned regular directory")
    for name in ("SEMWRIGHT_TEST_SANDBOX_HELPER", "SEMWRIGHT_TEST_AUDIO_METER"):
        value = os.getenv(name)
        if not value or not Path(value).is_absolute():
            raise RuntimeError("Missing absolute native analysis prerequisite: " + name)
        analysis_file(value)


def analysis_source(sha):
    current = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if not re.fullmatch(r"[0-9a-f]{40}", sha) or current != sha:
        raise RuntimeError("Analysis source SHA changed")
    if os.getenv("GITHUB_ACTIONS") == "true" and os.getenv("GITHUB_SHA") != sha:
        raise RuntimeError("Analysis checked-out SHA differs from the workflow SHA")
    tracked = subprocess.run(["git", "diff", "--quiet", "HEAD", "--"], cwd=ROOT, capture_output=True, check=False)
    if type(tracked.returncode) is not int or tracked.returncode != 0:
        raise RuntimeError("Analysis tracked source is dirty or could not be observed")
    paths = (
        "Cargo.lock", "Cargo.toml", "scripts/audio/run-suite.py", ".github/workflows/audio-diagnostics.yml",
        "crates/driver-faust-audio/Cargo.toml",
        *("crates/driver-faust-audio/tests/" + name + ".rs" for name in ANALYSIS_TESTS),
    )
    return {"source_sha": sha, "tracked_diff": "CLEAN", "selected_inputs": {
        name: analysis_file(ROOT / name, limit=16 * 1024 * 1024)[0] for name in paths}}


def analysis_nominal(text, listed):
    text = text.replace("\r\n", "\n")
    if any(character in text for character in ("\r", "\v", "\f", "\x85", "\u2028", "\u2029")):
        raise RuntimeError("Analysis nominal log contains a non-line separator")
    pattern = r"([A-Za-z_][A-Za-z0-9_:]*): test" if listed else r"test ([A-Za-z_][A-Za-z0-9_:]*) \.\.\. (ok|FAILED|ignored)"
    rows = [match.groups() for line in text.split("\n") if (match := re.fullmatch(pattern, line))]
    names = [row[0] for row in rows]
    if sorted(names) != sorted(ANALYSIS_TESTS.values()) or len(names) != len(set(names)):
        raise RuntimeError("Analysis Host nominal roster differs, is duplicated, or omitted a selected case")
    if not listed and any(row[1] != "ok" for row in rows):
        raise RuntimeError("Analysis Host named cases did not all pass without ignored tests")
    return names


def analysis_listing(listing):
    cases = analysis_nominal(listing, True)
    wanted = {**{name: "bin" for name in ANALYSIS_BINS}, **{name: "test" for name in ANALYSIS_TESTS}}
    package_manifest = ROOT / "crates/driver-faust-audio/Cargo.toml"
    binaries = {}
    for line in listing.split("\n"):
        if not line.startswith("{"):
            continue
        try:
            row = json.loads(line)
        except (ValueError, TypeError) as error:
            raise RuntimeError("Invalid Cargo artifact JSON in analysis inventory") from error
        if type(row) is not dict or row.get("reason") != "compiler-artifact":
            continue
        target = row.get("target")
        if type(target) is not dict or target.get("name") not in wanted:
            continue
        name, kind = target["name"], wanted[target["name"]]
        if (row.get("manifest_path") != str(package_manifest)
                or target.get("kind") != [kind]
                or type(row.get("profile")) is not dict
                or row["profile"].get("test") is not (kind == "test")):
            raise RuntimeError("Analysis Cargo artifact does not belong to the selected package/target")
        executable = row.get("executable")
        if type(executable) is not str or not Path(executable).is_absolute() or name in binaries:
            raise RuntimeError("Analysis Cargo executable is absent, relative, or duplicated")
        path = Path(executable)
        if not path.is_relative_to(ROOT) or ".." in path.parts:
            raise RuntimeError("Analysis Cargo executable escaped the candidate workspace")
        binaries[name] = analysis_file(path)[0]
    if set(binaries) != set(wanted):
        raise RuntimeError("Analysis Cargo inventory omitted a selected test or actual driver/snapshot binary")
    for name in ("SEMWRIGHT_TEST_SANDBOX_HELPER", "SEMWRIGHT_TEST_AUDIO_METER"):
        value = os.getenv(name)
        if not value or not Path(value).is_absolute():
            raise RuntimeError("Analysis native prerequisite changed or disappeared")
        binaries[name] = analysis_file(value)[0]
    return {"expected_test_ids": cases, "test_targets": list(ANALYSIS_TESTS), "binaries": binaries,
            "binding_point": "AFTER_ALL_LISTING_PRODUCERS_BEFORE_NATIVE_EXECUTION"}


def analysis_retain_reports(out, receipt):
    observed = {}
    missing = []
    package_output = ROOT / "crates/driver-faust-audio/verification/audio"
    for name, relative in ANALYSIS_REPORTS.items():
        source = package_output / name
        if not os.path.lexists(source):
            missing.append(name)
            continue
        original, raw = analysis_file(source, keep_bytes=True, limit=4 * 1024 * 1024)
        destination = out / relative
        if relative.startswith("analysis-host-originals/"):
            destination.parent.mkdir(exist_ok=False)
        with destination.open("xb") as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        copied, copied_bytes = analysis_file(destination, keep_bytes=True, limit=4 * 1024 * 1024)
        if copied_bytes != raw or copied["sha256"] != original["sha256"]:
            raise RuntimeError("Analysis original copy differs from its package output")
        unchanged, _ = analysis_file(source, limit=4 * 1024 * 1024)
        if unchanged != original:
            raise RuntimeError("Analysis original changed during retention")
        observed[name] = {"package_original": original, "retained_original": copied,
                          "original_bytes_unchanged": True, "json_reencoded": False}
        receipt["analysis_native_originals"] = observed
    receipt["analysis_native_originals"] = observed
    receipt["analysis_missing_originals"] = missing
    return missing


def analysis_finish(receipt, inventory, source_before, code, text, out, sha):
    receipt["analysis_exit_code"] = code
    receipt["analysis_inventory"] = inventory
    receipt["analysis_source_before"] = source_before
    unknown = type(code) is not int or code < 0
    if unknown:
        receipt["status"] = "UNKNOWN_NO_RETRY"
        receipt["analysis_postcheck"] = "NOT_OBSERVED_AFTER_UNKNOWN"
    receipt["analysis_stream_originals"] = {
        name: analysis_file(out / name, limit=64 * 1024 * 1024)[0]
        for name in ("analysis-host-list.log", "analysis-host.log")}
    if unknown:
        try:
            analysis_retain_reports(out, receipt)
        except (RuntimeError, OSError, ValueError) as error:
            receipt["analysis_custody_error"] = str(error)
        raise RuntimeError("Analysis terminal was not an ordinary exit; no retry or post-execution command")
    missing = analysis_retain_reports(out, receipt)
    after = {name: analysis_file(Path(row["path"]))[0] for name, row in inventory["binaries"].items()}
    receipt["analysis_binaries_after"] = after
    if after != inventory["binaries"]:
        raise RuntimeError("Analysis test/driver/helper/meter identity changed after sealing")
    receipt["analysis_source_after"] = analysis_source(sha)
    if receipt["analysis_source_after"] != source_before:
        raise RuntimeError("Analysis source or lock changed across native execution")
    if code != 0 or missing:
        raise RuntimeError("Analysis failed or did not produce every required original")
    receipt["analysis_executed_test_ids"] = analysis_nominal(text, False)
    receipt["analysis_product_pair"] = "STANDALONE_SDK_ONLY_NO_MOTIONWRIGHT_PAIR_CREDIT"
    receipt["analysis_formal_acceptance"] = False


def run(argv, log):
    with log.open("w", encoding="utf-8") as stream:
        result = subprocess.run(argv, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT, check=False)
    text = log.read_text(encoding="utf-8", errors="replace")
    print(text[-48000:])
    return result.returncode, text


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("suite", choices=sorted(SUITES))
    args = parser.parse_args()
    out = ROOT / "verification" / "audio"
    analysis = args.suite == "analysis-host"
    if analysis:
        analysis_preflight(out)
    out.mkdir(parents=True, exist_ok=True)
    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    extra = ["--ignored"] if args.suite in {"faust", "faust-host", "analysis-host", "ardour-host"} else []
    command = ["cargo", "test", "--locked", *SUITES[args.suite]]
    if analysis:
        command.insert(3, "--message-format=json")
    receipt = {"schema_version": 1, "tested_sha": sha, "github_sha": os.getenv("GITHUB_SHA"),
               "run_id": os.getenv("GITHUB_RUN_ID"), "run_attempt": os.getenv("GITHUB_RUN_ATTEMPT"),
               "job": os.getenv("GITHUB_JOB"), "suite": args.suite, "status": "FAIL",
               "lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
               "expected_tests": 0, "passed": 0, "failed": 0, "ignored": 0}
    try:
        source_before = analysis_source(sha) if analysis else None
        code, listing = run(command + ["--", "--list", *extra], out / (args.suite + "-list.log"))
        expected = sum(bool(re.match(r"^.+: test$", line)) for line in listing.splitlines())
        receipt["expected_tests"] = expected
        if code or not expected:
            raise RuntimeError("Suite failed to enumerate nonzero tests")
        inventory = analysis_listing(listing) if analysis else None
        code, text = run(command + ["--", *extra, "--nocapture"], out / (args.suite + ".log"))
        if analysis:
            analysis_finish(receipt, inventory, source_before, code, text, out, sha)
        summaries = re.findall(r"test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
        for key, index in [("passed", 0), ("failed", 1), ("ignored", 2)]:
            receipt[key] = sum(int(row[index]) for row in summaries)
        if code or receipt["failed"] or receipt["passed"] == 0:
            raise RuntimeError("Suite did not execute successfully")
        if receipt["passed"] + receipt["ignored"] != expected:
            raise RuntimeError("Executed/ignored test count differs from enumeration")
        if args.suite != "portable" and receipt["ignored"]:
            raise RuntimeError("Native suite cannot satisfy acceptance with ignored tests")
        receipt["status"] = "PASS"
    except (RuntimeError, OSError, ValueError) as error:
        receipt["error"] = str(error)
    if analysis:
        with (out / (args.suite + ".json")).open("x", encoding="utf-8") as stream:
            stream.write(json.dumps(receipt, indent=2) + "\n")
    else:
        (out / (args.suite + ".json")).write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))
    return 0 if receipt["status"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
