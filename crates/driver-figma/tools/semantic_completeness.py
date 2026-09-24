#!/usr/bin/env python3
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
PLUGIN_COVERAGE = ROOT / "docs" / "API_COVERAGE.json"
REST_COVERAGE = ROOT / "docs" / "REST_API_COVERAGE.json"

def fail(message: str) -> None:
    raise SystemExit(message)

def classification_strings(value):
    if isinstance(value, dict):
        for child in value.values():
            yield from classification_strings(child)
    elif isinstance(value, list):
        for child in value:
            yield from classification_strings(child)
    elif isinstance(value, str):
        yield value

def main() -> int:
    plugin = json.loads(PLUGIN_COVERAGE.read_text(encoding="utf-8"))
    rest = json.loads(REST_COVERAGE.read_text(encoding="utf-8"))
    summary = plugin.get("summary", {})
    if summary.get("unclassified_auxiliary_methods") != 0:
        fail("unclassified auxiliary Plugin API methods remain")
    if summary.get("unmapped_method_names") != []:
        fail("unmapped scene-node Plugin API methods remain")
    if summary.get("scene_node_members") != summary.get("supported_scene_node_members"):
        fail("not every pinned scene-node member is semantically classified")

    generic = plugin.get("generic_property_surface", {})
    readable = set(generic.get("readable", []))
    writable = set(generic.get("writable", []))
    if "mainComponent" in readable or "mainComponent" in writable:
        fail("InstanceNode.mainComponent must use explicit async-read/ref-write semantics")
    if "stuckTo" in writable:
        fail("StickableMixin.stuckTo must use an explicit node-ref mutation")
    instance_main = (
        plugin.get("scene_node_types", {})
        .get("INSTANCE", {})
        .get("members", {})
        .get("mainComponent", {})
    )
    if (
        instance_main.get("status") != "SUPPORTED_SPECIAL_PROPERTY"
        or instance_main.get("read_capability") != "instance.inspect"
        or instance_main.get("write_capability") != "instance.main_component.set"
    ):
        fail("InstanceNode.mainComponent special semantic mapping drifted")
    for node_type in ("STAMP", "HIGHLIGHT", "WASHI_TAPE", "WIDGET"):
        stuck_to = (
            plugin.get("scene_node_types", {})
            .get(node_type, {})
            .get("members", {})
            .get("stuckTo", {})
        )
        if (
            stuck_to.get("status") != "SUPPORTED_SPECIAL_PROPERTY"
            or stuck_to.get("write_capability") != "figjam.stuck_to.set"
        ):
            fail(f"{node_type}.stuckTo special semantic mapping drifted")

    classifications = list(classification_strings(plugin))
    forbidden_fragments = (
        "UNCLASSIFIED",
        "UNMAPPED_METHOD",
        "INTERNAL_OR_POLICY_EXCLUDED",
        "POLICY_EXCLUDED",
    )
    forbidden = sorted({
        item for item in classifications
        if any(fragment in item for fragment in forbidden_fragments)
    })
    if forbidden:
        fail("semantic coverage contains unresolved policy/gap classifications: " + ", ".join(forbidden))

    payments = plugin.get("interfaces", {}).get("PaymentsAPI", {})
    if payments.get("getPluginPaymentTokenAsync") != "INTERNAL_SECRET_COMPOSITION":
        fail("Figma payment token must remain internal secret composition")
    if plugin.get("interfaces", {}).get("PluginAPI", {}).get("payments") != "NAMESPACE_MAPPED":
        fail("PluginAPI.payments must be semantically mapped")

    expected_rest = rest.get("operation_count")
    operations = rest.get("operations", [])
    extras = rest.get("documented_extras", [])
    if expected_rest != 54 or len(operations) != 54:
        fail("pinned REST surface must contain all 54 OpenAPI operations")
    if len(extras) != 1:
        fail("REST semantic extras changed without completeness review")
    upstream_partner = sum(
        1 for item in classifications if item == "UPSTREAM_PARTNER_RESTRICTED"
    )
    upstream_widget = sum(
        1 for item in classifications if item == "UPSTREAM_WIDGET_CONTEXT_RESTRICTED"
    )
    if upstream_partner != 34 or upstream_widget != 2:
        fail(
            "upstream-restricted surface changed; classify the new Figma contract explicitly "
            f"(partner={upstream_partner}, widget={upstream_widget})"
        )

    print(
        "PASS "
        f"typings={plugin.get('plugin_typings_version')} "
        f"global_members={summary.get('global_members')} "
        f"auxiliary_methods={summary.get('auxiliary_method_entries')} "
        f"scene_node_members={summary.get('scene_node_members')} "
        f"rest_operations={len(operations)} "
        f"rest_extras={len(extras)} "
        f"upstream_restricted={upstream_partner + upstream_widget} "
        "policy_gaps=0"
    )
    return 0

if __name__ == "__main__":
    sys.exit(main())
