"""Build the review delivery on a hosted runner; this is not a release gate."""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import zipfile

SOURCE = "cd518748f742025a251b78028613aa1b16919e73"
BASE = "b736d41b61c4a4146c9e75c16796e251b025e69f"
assert os.environ.get("GITHUB_ACTIONS") == "true"
assert os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted"
root = Path(__file__).resolve().parent
product = Path(os.environ["PRODUCT_CHECKOUT"])


def git(*args):
    return subprocess.check_output(["git", "-C", str(product), *args])


assert git("rev-parse", "HEAD").decode().strip() == SOURCE
assert not git("status", "--porcelain")
git("merge-base", "--is-ancestor", BASE, SOURCE)
acceptance = json.loads((root / "delivery/INTEGRATED_ACCEPTANCE.json").read_text())
assert acceptance["source_sha"] == SOURCE
assert acceptance["r16_closed"] is False
assert acceptance["main_merge_performed"] is False
assert acceptance["release_performed"] is False
assert acceptance["engineering_ready"] is True
assert all(row["status"]=="PASS" for row in acceptance["requirements"] if row["id"]!="I12")
# These records are uploaded only after reproducible archive and CRC/hash checks.
# This package-local final acceptance is separate from the input snapshot.
acceptance["status"]="INTEGRATED_CANDIDATE_READY_FOR_REVIEW_AND_EVALUATION"
acceptance["ready_for_integration"]=True
for row in acceptance["requirements"]:
    if row["id"]=="I12":
        row["status"]="PASS"
        row["evidence"]="This hosted delivery: two byte-identical archives, CRC and every source/evidence entry digest verified; source/commit map/runbook/handoffs included. No release or main merge."
acceptance["delivery_workflow_sha"]=os.environ["GITHUB_SHA"]
acceptance["delivery_run_id"]=os.environ["GITHUB_RUN_ID"]
entries = {}
archive = tarfile.open(fileobj=io.BytesIO(git("archive", "--format=tar", SOURCE)))
for member in archive:
    if member.isfile():
        entries["source/" + member.name] = (archive.extractfile(member).read(), member.mode)
    elif member.issym():
        # Preserve the original Git symlink without traversing its destination.
        entries["source/" + member.name] = (member.linkname.encode(), 0o120777)
    elif not member.isdir():
        raise RuntimeError("Unexpected source archive entry: " + member.name)
for directory in ["delivery", "evidence"]:
    for path in sorted((root / directory).rglob("*")):
        assert not path.is_symlink()
        if path.is_file():
            assert path.stat().st_size < 2_000_000
            entries[path.relative_to(root).as_posix()] = (path.read_bytes(), 0o100644)
entries["delivery/INTEGRATED_ACCEPTANCE.json"] = ((json.dumps(acceptance,indent=2)+"\n").encode(),0o100644)
entries["integration.patch"] = (git("diff", "--binary", BASE, SOURCE), 0o100644)
entries["commit-map.txt"] = (git("log", "--format=%H %P %s", BASE + ".." + SOURCE), 0o100644)
manifest = {
    "schema_version": 1,
    "source_sha": SOURCE,
    "integration_base_sha": BASE,
    "packet_suite_sha": os.environ["GITHUB_SHA"],
    "run_id": os.environ["GITHUB_RUN_ID"],
    "kind": "DEVELOPMENT_SOURCE_AND_REVIEW_EVIDENCE",
    "acceptance_status": acceptance["status"],
    "engineering_ready": acceptance["engineering_ready"],
    "ready_for_integration": acceptance["ready_for_integration"],
    "contains_runtime_binaries": False,
    "r16_closed": False,
    "main_merge_performed": False,
    "release_performed": False,
    "files": {name: hashlib.sha256(data).hexdigest() for name, (data, _) in sorted(entries.items())},
}
entries["manifest.json"] = ((json.dumps(manifest, indent=2) + "\n").encode(), 0o100644)


def make_zip():
    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for name, (data, mode) in sorted(entries.items()):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (mode | (0o100000 if mode < 0o100000 else 0)) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data, compresslevel=9)
    return out.getvalue()


first = make_zip()
assert first == make_zip(), "Reproducible package mismatch"
with zipfile.ZipFile(io.BytesIO(first)) as z:
    assert z.testzip() is None
    assert set(z.namelist()) == set(entries)
    assert all(hashlib.sha256(z.read(n)).hexdigest() == h for n, h in manifest["files"].items())
output = Path(os.environ["PACKET_OUTPUT"])
output.mkdir(parents=True, exist_ok=True)
name = "semwright-semantic-creation-I-cd51874-review.zip"
(output / name).write_bytes(first)
digest = hashlib.sha256(first).hexdigest()
(output / (name + ".sha256")).write_text(digest + "  " + name + "\n")
(output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
(output / "INTEGRATED_ACCEPTANCE.json").write_text(json.dumps(acceptance,indent=2)+"\n")
summary={"source_sha":SOURCE,"suite_sha":os.environ["GITHUB_SHA"],"run_id":os.environ["GITHUB_RUN_ID"],
         "entries":len(entries),"bytes":len(first),"sha256":digest,"reproducible":True,"testzip":"PASS",
         "entry_hashes_verified":True,"ready_for_integration":True,"r16_closed":False,
         "contains_runtime_binaries":False,"main_merge_performed":False,"release_performed":False}
(output / "delivery-summary.json").write_text(json.dumps(summary,indent=2)+"\n")
print(json.dumps(summary))
