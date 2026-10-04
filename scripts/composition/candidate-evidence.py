#!/usr/bin/env python3
"""Validate one exact-SHA Composition/AV candidate evidence manifest."""
from __future__ import annotations
import argparse,json,re,sys
from pathlib import Path

SHA=re.compile(r"^[0-9a-f]{40}$")
STATES={"PASS","FAIL","UNKNOWN","PENDING","NOT_APPLICABLE"}
REQUIRED={
 "common_contracts","figma_regression","motion_native","audio_native","av_native_mux_decode_sync",
 "security_targeted","skill_packages","development_package","required_repository_checks"
}

class Invalid(ValueError): pass
def need(ok,msg):
    if not ok: raise Invalid(msg)
def sha(value,label):
    need(isinstance(value,str) and SHA.fullmatch(value),f"{label} must be a full lowercase Git SHA")

def validate(value:dict)->dict:
    need(isinstance(value,dict),"candidate evidence root must be an object")
    need(value.get("schema_version")==1,"schema_version must be 1")
    for field in ("candidate_sha","composition_sha","audio_sha","composition_composition_c0_sha"): sha(value.get(field),field)
    need(value.get("composition_composition_c0_sha")=="26602e4b25929be869d69ef28fef4dd9713180d7","unexpected C0 contract SHA")
    for field in ("combined_candidate","audio_ready_for_integration","ready_for_demo_production",
                  "figma_required_for_workflow","r16_closed","promotional_video_created"):
        need(isinstance(value.get(field),bool),f"{field} must be boolean")
    need(value["r16_closed"] is False,"this mission cannot close R16")
    need(value["promotional_video_created"] is False,"this mission cannot create the promotional video")
    for field in ("ready_for","not_ready_for"):
        need(isinstance(value.get(field),list) and len(value[field])<=64,f"{field} must be bounded list")
        need(all(isinstance(x,str) and 0<len(x)<=128 for x in value[field]),f"{field} entries invalid")
    gates=value.get("required_gates")
    need(isinstance(gates,dict) and set(gates)==REQUIRED,"required_gates set is incomplete or contains unknown gates")
    need(all(state in STATES for state in gates.values()),"invalid required gate state")
    evidence=value.get("workflow_evidence")
    need(isinstance(evidence,list) and len(evidence)<=128,"workflow_evidence must be bounded list")
    for index,row in enumerate(evidence):
        need(isinstance(row,dict),f"workflow_evidence[{index}] must be object")
        need(set(row)=={"workflow","run_id","job_ids","tested_sha","status","artifacts","limitations"},
             f"workflow_evidence[{index}] fields mismatch")
        need(isinstance(row["workflow"],str) and 0<len(row["workflow"])<=128,"workflow name invalid")
        need(isinstance(row["run_id"],int) and row["run_id"]>0,"run_id invalid")
        need(isinstance(row["job_ids"],list) and len(row["job_ids"])<=128 and all(isinstance(x,int) and x>0 for x in row["job_ids"]),"job_ids invalid")
        sha(row["tested_sha"],f"workflow_evidence[{index}].tested_sha")
        need(row["tested_sha"]==value["candidate_sha"],"workflow evidence mixes candidate SHAs")
        need(row["status"] in STATES,"workflow evidence status invalid")
        need(isinstance(row["artifacts"],list) and len(row["artifacts"])<=128,"artifact evidence invalid")
        need(isinstance(row["limitations"],list) and len(row["limitations"])<=64,"limitations invalid")
    if value["ready_for_demo_production"]:
        need(value["combined_candidate"],"READY requires a combined Composition+Audio candidate")
        need(value["audio_ready_for_integration"],"READY requires an audio-ready revision")
        for gate, state in gates.items():
            if gate == "figma_regression" and not value["figma_required_for_workflow"]:
                need(state in {"PASS","NOT_APPLICABLE"},"non-Figma workflow may only mark Figma PASS or NOT_APPLICABLE")
            else:
                need(state=="PASS",f"READY requires {gate} PASS")
        need(evidence,"READY requires exact-SHA workflow evidence")
        need(value["ready_for"],"READY requires an explicit workflow scope")
        need(not value["not_ready_for"],"READY cannot retain required workflow blockers")
        if value["figma_required_for_workflow"]:
            need(gates["figma_regression"]=="PASS","Figma-dependent READY requires Figma PASS")
    else:
        need(bool(value["not_ready_for"]) or any(s!="PASS" for s in gates.values()) or not value["combined_candidate"],
             "non-ready manifest must explain a blocker")
    return {
      "valid":True,
      "candidate_sha":value["candidate_sha"],
      "ready_for_demo_production":value["ready_for_demo_production"],
      "combined_candidate":value["combined_candidate"],
      "audio_ready_for_integration":value["audio_ready_for_integration"],
      "all_required_gates_pass":all(
          state=="PASS" or (
              gate=="figma_regression"
              and not value["figma_required_for_workflow"]
              and state=="NOT_APPLICABLE"
          )
          for gate,state in gates.items()
      ),
      "workflow_evidence_rows":len(evidence)
    }

def main():
    p=argparse.ArgumentParser()
    p.add_argument("manifest",type=Path)
    args=p.parse_args()
    try:
        result=validate(json.loads(args.manifest.read_text()))
    except (OSError,json.JSONDecodeError,Invalid) as exc:
        print(f"candidate evidence rejected: {exc}",file=sys.stderr); return 2
    print(json.dumps(result,indent=2,sort_keys=True)); return 0
if __name__=="__main__": raise SystemExit(main())
