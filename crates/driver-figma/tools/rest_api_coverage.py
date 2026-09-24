#!/usr/bin/env python3
import argparse
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
COVERAGE = ROOT / "docs" / "REST_API_COVERAGE.json"
CATALOG = ROOT / "src" / "rest_catalog.rs"
OPS = ROOT / "src" / "semantic_rest_ops.rs"
REST = ROOT / "src" / "rest.rs"

def operation_ids_from_openapi(path: pathlib.Path) -> set[str]:
    text = path.read_text(encoding="utf-8")
    return set(re.findall(r"(?m)^\s*operationId:\s*([A-Za-z0-9_]+)\s*$", text))

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--openapi")
    args = ap.parse_args()
    coverage = json.loads(COVERAGE.read_text(encoding="utf-8"))
    operations = coverage.get("operations", [])
    if coverage.get("operation_count") != len(operations) or len(operations) != 54:
        raise SystemExit("REST coverage must contain exactly the 54 pinned OpenAPI operations")
    extras = coverage.get("documented_extras", [])
    ids = [item["operation_id"] for item in operations]
    caps = [item["capability"] for item in operations]
    all_ids = ids + [item["operation_id"] for item in extras]
    all_caps = caps + [item["capability"] for item in extras]
    if len(set(all_ids)) != len(all_ids) or len(set(all_caps)) != len(all_caps):
        raise SystemExit("duplicate REST operationId or capability")
    catalog = CATALOG.read_text(encoding="utf-8")
    rust_ids = set(re.findall(r'operation_id:\s*"([^"]+)"', catalog))
    rust_caps = set(re.findall(r'capability:\s*"([^"]+)"', catalog))
    if rust_ids != set(all_ids) or rust_caps != set(all_caps):
        raise SystemExit("REST Rust catalog drifted from REST_API_COVERAGE.json")

    expected_rows = {
        (
            item["operation_id"],
            item["capability"],
            item["method"],
            item["path"],
            item["scope"],
            item["credential"],
        )
        for item in operations + extras
    }
    rust_rows = {
        (operation_id, capability, method, path, scope, credential)
        for operation_id, capability, method, path, scope, credential in re.findall(
            r'operation_id:\s*"([^"]+)",\s*'
            r'capability:\s*"([^"]+)",\s*'
            r'method:\s*"([^"]+)",\s*'
            r'path:\s*"([^"]+)",\s*'
            r'scope:\s*"([^"]*)",\s*'
            r'deprecated:\s*(?:true|false),\s*'
            r'credential:\s*"([^"]+)"',
            catalog,
            flags=re.S,
        )
    }
    if rust_rows != expected_rows:
        raise SystemExit(
            "REST metadata tuple drift: "
            f"missing={sorted(expected_rows-rust_rows)} "
            f"extra={sorted(rust_rows-expected_rows)}"
        )

    rest_text = REST.read_text(encoding="utf-8")
    dispatched = set(re.findall(r'"(cloud\.[^"]+)"\s*=>', rest_text))
    required_dispatch = set(all_caps)
    if not required_dispatch.issubset(dispatched):
        raise SystemExit(
            "REST dispatcher coverage gap: "
            f"missing={sorted(required_dispatch-dispatched)}"
        )

    ops_text = OPS.read_text(encoding="utf-8")
    advertised = set(re.findall(r'"(cloud\.[^"]+)"', ops_text))
    expected = set(all_caps) | {"cloud.status"}
    if advertised != expected:
        raise SystemExit(
            f"REST advertised capabilities drift: missing={sorted(expected-advertised)} "
            f"extra={sorted(advertised-expected)}"
        )
    if args.openapi:
        current = operation_ids_from_openapi(pathlib.Path(args.openapi))
        if current != set(ids):
            raise SystemExit(
                f"OpenAPI drift: missing={sorted(current-set(ids))} "
                f"removed={sorted(set(ids)-current)}"
            )
    print(
        "PASS "
        f"openapi_operations={len(ids)} "
        f"documented_extras={len(extras)} "
        f"advertised={len(expected)} "
        f"spec_sha={coverage['rest_api_spec_commit']}"
    )
    return 0

if __name__ == "__main__":
    sys.exit(main())
