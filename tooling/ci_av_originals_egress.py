#!/usr/bin/env python3
"""SDK-only bounded FFV1 original custody; no native invocation or replay.

Encrypts real recorder originals with the reviewed Motionwright private sealer.
No original stream is reconstructed. Only ciphertext and safe metadata are public.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys

import ci_private_evidence as custody

SOURCE = re.compile(r"[0-9a-f]{40}\Z")
RUN = re.compile(r"[1-9][0-9]*\Z")
NAME = re.compile(r"(av-motion-[0-9a-f]{16}\.mkv)(\.ffv1-[a-z0-9-]+\.(?:stdin|stdout|stderr|terminal)\.original\.(?:bin|json)|\.ffv1-process-custody\.original\.json)\Z")
SCHEMA = "semwright.combined-av.typed-original-custody/1"
LIMITS = {}
for group in ("host", "catalog"):
    for kind, cap in (("stdin.original.bin", 65536),
                      ("stdout.original.bin", 262144),
                      ("stderr.original.bin", 262144),
                      ("terminal.original.json", 65536)):
        LIMITS[f".ffv1-{group}.{kind}"] = cap
for group in ("zscale-help", "encode", "media-probe", "complete-probe",
              "decoder-version", "decode"):
    for kind, cap in (("stdout.original.bin", 16384),
                      ("stderr.original.bin", 16384),
                      ("terminal.original.json", 65536)):
        LIMITS[f".ffv1-{group}.{kind}"] = cap
LIMITS[".ffv1-process-custody.original.json"] = 65536

def need(ok, reason):
    if not ok:
        raise custody.CustodyRejected(reason)

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def directory(path, private=False):
    need(path.is_absolute(), "directory_not_absolute")
    for item in (path, *path.parents):
        if item.exists():
            mode = item.lstat().st_mode
            need(stat.S_ISDIR(mode) and not stat.S_ISLNK(mode),
                 "directory_ancestor_not_regular")
    info = path.lstat()
    need(stat.S_ISDIR(info.st_mode) and path.resolve(strict=True) == path,
         "directory_not_canonical")
    if private:
        need(info.st_uid == os.getuid() and not info.st_mode & 0o077,
             "directory_not_private_0700")

def identity():
    need(os.name == "posix", "linux_only")
    values = os.environ
    source, rid, attempt = (values.get(x, "") for x in
                            ("CANDIDATE_SHA", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"))
    need(bool(SOURCE.fullmatch(source)) and bool(RUN.fullmatch(rid))
         and bool(RUN.fullmatch(attempt)), "exact_run_source_required")
    temp, workspace = (Path(values.get(x, "")) for x in
                       ("RUNNER_TEMP", "GITHUB_WORKSPACE"))
    directory(temp); directory(workspace)
    need(not temp.is_relative_to(workspace) and workspace != temp,
         "private_root_inside_workspace")
    head = subprocess.check_output(["git", "-C", str(workspace), "rev-parse", "HEAD"],
                                   timeout=15, text=True).strip()
    need(head == source, "source_mismatch")
    recipient = {"spki_der_base64": values.get("EVIDENCE_RECIPIENT_SPKI", ""),
                 "sha256": values.get("EVIDENCE_RECIPIENT_SHA256", "")}
    custody.public_key(recipient)
    context = {"source_sha": source, "semwright_sha": source,
               "run_id": rid, "run_attempt": attempt}
    custody.validate_context(context)
    root = temp / f"combined-av-native-originals-{rid}-attempt-{attempt}"
    need(not root.is_relative_to(workspace), "originals_inside_checkout")
    return temp, workspace, root, recipient, context

def inspect(root):
    need(len(LIMITS) == 27 and sum(LIMITS.values()) == 1966080,
         "allowlist_budget_changed")
    if not root.exists():
        need(not root.is_symlink(), "linked_original_root")
        return [], None, "ROOT_ABSENT", 0
    directory(root, private=True)
    shared = root / "shared-output"
    if not shared.exists():
        need(not shared.is_symlink(), "linked_shared_output")
        return [], None, "SHARED_ABSENT", 0
    directory(shared, private=True)
    paths = sorted(shared.iterdir(), key=lambda x: x.name)
    need(len(paths) <= 256, "namespace_budget")
    selected, prefixes, other = [], set(), 0
    for path in paths:
        match = NAME.fullmatch(path.name)
        if match is None or match.group(2) not in LIMITS:
            other += 1
            continue
        stat_before = path.lstat()
        need(stat.S_ISREG(stat_before.st_mode), "original_not_regular")
        need(stat_before.st_size <= LIMITS[match.group(2)], "original_too_large")
        prefixes.add(match.group(1))
        selected.append((path, path.name, LIMITS[match.group(2)]))
    need(len(prefixes) <= 1 and len(selected) <= 27
         and sum(x[0].lstat().st_size for x in selected) <= 1966080,
         "digest_or_file_budget")
    return selected, next(iter(prefixes)) if prefixes else None, "OBSERVED", other

def perform(operation):
    temp, workspace, root, recipient, context = identity()
    need(temp.lstat().st_uid == os.getuid()
         and not temp.lstat().st_mode & 0o022, "runner_temp_not_private_owner")
    if operation == "preflight":
        need(not root.exists() and not root.is_symlink(), "root_already_exists")
        return {"schema": SCHEMA, "status": "PREFLIGHT", "sdk_sha": context["source_sha"],
                "native_cases_executed": 0, "automated_retry": False}
    selected, prefix, root_status, other = inspect(root)
    output = workspace / "verification" / "composition-av"
    directory(output)
    staging = temp / f"combined-av-egress-{context['run_id']}-attempt-{context['run_attempt']}"
    staging.mkdir(mode=0o700, exist_ok=False)
    directory(staging, private=True)
    isolated = staging / "selected"
    isolated.mkdir(mode=0o700, exist_ok=False)
    before = []
    for original, name, limit in selected:
        meta = original.lstat()
        os.link(original, isolated / name, follow_symlinks=False)
        copy = (isolated / name).lstat()
        need(meta.st_dev == copy.st_dev and meta.st_ino == copy.st_ino
             and meta.st_size == copy.st_size and meta.st_mtime_ns == copy.st_mtime_ns
             and meta.st_size <= limit, "original_identity_changed")
        before.append((original, meta.st_dev, meta.st_ino, meta.st_size, meta.st_mtime_ns))
    snapshot, complete = custody.gather_snapshot({"selected": isolated}, preserve_partial=True)
    for original, dev, ino, size, mtime in before:
        meta = original.lstat()
        need(stat.S_ISREG(meta.st_mode) and
             (meta.st_dev, meta.st_ino, meta.st_size, meta.st_mtime_ns) ==
             (dev, ino, size, mtime), "original_changed_during_snapshot")
    envelope = custody.encrypt_snapshot(snapshot, recipient, context)
    custody.new_bytes(output / "private-evidence.mwseal", envelope)
    absent = (sorted(set(LIMITS) - {name[len(prefix):] for _, name, _ in selected})
              if prefix is not None else None)
    public = {"schema": SCHEMA, "status": "SEALED" if complete and selected else "PARTIAL_OR_ABSENT",
              "scope": "SDK_ONLY_NO_PRODUCT_G2", "sdk_sha": context["source_sha"],
              "run_id": context["run_id"], "attempt": context["run_attempt"],
              "selected_count": len(selected), "excluded_count": other,
              "root_status": root_status, "absent_suffixes": absent,
              "absence_claim": "EXACT_OBSERVED_NAMESPACE" if prefix else "UNKNOWN_NO_PREFIX",
              "ciphertext_sha256": sha(envelope), "ciphertext_bytes": len(envelope),
              "recipient_sha256": recipient["sha256"], "captured_complete": complete,
              "missing_is_not_empty": True, "native_pass_asserted": False,
              "release_pass": False, "automatic_retry": False}
    custody.new_json(output / "private-evidence.public.json", public)
    return public

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["preflight", "seal"])
    args = parser.parse_args()
    try:
        result = perform(args.mode)
        print(json.dumps({"status": result["status"],
                          "sdk_sha": result["sdk_sha"],
                          "selected_count": result.get("selected_count", 0)}))
        return 0
    except BaseException as exc:
        print("AV_CUSTODY_STOPPED:"+type(exc).__name__+":"+str(exc)[:120], file=sys.stderr)
        return 2

if __name__ == "__main__":
    raise SystemExit(main())
