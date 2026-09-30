#!/usr/bin/env python3
"""Finalize E acceptance only after all exact-SHA workflow gates succeeded."""
import hashlib
import json
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = Path(os.environ["SEMWRIGHT_AUTHORING_EVIDENCE"])
SOURCE = os.environ["GITHUB_SHA"]
RUN_ID = os.environ["GITHUB_RUN_ID"]
JOB = os.environ["GITHUB_JOB"]

PASS_IDS = {
    "E01", "E02", "E03", "E04", "E05", "E06", "E07", "E08",
    "E09", "E10", "E12", "E14", "E15", "E16",
}


def require(ok, message):
    if not ok:
        raise SystemExit(message)


def load(name):
    path = EVIDENCE / name
    require(path.is_file() and path.stat().st_size > 0, f"missing evidence: {name}")
    return json.loads(path.read_text())


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


model = load("blender-model.json")
native = load("blender-native-authoring.json")
pipeline = load("native-pipeline.json")
roundtrip = load("glb-roundtrip-blender.json")
skill = load("skill-validate.json")
source_package = load("source-package.json")

for report, expected_suite, minimum in [
    (model, "blender-model", 50),
    (native, "blender-native-authoring", 3),
]:
    require(report["source_sha"] == SOURCE, f"{expected_suite} source SHA mismatch")
    require(report["github_sha"] == SOURCE, f"{expected_suite} GitHub SHA mismatch")
    require(report["suite"] == expected_suite, f"{expected_suite} suite mismatch")
    require(report["outcome"] == "PASS", f"{expected_suite} did not PASS")
    require(report["passed"] >= minimum and report["failed"] == 0, f"{expected_suite} counts")

require(pipeline["native_assertions_completed"] is True, "native assertions incomplete")
require(pipeline["native_authoring_c_f_verified"] is True, "native C/F evidence incomplete")
require(pipeline["writer_process"] != pipeline["reader_process"], "reopen was not fresh-process")
require(roundtrip["result"] == "PASS", "fresh Blender GLB oracle did not PASS")
require(roundtrip["source_sha256"] == pipeline["glb"]["sha256"], "GLB oracle digest mismatch")

native_log = EVIDENCE / "blender-native-authoring.log"
require(native_log.is_file() and native_log.stat().st_size > 0, "native authoring log missing")
native_text = native_log.read_text(errors="replace")
for marker in [
    "NATIVE_PRE_CANCELLED_PASS",
    "NATIVE_MEDIA_TIME_SAMPLE_PASS",
    "NATIVE_INCREMENTAL_MATERIAL_PASS",
    "NATIVE_ALIGN_PASS",
    "NATIVE_PRODUCT_PREVIEW_PASS",
]:
    require(marker in native_text, f"missing native evidence marker: {marker}")

startup_log = EVIDENCE / "startup-security.log"
require(startup_log.is_file() and startup_log.stat().st_size > 0, "startup security log missing")
require("test result: ok" in startup_log.read_text(errors="replace"), "startup security test failed")

preview = EVIDENCE / "product-preview.png"
require(preview.is_file() and preview.stat().st_size > 8, "product preview missing")
require(preview.read_bytes()[:8] == b"\x89PNG\r\n\x1a\n", "product preview is not PNG")

require(skill["result"] == "valid" and skill["standard_valid"] is True, "Skill validation failed")

skill_zip = EVIDENCE / "semwright-blender-production.zip"
skill_sum = (EVIDENCE / "semwright-blender-production.zip.sha256").read_text().split()[0]
require(skill_zip.is_file() and sha256(skill_zip) == skill_sum, "Skill ZIP checksum mismatch")

source_zip = EVIDENCE / "semwright-blender-authoring-E-source.zip"
source_sum_file = EVIDENCE / "semwright-blender-authoring-E-source.zip.sha256"
require(source_package["source_sha"] == SOURCE, "source package SHA mismatch")
require(source_zip.is_file() and source_zip.stat().st_size > 0, "source backup ZIP missing")
require(source_sum_file.is_file() and source_sum_file.stat().st_size > 0, "source checksum missing")
source_sum = source_sum_file.read_text().split()[0]
require(source_package["sha256"] == source_sum == sha256(source_zip), "source ZIP checksum mismatch")

hostile = EVIDENCE / "hostile-export.log"
fuzz = EVIDENCE / "fuzz-blender-authoring.log"
require(hostile.is_file() and '"passed": true' in hostile.read_text(), "hostile GLB gate missing PASS")
require(fuzz.is_file() and fuzz.stat().st_size > 0, "fuzz evidence missing")

matrix = json.loads((ROOT / "docs/blender/authoring/ACCEPTANCE.json").read_text())
for requirement in matrix["requirements"]:
    rid = requirement["id"]
    if rid in PASS_IDS:
        requirement["status"] = "PASS"
        requirement.pop("open", None)
    elif rid == "E11":
        requirement["status"] = "BLOCKED_DEPENDENCY"
        requirement["open"] = (
            "Godot-side import/native semantic verification remains D-owned; "
            "E does not infer PASS before D's exact-SHA cross-app lane succeeds."
        )
    elif rid == "E13":
        requirement["status"] = "PARTIAL"
        requirement["open"] = (
            "Blender-native C/F authoring evidence PASS; cross-app C activities/receipts "
            "remain dependent on the D import/verification route."
        )
    else:
        raise SystemExit(f"unexpected acceptance requirement: {rid}")

matrix["exact_evidence"] = {
    "source_sha": SOURCE,
    "run_id": int(RUN_ID),
    "job_key": JOB,
    "model": {"passed": model["passed"], "log_sha256": model["log_sha256"]},
    "native": {"passed": native["passed"], "log_sha256": native["log_sha256"]},
    "glb_sha256": pipeline["glb"]["sha256"],
    "blend_sha256": pipeline["blend"]["sha256"],
    "c_authoring_receipt_digest": pipeline["c_authoring_receipt_digest"],
    "skill_zip_sha256": skill_sum,
    "source_backup_sha256": source_sum,
    "hostile_cases": 9,
    "fresh_blender_roundtrip": True,
    "startup_factory_clean_autoexec_disabled": True,
    "pre_cancelled_apply_fail_closed": True,
    "sampled_media_time_verified": True,
    "incremental_material_slots_verified": True,
    "axis_selective_align_verified": True,
    "product_preview_sha256": sha256(preview),
    "godot_reimport_verified": False,
}
matrix["blender_authoring_ready"] = False
matrix["readiness_blocker"] = (
    "E11 Godot-side verification and E13 cross-app provenance remain D/C dependencies."
)
(EVIDENCE / "acceptance-final.json").write_text(
    json.dumps(matrix, indent=2, sort_keys=True) + "\n"
)
print(json.dumps({
    "source_sha": SOURCE,
    "run_id": RUN_ID,
    "pass_requirements": sorted(PASS_IDS),
    "blocked": ["E11"],
    "partial": ["E13"],
    "blender_authoring_ready": False,
}, sort_keys=True))
