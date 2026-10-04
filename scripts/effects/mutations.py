#!/usr/bin/env python3
"""Directed production-guard mutants. A compile error is NOT a killed mutant."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time

if os.environ.get("GITHUB_ACTIONS") != "true": raise SystemExit("remote CI only")
root=Path(__file__).resolve().parents[2]; os.chdir(root)
sha=subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip()
if sha != os.environ.get("EXPECTED_SHA"): raise SystemExit("source SHA mismatch")
out=root/"verification/effects"; out.mkdir(parents=True,exist_ok=True)
mutants=[
    ("false-result-promoted","src/evaluator.rs",r"verdict = Verdict::Fail;","verdict = Verdict::Pass;","evaluator","required_false_survives_unknown_and_optional_warning"),
    ("count-guard-removed","src/enumeration.rs",r"if total\.is_some_and\(\|n\| n as usize != audit\.count\) \{","if false {","enumeration","total_mismatch_is_not_complete_even_with_final_flag"),
    ("owner-guard-removed","src/evaluator.rs",r"binding\.owner != context\.owner\s*\|\|","false ||","evaluator","owner_request_operation_plan_contract_and_artifact_tampering_are_unknown"),
]
receipts=[]
for name,relative,pattern,replacement,test_file,test_name in mutants:
    path=root/"crates/effect-conformance"/relative
    original=path.read_text(); mutated,count=re.subn(pattern,replacement,original,count=1)
    if count!=1: raise SystemExit("mutation target missing: "+name)
    started=time.monotonic()
    try:
        path.write_text(mutated)
        cmd=["cargo","test","--locked","-p","semwright-effect-conformance","--test",test_file,test_name,"--","--exact","--nocapture"]
        result=subprocess.run(cmd,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=180)
        (out/(name+".log")).write_text(result.stdout[-60000:])
        killed=result.returncode!=0 and re.search(r"test "+re.escape(test_name)+r" \.\.\. FAILED",result.stdout) is not None and "1 failed" in result.stdout
        receipts.append({"name":name,"source_sha":sha,"mutated_file":relative,"mutated_sha256":hashlib.sha256(mutated.encode()).hexdigest(),
            "test":test_name,"executed":bool(re.search(r"running 1 test",result.stdout)),"killed":killed,"exit_code":result.returncode,"duration_seconds":round(time.monotonic()-started,3)})
        if not killed: raise RuntimeError("surviving or invalid mutant: "+name+"\n"+result.stdout[-5000:])
    finally:
        path.write_text(original)
        (out/"mutations.json").write_text(json.dumps({"schema_version":1,"role":"effect-conformance","source_sha":sha,"mutants":receipts},indent=2)+"\n")
print("killed all",len(receipts),"targeted mutants; restored exact original sources")
