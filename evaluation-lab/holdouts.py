"""Procedural reserved-task commitments, with explicit confidentiality limits.

This never discovers credentials, uploads private parameters, or labels draft
reservations a final frozen study. A same-user assistant can read private files;
the seal is an integrity commitment, not proof of cryptographic secrecy.
"""
import argparse
from copy import deepcopy
import hashlib
import json
import os
from pathlib import Path
import random
import secrets

import evaluate


def reserve(registry, *, round_id, seed):
    evaluate.validate_registry(registry)
    if registry["split"] != "public_dev":
        raise ValueError("Reservation needs declared public development templates")
    if not round_id or type(seed) is not int:
        raise ValueError("Round and integer reservation seed required")
    rng = random.Random(seed)
    heldout = deepcopy(registry)
    heldout["split"] = "heldout"
    for i, task in enumerate(heldout["tasks"]):
        task["id"] = "reserved-%s-%02d" % (round_id, i)
        p = task["parameters"]
        family = task["family"]
        if family == "godot":
            p["objective_count"] = rng.randint(7, 14)
            p["timer_seconds"] = rng.randint(80, 140)
        elif family == "blender":
            p["segments"] = rng.randint(8, 12)
            p["material_palette"] = rng.sample(["amber","slate","teal","ivory"], 2)
        elif family == "cross_app":
            p["replacement_scale"] = rng.randint(4, 7)
        elif family == "media":
            p["duration_seconds"] = rng.randint(5, 9)
            p["cue_count"] = rng.randint(4, 7)
            p["sample_rate"] = rng.choice([44100, 48000])
        elif family == "recovery":
            p["fault_sequence"] = rng.sample(task["declared_faults"], 3)
        else:
            raise ValueError("Unregistered reserved family")
    # The validator refuses duplicates within each family, even with random seeds.
    evaluate.validate_registry(heldout)
    public_parameters = {evaluate.digest(t["parameters"]) for t in registry["tasks"]}
    if any(evaluate.digest(t["parameters"]) in public_parameters for t in heldout["tasks"]):
        raise ValueError("Public development parameters reused as holdouts")
    return heldout


def commitment(payload):
    return hashlib.sha256(evaluate.canonical(payload)).hexdigest()


def seal(payload):
    evaluate.validate_registry(payload["registry"])
    if payload["registry"]["split"] != "heldout":
        raise ValueError("Cannot seal public development tasks")
    if not evaluate.SHA.fullmatch(payload.get("technical_product_sha", "")):
        raise ValueError("Full technical product identity required")
    if not evaluate.DIGEST.fullmatch(payload.get("nonce", "")) or not payload.get("round_id"):
        raise ValueError("Reservation nonce/round absent")
    return {"schema_version":1,"round_id":payload["round_id"],
            "technical_product_sha":payload["technical_product_sha"],
            "commitment_sha256":commitment(payload), "task_count":len(payload["registry"]["tasks"]),
            "state":"DRAFT_RESERVED_NOT_FINAL_EVALUATION_FREEZE",
            "confidentiality":"Procedural reservation only; no secrecy claim against the same user/assistant",
            "parameters_revealed":False,"model_protocol_frozen":False}


def reveal(payload, public_seal, *, consumed_rounds):
    expected = seal(payload)
    for key in ("round_id","technical_product_sha","commitment_sha256","task_count"):
        if expected[key] != public_seal.get(key):
            raise ValueError("Seal/reveal identity or commitment mismatch")
    if payload["round_id"] in consumed_rounds:
        raise ValueError("Exposed/consumed round cannot be reused")
    return {"schema_version":1,"round_id":payload["round_id"],
            "commitment_sha256":expected["commitment_sha256"],"state":"REVEALED_REQUIRES_NEW_HOLDOUTS_FOR_NEXT_ROUND",
            "payload":payload,"model_evaluation_executed":False}


def exclusive_write(path, value, mode):
    # Never overwrite a previous reservation, reveal or valuable evidence.
    fd = os.open(path, os.O_WRONLY|os.O_CREAT|os.O_EXCL, mode)
    with os.fdopen(fd,"w") as stream:
        stream.write(json.dumps(value,indent=2)+"\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--public-registry", required=True)
    parser.add_argument("--private-directory", required=True)
    parser.add_argument("--public-seal", required=True)
    parser.add_argument("--product-sha", required=True)
    parser.add_argument("--round", required=True)
    args = parser.parse_args()
    private = Path(args.private_directory).resolve()
    repository = Path(__file__).resolve().parents[1]
    if private == repository or repository in private.parents:
        raise ValueError("Private parameters must remain outside the repository")
    private.mkdir(mode=0o700,parents=True,exist_ok=False)
    seed = secrets.randbits(128)
    registry = reserve(evaluate.load(args.public_registry),round_id=args.round,seed=seed)
    payload = {"schema_version":1,"round_id":args.round,"technical_product_sha":args.product_sha,
               "nonce":secrets.token_hex(32),"reservation_seed":seed,"registry":registry}
    public = seal(payload)
    exclusive_write(private/"reserved-parameters.json",payload,0o600)
    exclusive_write(args.public_seal,public,0o644)
    print("Draft reservation commitment",public["commitment_sha256"],"tasks",public["task_count"])


if __name__ == "__main__":
    main()
