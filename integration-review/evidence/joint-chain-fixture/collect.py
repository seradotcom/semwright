"""Collect the six small native-chain artifacts after the hosted workflow succeeds."""
import hashlib
import io
import json
from pathlib import Path
import subprocess
import time
import zipfile

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "I_JOINT_CHAIN_cd51874.json"
state = json.loads(MANIFEST.read_text())
SOURCE = state["source_sha"]
SUITE = state["suite_sha"]
RUN = state["run_id"]


def api(path, raw=False):
    for attempt in range(6):
        try:
            value = subprocess.check_output(["gh", "api", "repos/seradotcom/semwright/" + path])
            return value if raw else json.loads(value)
        except subprocess.CalledProcessError:
            if attempt == 5:
                raise
            time.sleep(min(2 ** (attempt + 1), 15))


prior = None
while True:
    run = api(f"actions/runs/{RUN}")
    assert run["head_sha"] == SUITE
    current = (run["status"], run["conclusion"])
    if current != prior:
        print("Shared-asset chain", RUN, current, flush=True)
        prior = current
        state["workflow_status"], state["conclusion"] = current
        MANIFEST.write_text(json.dumps(state, indent=2) + "\n")
    if run["status"] == "completed":
        break
    time.sleep(30)
if run["conclusion"] != "success":
    state["status"] = "FAILED_REVIEW_REQUIRED"
    MANIFEST.write_text(json.dumps(state, indent=2) + "\n")
    raise SystemExit("Failure preserved; no automatic native rerun")
artifacts = api(f"actions/runs/{RUN}/artifacts")["artifacts"]
if state.get("reused_e_run_id"):
    assert len(artifacts) == 4
    producers = json.loads((ROOT / "I_JOINT_CHAIN_E_PRODUCERS_cd51874.json").read_text())
    assert producers["run_id"] == state["reused_e_run_id"] and producers["producer_jobs_passed"]
    for producer in producers["artifacts"]:
        meta = api(f'actions/artifacts/{producer["artifact_id"]}')
        assert meta["digest"] == "sha256:" + producer["zip_sha256"]
        assert meta["workflow_run"]["id"] == state["reused_e_run_id"]
        artifacts.append(meta)
assert len(artifacts) == 6
verified = {}
for meta in artifacts:
    assert not meta["expired"] and meta["size_in_bytes"] < 1_000_000
    raw = api(f'actions/artifacts/{meta["id"]}/zip', raw=True)
    digest = hashlib.sha256(raw).hexdigest()
    assert meta["digest"] == "sha256:" + digest
    z = zipfile.ZipFile(io.BytesIO(raw))
    docs = {n: json.loads(z.read(n)) for n in z.namelist() if n.endswith(".json")}
    item = {"artifact_id": meta["id"], "name": meta["name"], "zip_sha256": digest,
            "artifact_run_id": meta.get("workflow_run", {}).get("id", RUN),
            "files_sha256": {n: hashlib.sha256(z.read(n)).hexdigest() for n in z.namelist() if not n.endswith("/")}}
    if meta["name"].startswith("joint-e-"):
        identity = docs["identity.json"]
        assert identity["source_sha"] == SOURCE and identity["suite_sha"] == state.get("reused_e_suite_sha", SUITE)
        assert identity["run_id"] == str(state.get("reused_e_run_id", RUN)) and identity["same_managed_e_island"] is True
        assert "test result: ok. 1 passed; 0 failed; 0 ignored;" in z.read("native.log").decode()
        for name, field in [("initial-articulated", "initial_glb_sha256"), ("articulated", "revised_glb_sha256")]:
            expected = identity[field]
            assert hashlib.sha256(z.read(name + ".glb")).hexdigest() == expected
            oracle = docs[name + "-oracle.json"]
            assert oracle["source_sha256"] == expected and oracle["result"] == "PASS"
            assert all(oracle["required_checks"].values())
        assert identity["initial_glb_sha256"] != identity["revised_glb_sha256"]
        pipeline = docs["native-pipeline.json"]
        assert pipeline["writer_process"] != pipeline["reader_process"]
        assert pipeline["native_assertions_completed"] and pipeline["native_authoring_c_f_verified"]
        item["identity"] = identity
        worker = identity["worker"]
        kind = "E"
    elif meta["name"].startswith("joint-d-"):
        worker = meta["name"].removeprefix("joint-d-")
        d = docs["joint-d.json"]
        assert d["source_sha"] == SOURCE and d["suite_sha"] == SUITE
        assert "test result: ok. 1 passed; 0 failed; 0 ignored;" in z.read("native.log").decode()
        assert all(d[k] for k in ["native_asset_revision_verified", "behavior_preserved", "collision_mapping_preserved",
                                  "fresh_persistence_verified", "standalone_export_launch_verified"])
        assert not d["coverage_complete"] and not d["godot_frame_capture_verified"] and not d["r16_closed"]
        item["native_revision"] = d
        for name in ["initial-receipt.json", "revised-receipt.json"]:
            receipt = docs[name]
            assert receipt["operation"]["capability"] == "driver.godot.composition.apply"
            assert receipt["coverage"]["complete"] is False
        kind = "D"
    else:
        assert meta["name"].startswith("i-chain-av-")
        identity = docs["run.json"]
        assert identity["candidate_sha"] == SOURCE and identity["suite_sha"] == SUITE
        worker = identity["worker"]
        d = docs["joint-native-revisions.json"]
        assert d["source_sha"] == SOURCE and d["suite_sha"] == SUITE
        assert "test result: ok. 1 passed; 0 failed; 0 ignored;" in z.read("combined-native.log").decode()
        assert all(d[k] for k in ["native_godot_transform_drives_motion", "existing_managed_project",
            "visual_revision_changes_pixels", "visual_revision_preserves_audio_digest", "audio_revision_changes_pcm",
            "audio_revision_visual_pixels_identical_after_conservative_rerender", "post_encode_audio_and_exhaustive_sync_both_revisions",
            "new_owned_publication_pointers", "original_publication_preserved"])
        assert not any(d[k] for k in ["coverage_complete", "safe_cache_reuse_claimed", "godot_frame_capture_verified", "r16_closed"])
        assert abs(d["initial_body_x"] - d["revised_body_x"]) > 0.1
        item["native_revision"] = d
        kind = "AV"
    assert worker in ["clean-1", "clean-2"] and (kind, worker) not in verified
    verified[kind, worker] = item
for worker in ["clean-1", "clean-2"]:
    e = verified["E", worker]["identity"]
    d = verified["D", worker]["native_revision"]
    av = verified["AV", worker]["native_revision"]
    assert d["e_initial_glb_sha256"] == e["initial_glb_sha256"]
    assert d["e_revised_glb_sha256"] == e["revised_glb_sha256"]
    assert d["initial_body_bounds_center"][0] == av["initial_body_x"]
    assert d["revised_body_bounds_center"][0] == av["revised_body_x"]
    assert verified["D", worker]["files_sha256"]["initial-native.json"] == av["initial_godot_observation_sha256"]
    assert verified["D", worker]["files_sha256"]["revised-native.json"] == av["revised_godot_observation_sha256"]
report = {"source_sha": SOURCE, "suite_sha": SUITE, "run_id": RUN, "conclusion": "success",
          "clean_chains_verified": 2, "artifacts": list(verified.values()),
          "route": "Same native Blender asset revision -> native Godot mesh-bounds readback -> typed Motion schematic -> native AV -> C receipts",
          "godot_frame_capture_verified": False, "coverage_complete": False, "safe_cache_reuse_claimed": False,
          "r16_closed": False}
if state.get("reused_e_run_id"):
    report["reused_e_producer_evidence"] = "I_JOINT_CHAIN_E_PRODUCERS_cd51874.json"
    report["reused_e_suite_sha"] = state["reused_e_suite_sha"]
    report["producer_scope"] = "Both original E jobs passed; their overall workflow failed only in the original D fixture. Those E artifacts retain their original run/suite identity."
(ROOT / "I_JOINT_CHAIN_VERIFIED_cd51874.json").write_text(json.dumps(report, indent=2) + "\n")
state["status"] = "PASS_TWO_CLEAN_SHARED_ASSET_CHAINS"
state["verified_report"] = "I_JOINT_CHAIN_VERIFIED_cd51874.json"
MANIFEST.write_text(json.dumps(state, indent=2) + "\n")
print("Two native shared-asset chains verified on", SOURCE, flush=True)
