#!/usr/bin/env python3
"""Structural A/B/C benchmark over Driver Protocol v2 + repository fake Figma."""
from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
E2E = ROOT / "crates" / "driver-figma" / "tools" / "e2e_driver.py"

module_spec = importlib.util.spec_from_file_location("figma_e2e", E2E)
if module_spec is None or module_spec.loader is None:
    raise SystemExit("could not load Figma e2e helpers")
e2e = importlib.util.module_from_spec(module_spec)
module_spec.loader.exec_module(e2e)

def normalized_spec():
    spec = json.loads((HERE / "semwright-landing.composition.json").read_text())
    by_id = {node["id"]: node for node in spec["nodes"]}
    for relation in spec["relationships"]:
        if relation["kind"] == "minimum_gap":
            subject, obj = by_id[relation["subject"]], by_id[relation["object"]]
            if subject.get("parent") and subject.get("parent") == obj.get("parent"):
                parent = by_id[subject["parent"]]
                if parent.get("layout"):
                    parent["layout"]["gap"] = relation["value"]
    return spec

def layout_mode(node):
    if node["kind"] == "stack":
        return "VERTICAL"
    if node["kind"] in {"row", "split"}:
        return "HORIZONTAL"
    if node["kind"] == "grid":
        return "GRID"
    direction = (node.get("layout") or {}).get("direction", "none")
    return {"vertical": "VERTICAL", "horizontal": "HORIZONTAL", "grid": "GRID"}.get(
        direction, "NONE"
    )

def topological(spec):
    by_id = {node["id"]: node for node in spec["nodes"]}
    cache = {}
    def depth(node_id):
        if node_id not in cache:
            parent = by_id[node_id].get("parent")
            cache[node_id] = 0 if parent is None else depth(parent) + 1
        return cache[node_id]
    return sorted(spec["nodes"], key=lambda n: (depth(n["id"]), n["order"], n["id"]))
class Session:
    def __init__(self):
        self.driver = None
        self.fake = None
        self.caps = {}
        self.session_id = ""
        self.calls = 0

    def start(self):
        if not e2e.DRIVER.exists() or not e2e.FAKE.exists():
            raise SystemExit("build semwright-figma-driver and semwright-fake-figma first")
        env = os.environ.copy()
        env["SEMWRIGHT_FIGMA_BRIDGE_PORT"] = "0"
        self.driver = subprocess.Popen(
            [str(e2e.DRIVER)], cwd=ROOT, stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
        )
        e2e.request(self.driver, {
            "type": "hello", "protocol": 2,
            "provider": {
                "id": "driver:figma", "kind": "driver",
                "version": e2e.DRIVER_VERSION, "namespace": "driver.figma.",
                "application": None, "origin": "figma-semantic-benchmark",
            },
            "executable_sha256": "0" * 64,
        }, "ready")
        catalog = e2e.request(
            self.driver, {"type": "capabilities", "id": "caps"}, "capabilities"
        )
        self.caps = {c["descriptor"]["name"]: c for c in catalog["capabilities"]}
        pairing = e2e.execute(
            self.driver, self.caps, "driver.figma.pairing.begin", {}, "pairing"
        )
        fake_env = os.environ.copy()
        fake_env["SEMWRIGHT_FIGMA_PAIRING_SECRET"] = pairing["value"]["pairing_code"]
        port = pairing["value"]["listen_port"]
        self.fake = subprocess.Popen(
            [str(e2e.FAKE), f"ws://127.0.0.1:{port}"], cwd=ROOT,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=fake_env,
        )
        for _ in range(80):
            result = e2e.execute(
                self.driver, self.caps, "driver.figma.session.list", {}, "sessions"
            )
            if result["type"] == "result" and result["value"]:
                self.session_id = result["value"][0]["session_id"]
                return
            time.sleep(0.05)
        raise AssertionError("fake Figma pairing timed out")

    def revision(self):
        result = e2e.execute(
            self.driver, self.caps, "driver.figma.session.list", {}, "revision"
        )
        return int(result["value"][0]["revision"])

    def call(self, capability, args, label):
        self.calls += 1
        result = e2e.execute(
            self.driver, self.caps, capability, args, f"{label}-{self.calls}"
        )
        if result["type"] != "result":
            raise AssertionError((capability, result))
        return result["value"]

    def close(self):
        if self.driver is not None and self.driver.poll() is None:
            try:
                e2e.request(self.driver, {"type": "shutdown", "id": "bye"}, "shutdown")
                self.driver.wait(timeout=3)
            except Exception:
                self.driver.terminate()
        if self.fake is not None and self.fake.poll() is None:
            self.fake.terminate()
            try:
                self.fake.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.fake.kill()

def measure(session, roots):
    revision = session.revision()
    nodes = []
    for index, root in enumerate(roots):
        value = session.call(
            "driver.figma.composition.measure",
            {"session_id": session.session_id, "expected_revision": revision,
             "root_node_id": root, "max_nodes": 512},
            f"measure-{index}",
        )
        nodes.extend(value["nodes"])
    return nodes

def metrics(measured, spec):
    expected_text = sum(n["kind"] == "text" for n in spec["nodes"])
    expected_auto = sum(layout_mode(n) != "NONE" for n in spec["nodes"])
    native_text = sum(n["type"] == "TEXT" for n in measured)
    auto = sum(n.get("layoutMode") not in {None, "NONE"} for n in measured)
    return {
        "observed_nodes": len(measured),
        "native_text_nodes": native_text,
        "native_text_percentage_of_expected_copy": round(
            native_text * 100 / expected_text, 2
        ),
        "auto_layout_nodes": auto,
        "auto_layout_percentage_of_expected_containers": round(
            auto * 100 / expected_auto, 2
        ),
    }

def svg_payload(spec):
    lines = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="1900" height="4200">',
        '<rect width="1900" height="4200" fill="#F4EFE6"/>',
    ]
    y = 50
    for node in spec["nodes"]:
        if node["kind"] != "text":
            continue
        copy_text = node["text"]["characters"]
        escaped = copy_text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
        lines.append(
            f'<text x="50" y="{y}" font-family="Inter" font-size="18">{escaped}</text>'
        )
        y += 42
    lines.append("</svg>")
    return "".join(lines)

def code_first_arm(spec):
    s = Session()
    s.start()
    try:
        start = s.calls
        root = s.call(
            "driver.figma.svg.import",
            {"session_id": s.session_id, "expected_revision": 0,
             "svg": svg_payload(spec)},
            "svg-import",
        )["id"]
        measured = measure(s, [root])
        result = metrics(measured, spec)
        result.update({
            "arm": "A_code_first_svg",
            "authoring_calls": s.calls - start,
            "giant_imported_svg_structures": 1,
            "semantic_contract_validation": "NOT_APPLICABLE",
            "follow_up_semantic_edit": "REIMPORT_OR_LOW_LEVEL_RECONSTRUCTION_REQUIRED",
            "follow_up_identity_preserved": False,
            "visual_quality": "UNEXECUTED_FAKE_RUNTIME",
        })
        return result
    finally:
        s.close()

def fixed(node, key):
    value = (node.get("sizing") or {}).get(key) or {}
    return value.get("value") if value.get("mode") == "fixed" else None

def low_level_arm(spec):
    s = Session()
    s.start()
    try:
        start = s.calls
        ids = {}
        for node in topological(spec):
            revision = s.revision()
            if node["kind"] == "text":
                cap = "driver.figma.text.create"
                args = {
                    "session_id": s.session_id, "expected_revision": revision,
                    "name": node["name"], "characters": node["text"]["characters"],
                }
            elif node["kind"] == "shape":
                cap = "driver.figma.rect.create"
                args = {
                    "session_id": s.session_id, "expected_revision": revision,
                    "name": node["name"],
                }
            else:
                cap = "driver.figma.frame.create"
                args = {
                    "session_id": s.session_id, "expected_revision": revision,
                    "name": node["name"],
                }
            created = s.call(cap, args, f"create-{node['id']}")
            ids[node["id"]] = created["id"]

            width, height = fixed(node, "width"), fixed(node, "height")
            if width is not None or height is not None:
                revision = s.revision()
                s.call(
                    "driver.figma.node.resize",
                    {"session_id": s.session_id, "expected_revision": revision,
                     "nodeId": created["id"], "width": width or 100, "height": height or 100},
                    f"resize-{node['id']}",
                )
            if node.get("parent") is not None:
                revision = s.revision()
                s.call(
                    "driver.figma.node.reparent",
                    {"session_id": s.session_id, "expected_revision": revision,
                     "nodeId": created["id"], "parentId": ids[node["parent"]]},
                    f"reparent-{node['id']}",
                )
            mode = layout_mode(node)
            if mode != "NONE":
                info = node.get("layout") or {}
                padding = info.get("padding") or {}
                revision = s.revision()
                s.call(
                    "driver.figma.layout.patch",
                    {
                        "session_id": s.session_id, "expected_revision": revision,
                        "nodeId": created["id"], "layoutMode": mode,
                        "itemSpacing": info.get("gap", 0),
                        "paddingTop": padding.get("top", 0),
                        "paddingRight": padding.get("right", 0),
                        "paddingBottom": padding.get("bottom", 0),
                        "paddingLeft": padding.get("left", 0),
                    },
                    f"layout-{node['id']}",
                )

        measured = measure(s, [ids["desktop"], ids["mobile"]])
        heading_id = ids["d-heading"]
        revision = s.revision()
        s.call(
            "driver.figma.text.patch",
            {"session_id": s.session_id, "expected_revision": revision,
             "nodeId": heading_id,
             "characters": "Native edits preserve semantic object identity."},
            "follow-up-heading",
        )
        followup = measure(s, [ids["desktop"]])
        preserved = any(
            n["nodeId"] == heading_id
            and (n.get("text") or {}).get("characters")
            == "Native edits preserve semantic object identity."
            for n in followup
        )
        result = metrics(measured, spec)
        result.update({
            "arm": "B_existing_low_level",
            "authoring_calls": s.calls - start,
            "giant_imported_svg_structures": 0,
            "semantic_contract_validation": "NOT_APPLICABLE_NO_LOGICAL_BINDINGS",
            "follow_up_semantic_edit": "EXACT_NODE_ID_PLUS_TEXT_PATCH",
            "follow_up_identity_preserved": preserved,
            "visual_quality": "UNEXECUTED_FAKE_RUNTIME",
        })
        return result
    finally:
        s.close()

def semantic_arm(spec):
    s = Session()
    s.start()
    try:
        start = s.calls
        plan = s.call(
            "driver.figma.composition.plan",
            {"session_id": s.session_id, "expected_revision": 0, "spec": spec},
            "semantic-plan",
        )
        applied = s.call(
            "driver.figma.composition.apply",
            {"session_id": s.session_id, "expected_revision": 0, "plan": plan},
            "semantic-apply",
        )
        roots = applied["rootNodeIds"]
        measured = measure(s, roots)
        revision = s.revision()
        statuses = []
        for index, root in enumerate(roots):
            validation = s.call(
                "driver.figma.composition.validate",
                {"session_id": s.session_id, "expected_revision": revision,
                 "root_node_id": root, "spec": spec, "max_findings": 128},
                f"semantic-validate-{index}",
            )
            statuses.append(validation["status"])

        parent_id = applied["logicalToNode"]["d-hero-copy"]
        heading_id = applied["logicalToNode"]["d-heading"]
        update_spec = {
            "version": 1,
            "target": {"page_id": None, "parent_node_id": None},
            "nodes": [
                {
                    "id": "d-hero-copy", "kind": "stack",
                    "name": "Hero / Editorial copy", "parent": None,
                    "existing_node_id": parent_id, "order": 0, "role": "hero-copy",
                    "layout": {"direction": "vertical", "gap": 28},
                },
                {
                    "id": "d-heading", "kind": "text",
                    "name": "Hero / Heading", "parent": "d-hero-copy",
                    "existing_node_id": heading_id, "order": 0, "role": "heading",
                    "text": {
                        "characters": "Native edits preserve semantic object identity.",
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
        revision = s.revision()
        update_plan = s.call(
            "driver.figma.composition.plan",
            {"session_id": s.session_id, "expected_revision": revision,
             "spec": update_spec},
            "semantic-update-plan",
        )
        updated = s.call(
            "driver.figma.composition.apply",
            {"session_id": s.session_id, "expected_revision": revision,
             "plan": update_plan},
            "semantic-update-apply",
        )
        preserved = (
            updated["logicalToNode"]["d-heading"] == heading_id
            and updated["created"] == []
        )
        result = metrics(measured, spec)
        result.update({
            "arm": "C_semantic_authoring",
            "authoring_calls": s.calls - start,
            "giant_imported_svg_structures": 0,
            "semantic_contract_validation": (
                "PASS" if statuses and all(status == "PASS" for status in statuses)
                else statuses
            ),
            "follow_up_semantic_edit": "REVISION_BOUND_PLAN_APPLY",
            "follow_up_identity_preserved": preserved,
            "visual_quality": "UNEXECUTED_FAKE_RUNTIME",
        })
        return result
    finally:
        s.close()

def main():
    spec = normalized_spec()
    expected = {
        "nodes": len(spec["nodes"]),
        "text_nodes": sum(n["kind"] == "text" for n in spec["nodes"]),
        "auto_layout_containers": sum(layout_mode(n) != "NONE" for n in spec["nodes"]),
    }
    result = {
        "schema_version": 1,
        "benchmark": "semwright_figma_semantic_authoring_structural_v1",
        "environment": "Driver Protocol v2 + repository fake-Figma runtime",
        "brief": "Semwright launch landing desktop + mobile",
        "expected": expected,
        "arms": [code_first_arm(spec), low_level_arm(spec), semantic_arm(spec)],
        "visual_quality": "UNEXECUTED",
        "visual_quality_reason": (
            "Fake Figma proves protocol/structure semantics, not live rendering quality."
        ),
        "interpretation_rules": {
            "no_winner_score": True,
            "tool_calls_are_not_visual_quality": True,
            "real_figma_required_for_visual_claims": True,
            "code_first_svg_is_modeled_as_an_opaque_import_in_fake_figma": True,
        },
    }
    output = Path(os.environ.get(
        "SEMWRIGHT_FIGMA_BENCHMARK_OUT",
        ROOT / "verification" / "native-ci" / "figma-semantic-benchmark.json",
    ))
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps(result, sort_keys=True))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
