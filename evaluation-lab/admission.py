"""Preflight for a future actual model run; performs no network or secret access.

The native smoke runner does not call this, and cannot produce MODEL_AGENT rows.
An arbitrary adapter is not certified just because the draft mentions one.
"""
import os
import evaluate


def admit_model_session(freeze, registry, adapter, actor_uid):
    evaluate.validate_freeze(freeze, registry)
    if actor_uid == os.getuid() or type(actor_uid) is not int or actor_uid <= 0:
        raise ValueError("Model actor must be isolated from controller UID")
    if adapter.get("authorization") != "USER_CONFIGURED_MODEL_ACCESS":
        raise ValueError("No authorized model adapter")
    if adapter.get("model_identity") != freeze["model_identity"]:
        raise ValueError("Model adapter differs from frozen model identity")
    if adapter.get("config_digest") != freeze["model_config_digest"]:
        raise ValueError("Model adapter configuration differs from freeze")
    if adapter.get("budget_authorized") is not True:
        raise ValueError("Model budget has not been authorized")
    if adapter.get("native_adapter_certificate") is not True:
        raise ValueError("Model transport/tool/usage collection not certified")
    return {"admitted": True, "model_identity": freeze["model_identity"],
            "actor_uid": actor_uid, "model_calls_made": 0}
