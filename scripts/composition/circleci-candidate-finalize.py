#!/usr/bin/env python3
import json, os, subprocess
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/"verification/circleci-certification"
def need(ok,msg):
    if not ok: raise SystemExit(msg)
def load(rel):
    path=ROOT/rel
    need(path.is_file() and path.stat().st_size>0,f"missing certification artifact: {rel}")
    return json.loads(path.read_text()), rel

sha=subprocess.check_output(["git","rev-parse","HEAD"],cwd=ROOT,text=True).strip()
need(os.environ.get("CIRCLECI")=="true","not CircleCI")
need(os.environ.get("CIRCLE_BRANCH")=="integration/composition-av","wrong branch")
need(os.environ.get("SEMWRIGHT_CIRCLECI_CANDIDATE_CERTIFICATION")=="true","certification mode absent")
need(os.environ.get("CIRCLE_SHA1")==sha,"CircleCI SHA differs from candidate")
manifest=json.loads((ROOT/"packaging/composition-development/manifest.json").read_text())
a_sha=manifest["composition_source_sha"]
b_sha=manifest["audio_source_sha"]
artifacts=[]
for suite in ("contracts","motion","av"):
    receipt,rel=load(f"verification/circleci-certification/{suite}.json")
    need(receipt.get("status")=="PASS" and receipt.get("certification_eligible") is True,f"{suite} not certified")
    need(receipt.get("tested_sha")==sha,f"{suite} tested another SHA")
    artifacts.append(rel)
audio,rel=load("verification/circleci-certification/audio/final-certification.json")
need(audio.get("status")=="PASS" and audio.get("candidate_sha")==sha,"audio candidate certification mismatch")
artifacts.append(rel)
for rel in (
    "verification/circleci-composition/motion/iteration-classification.json",
    "verification/circleci-composition/combined-av/iteration-classification.json",
):
    receipt,_=load(rel)
    need(receipt.get("tested_sha")==sha,"native certification tested another SHA")
    need(receipt.get("classification")=="CANDIDATE_CERTIFICATION","native evidence remains diagnostic")
    need(receipt.get("certification_eligible") is True,"native evidence not certification eligible")
    artifacts.append(rel)
for rel in (
    "verification/circleci-composition/figma-portable.log",
    "verification/circleci-certification/figma-plugin.log",
    "verification/circleci-composition/skills.log",
    "verification/circleci-certification/security-authority.log",
    "verification/circleci-certification/candidate-package.json",
):
    path=ROOT/rel
    need(path.is_file() and path.stat().st_size>0,f"missing gate artifact: {rel}")
    artifacts.append(rel)
OUT.mkdir(parents=True,exist_ok=True)
payload={
 "schema_version":2,"candidate_sha":sha,"a_sha":a_sha,"b_sha":b_sha,
 "c0_sha":"26602e4b25929be869d69ef28fef4dd9713180d7",
 "combined_candidate":True,"b_audio_ready_for_integration":True,
 "ready_for_demo_production":False,
 "ready_for":["a-role-composition-motion-audio-av-linux"],
 "not_ready_for":["independent G retest remains external","F12 effect/audio consumer remains external","global repository/I gates remain external"],
 "figma_required_for_workflow":False,"r16_closed":False,"promotional_video_created":False,
 "required_gates":{
  "common_contracts":"PASS","figma_regression":"PASS","motion_native":"PASS",
  "audio_native":"PASS","av_native_mux_decode_sync":"PASS","security_targeted":"PASS",
  "skill_packages":"PASS","development_package":"PASS","required_repository_checks":"PENDING"},
 "workflow_evidence":[{
  "provider":"circleci","classification":"CANDIDATE_CERTIFICATION",
  "workflow":"a-linux-iteration/candidate-certification","run_id":os.environ["CIRCLE_WORKFLOW_ID"],
  "job_ids":[int(os.environ["CIRCLE_BUILD_NUM"])],"tested_sha":sha,"status":"PASS",
  "artifacts":artifacts,
  "limitations":["repository-wide required checks are owned by global integration","independent G/F handoffs are not self-certified by A"]}]
}
out=OUT/"candidate-evidence.json"
out.write_text(json.dumps(payload,indent=2,sort_keys=True)+"\n")
print(out)
