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
