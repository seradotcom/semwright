"""CircleCI-only lightweight H iteration; no model/native productivity claims."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import time


def main():
    if os.environ.get("CIRCLECI") != "true" or os.environ.get("GITHUB_ACTIONS"):
        raise RuntimeError("CircleCI iteration only; no workstation or Actions execution")
    root = Path.cwd()
    suite = subprocess.check_output(["git","rev-parse","HEAD"]).decode().strip()
    assert suite == os.environ["CIRCLE_SHA1"]
    laboratory = os.environ["SW_HARNESS_SHA"]
    assert laboratory == "6fe1994d9ab25f44631ef8c41e9dad8f05aee0ca"
    if subprocess.run(["git","cat-file","-e",laboratory+"^{commit}"],capture_output=True).returncode:
        subprocess.run(["git","fetch","--no-tags","--depth=1","origin",laboratory],check=True)
    output = root/"verification/circleci-harness"
    output.mkdir(parents=True,exist_ok=False)
    work = Path(tempfile.mkdtemp(prefix="semwright-h-circle-"))
    archive = work/"h-laboratory.tar"
    with archive.open("wb") as stream:
        subprocess.run(["git","archive",laboratory,"evaluation-lab"],stdout=stream,check=True)
    subprocess.run(["tar","-xf",str(archive),"-C",str(work)],check=True)
    lab = work/"evaluation-lab"
    tracked = subprocess.check_output(["git","ls-tree","-r","--name-only",laboratory,"--","evaluation-lab"]).decode().splitlines()
    before = {name:hashlib.sha256((work/name).read_bytes()).hexdigest() for name in tracked}
    protocol = json.loads((lab/"protocol.json").read_text())
    freeze_bytes = (lab/protocol["technical_target_manifest"]).read_bytes()
    assert hashlib.sha256(freeze_bytes).hexdigest() == protocol["technical_target_manifest_sha256"]
    freeze = json.loads(freeze_bytes)
    assert freeze["SEMWRIGHT_EVAL_SHA"] == "cd518748f742025a251b78028613aa1b16919e73"
    assert protocol["budget_authorized"] is False and protocol["model_protocol_frozen"] is False
    start = time.monotonic_ns()
    result = subprocess.run(["python3","-m","unittest","discover","-s",str(lab/"tests"),"-v"],
                            cwd=work,capture_output=True,timeout=120)
    elapsed = (time.monotonic_ns()-start)//1_000_000
    log = result.stdout+b"\n"+result.stderr
    (output/"harness-tests.log").write_bytes(log)
    after = {name:hashlib.sha256((work/name).read_bytes()).hexdigest() for name in tracked}
    text = log.decode(errors="replace")
    passed = result.returncode == 0 and re.search(r"Ran 60 tests\b",text) and re.search(r"^OK$",text,re.M) and before == after
    report = {"schema_version":1,"status":"PASS" if passed else "FAIL",
              "execution_kind":"DETERMINISTIC_HARNESS_ITERATION","laboratory_source_sha":laboratory,
              "circle_helper_suite_sha":suite,"technical_product_target_sha":freeze["SEMWRIGHT_EVAL_SHA"],
              "technical_product_executed":False,"test_count":60 if passed else None,
              "source_unchanged":before==after,"runtime_ms":elapsed,"returncode":result.returncode,
              "log_sha256":hashlib.sha256(log).hexdigest(),"laboratory_files_sha256":before,
              "job_number":os.environ["CIRCLE_BUILD_NUM"],"job_url":os.environ.get("CIRCLE_BUILD_URL"),
              "workflow_id":os.environ.get("CIRCLE_WORKFLOW_ID"),"native_application_acceptance":False,
              "actual_final_heldout_reservation_generated":False,"model_evaluation_executed":False,
              "productivity_result":False,"certification_eligible":False,"r16_closed":False}
    (output/"iteration.json").write_text(json.dumps(report,indent=2)+"\n")
    print(text)
    if not passed:
        raise RuntimeError("H diagnostic failed or its inventory/source changed")
    print("H CircleCI iteration: 60 PASS; no native/model certification")


if __name__ == "__main__":
    main()
