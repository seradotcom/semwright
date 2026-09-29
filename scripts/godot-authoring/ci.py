#!/usr/bin/env python3
"""Exact-source diagnostic. Selectors are an enum, never shell command inputs."""
import hashlib, json, os, pathlib, re, subprocess, sys, time
ROOT=pathlib.Path(__file__).resolve().parents[2]
OUT=ROOT/"godot-authoring-evidence"
SUITES={"godot-model": ["cargo", "test", "--locked", "-p", "semwright-driver-godot", "--test", "authoring", "--", "--nocapture"]}
def main():
    if len(sys.argv)!=2 or sys.argv[1] not in SUITES:
        raise SystemExit("unregistered Godot authoring diagnostic selector")
    OUT.mkdir(exist_ok=True)
    sha=subprocess.check_output(["git","rev-parse","HEAD"],cwd=ROOT,text=True).strip()
    if os.environ.get("GITHUB_EVENT_NAME")=="push" and sha!=os.environ.get("GITHUB_SHA"):
        raise SystemExit("checkout SHA mismatch")
    suite=sys.argv[1];start=time.monotonic();command=SUITES[suite]
    receipt=dict(schema_version=1,role="D",source_sha=sha,suite=suite,contract_sha="26602e4b25929be869d69ef28fef4dd9713180d7",dependencies={"P0":None,"E0":None},workflow=os.environ.get("GITHUB_WORKFLOW"),run_id=os.environ.get("GITHUB_RUN_ID"),attempt=os.environ.get("GITHUB_RUN_ATTEMPT"),event=os.environ.get("GITHUB_EVENT_NAME"),job=os.environ.get("GITHUB_JOB"),job_id=None,runtime=None,features=[],lock_sha256=hashlib.sha256((ROOT/"Cargo.lock").read_bytes()).hexdigest(),command=command,requested_tests=12,executed_tests=0,skipped=None,native=False,outcome="FAIL",duration_seconds=None)
    try:
        result=subprocess.run(command,cwd=ROOT,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,check=False)
        log=result.stdout;(OUT/(suite+".log")).write_text(log)
        print(log,flush=True)
        matches=re.findall(r"^test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;",log,re.M)
        if len(matches)==1:
            receipt["executed_tests"],receipt["skipped"]=map(int,matches[0])
        ok=result.returncode==0 and receipt["executed_tests"]>=receipt["requested_tests"] and receipt["skipped"]==0
        receipt["outcome"]="PASS" if ok else "FAIL"
        receipt["exit_code"]=result.returncode
    finally:
        receipt["duration_seconds"]=round(time.monotonic()-start,3)
        (OUT/(suite+".json")).write_text(json.dumps(receipt,indent=2)+"\n")
    return 0 if receipt["outcome"]=="PASS" else 1
if __name__=="__main__":sys.exit(main())
