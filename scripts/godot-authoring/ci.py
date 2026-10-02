#!/usr/bin/env python3
"""Exact-source diagnostic. Selectors are an enum, never shell command inputs."""
import hashlib, json, os, pathlib, subprocess, sys, time
from collector import parse_tests
ROOT=pathlib.Path(__file__).resolve().parents[2]
OUT=ROOT/"godot-authoring-evidence"
EXPECTED_TESTS={"authoring":18,"authoring_store":14,"authoring_profile":7,"authoring_native":10}
SUITES={"godot-model": ["cargo", "test", "--locked", "-p", "semwright-driver-godot", "--test", "authoring", "--test", "authoring_store", "--test", "authoring_profile", "--test", "authoring_native", "--", "--nocapture"]}
def main():
    if len(sys.argv)!=2 or sys.argv[1] not in SUITES:
        raise SystemExit("unregistered Godot authoring diagnostic selector")
    OUT.mkdir(exist_ok=True)
    sha=subprocess.check_output(["git","rev-parse","HEAD"],cwd=ROOT,text=True).strip()
    if os.environ.get("GITHUB_EVENT_NAME")=="push" and sha!=os.environ.get("GITHUB_SHA"):
        raise SystemExit("checkout SHA mismatch")
    suite=sys.argv[1];start=time.monotonic();command=SUITES[suite]
    receipt=dict(schema_version=1,role="D",source_sha=sha,suite=suite,contract_sha="7ab43f99f4cc62be2a9b0ce9ce1155283a429768",dependencies={"A_C0":"26602e4b25929be869d69ef28fef4dd9713180d7","A_FINAL":"7ab43f99f4cc62be2a9b0ce9ce1155283a429768","P0":"6ee52b428310370d3ad438a13964086a63f48367","C_FINAL":"504ad2c6632305b580b51703a60b0a194865780d","F_E0":"dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff","F_IMPL":"b30d693c24d7dc0527834b7830823655be5216ce"},workflow=os.environ.get("GITHUB_WORKFLOW"),run_id=os.environ.get("GITHUB_RUN_ID"),attempt=os.environ.get("GITHUB_RUN_ATTEMPT"),event=os.environ.get("GITHUB_EVENT_NAME"),job=os.environ.get("GITHUB_JOB"),job_id=None,runtime=None,features=[],lock_sha256=hashlib.sha256((ROOT/"Cargo.lock").read_bytes()).hexdigest(),command=command,requested_tests=sum(EXPECTED_TESTS.values()),executed_tests=0,skipped=None,native=False,outcome="FAIL",duration_seconds=None)
    try:
        result=subprocess.run(command,cwd=ROOT,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,check=False)
        log=result.stdout;(OUT/(suite+".log")).write_text(log)
        print(log,flush=True)
        try:
            counts = parse_tests(log, EXPECTED_TESTS)
        except ValueError as error:
            counts = {}
            receipt["collector_error"] = str(error)
        receipt["test_binaries"]=counts
        if len(counts)==4:
            receipt["executed_tests"]=sum(c["executed"] for c in counts.values())
            receipt["skipped"]=sum(c["ignored"] for c in counts.values())
        ok=result.returncode==0 and receipt["executed_tests"]>=receipt["requested_tests"] and receipt["skipped"]==0
        receipt["outcome"]="PASS" if ok else "FAIL"
        receipt["exit_code"]=result.returncode
    finally:
        receipt["duration_seconds"]=round(time.monotonic()-start,3)
        (OUT/(suite+".json")).write_text(json.dumps(receipt,indent=2)+"\n")
    return 0 if receipt["outcome"]=="PASS" else 1
if __name__=="__main__":sys.exit(main())
