#!/usr/bin/env python3
"""Exercise bounded semantic convergence against the repository fake-Figma runtime."""
from __future__ import annotations
import json
import os
from pathlib import Path

from benchmark import Session

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]

def main():
    spec = json.loads((HERE / "semwright-landing.composition.json").read_text())
    session = Session()
    session.start()
    try:
        plan = session.call(
            "driver.figma.composition.plan",
            {"session_id": session.session_id, "expected_revision": 0, "spec": spec},
            "plan",
        )
        applied = session.call(
            "driver.figma.composition.apply",
            {"session_id": session.session_id, "expected_revision": 0, "plan": plan},
            "apply",
        )
        desktop = applied["logicalToNode"]["desktop"]
        mobile = applied["logicalToNode"]["mobile"]
        heading = applied["logicalToNode"]["d-heading"]
        hero_copy = applied["logicalToNode"]["d-hero-copy"]
        validation = session.call(
            "driver.figma.composition.validate",
            {"session_id": session.session_id, "expected_revision": 1,
             "root_node_id": desktop, "spec": spec, "max_findings": 128},
            "validate",
        )
        spacing = [
            item for item in validation["findings"]
            if item["category"] == "declared_spacing"
        ]
        if validation["status"] != "FAIL" or len(spacing) != 1:
            raise AssertionError(validation)

        repair_plan = session.call(
            "driver.figma.composition.repair.plan",
            {"session_id": session.session_id, "expected_revision": 1,
             "plan": plan, "findings": validation["findings"]},
            "repair-plan",
        )
        repaired = session.call(
            "driver.figma.composition.repair.apply",
            {"session_id": session.session_id, "expected_revision": 1,
             "plan": repair_plan},
            "repair-apply",
        )
        if repaired["observedRevision"] != 2:
            raise AssertionError(repaired)

        revalidated = session.call(
            "driver.figma.composition.validate",
            {"session_id": session.session_id, "expected_revision": 2,
             "root_node_id": desktop, "spec": spec, "max_findings": 128},
            "revalidate",
        )
        if revalidated["status"] != "PASS":
            raise AssertionError(revalidated)

        progress = []
        # Use raw helper here only to capture JobArtifact progress metadata as evidence.
        import benchmark as bench
        verified = bench.e2e.execute(
            session.driver, session.caps, "driver.figma.composition.verify",
            {"session_id": session.session_id, "expected_revision": 2,
             "root_node_id": desktop, "spec": spec, "scale": 1,
             "name": "semwright-desktop-verification.png", "max_findings": 128},
            "verify", progress,
        )
        if verified["type"] != "result":
            raise AssertionError(verified)

        update_spec = {
            "version": 1,
            "target": {"page_id": None, "parent_node_id": None},
            "nodes": [
                {
                    "id": "d-hero-copy", "kind": "stack",
                    "name": "Hero / Editorial copy", "parent": None,
                    "existing_node_id": hero_copy, "order": 0, "role": "hero-copy",
                    "layout": {"direction": "vertical", "gap": 30},
                },
                {
                    "id": "d-heading", "kind": "text", "name": "Hero / Heading",
                    "parent": "d-hero-copy", "existing_node_id": heading,
                    "order": 0, "role": "heading",
                    "text": {
                        "characters": "Native semantic execution stays editable after creation.",
                        "fit": "grow_height",
                    },
                },
            ],
            "relationships": [], "profiles": [],
            "validators": [{"kind": "native_text", "severity": "error"}],
            "budgets": {
                "max_nodes": 8, "max_depth": 4, "max_relationships": 8,
                "max_findings_per_round": 8, "max_repair_operations": 4,
                "max_iterations": 2, "max_mutations": 8,
            },
        }
        update_plan = session.call(
            "driver.figma.composition.plan",
            {"session_id": session.session_id, "expected_revision": 2,
             "spec": update_spec},
            "update-plan",
        )
        updated = session.call(
            "driver.figma.composition.apply",
            {"session_id": session.session_id, "expected_revision": 2,
             "plan": update_plan},
            "update-apply",
        )
        if updated["logicalToNode"]["d-heading"] != heading:
            raise AssertionError("heading identity changed")
        if updated["logicalToNode"]["d-hero-copy"] != hero_copy:
            raise AssertionError("hero-copy identity changed")
        stale = bench.e2e.execute(
            session.driver, session.caps, "driver.figma.composition.apply",
            {"session_id": session.session_id, "expected_revision": 3, "plan": plan},
            "stale-plan",
        )
        if stale["type"] != "failure" or stale["error"]["code"] != "StaleReference":
            raise AssertionError(stale)

        result = {
            "schema_version": 1,
            "environment": "Driver Protocol v2 + repository fake-Figma runtime",
            "live_figma": "UNEXECUTED",
            "initial_revision": 0,
            "after_initial_apply": 1,
            "initial_validation": validation["status"],
            "detected_failure": spacing[0]["category"],
            "repair_operations": len(repair_plan["changeset"]["modifies"]),
            "after_repair_revision": 2,
            "revalidation": revalidated["status"],
            "verification_media_type": verified["value"]["mediaType"],
            "verification_artifact_reported": bool(progress),
            "desktop_root_node": desktop,
            "mobile_root_node": mobile,
            "post_creation_edit_revision": 3,
            "heading_identity_preserved": True,
            "hero_copy_identity_preserved": True,
            "stale_original_plan_rejected": True,
        }
        output = Path(os.environ.get(
            "SEMWRIGHT_FIGMA_DEMO_OUT",
            ROOT / "verification" / "native-ci" / "figma-semantic-demo.json",
        ))
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
        print(json.dumps(result, sort_keys=True))
        return 0
    finally:
        session.close()

if __name__ == "__main__":
    raise SystemExit(main())
