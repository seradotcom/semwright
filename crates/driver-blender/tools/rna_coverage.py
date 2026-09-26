"""Generate deterministic Blender RNA coverage inventory inside the pinned Blender runtime."""
import importlib.util
import json
import sys
from pathlib import Path

import bpy

repo = Path(__file__).resolve().parents[3]
semantic_path = repo / "crates" / "driver-blender" / "src" / "semantic.py"
spec = importlib.util.spec_from_file_location("semwright_blender_semantic", semantic_path)
semantic = importlib.util.module_from_spec(spec)
spec.loader.exec_module(semantic)

try:
    marker = sys.argv.index("--")
    output = Path(sys.argv[marker + 1])
except (ValueError, IndexError):
    raise SystemExit("usage: blender ... --python rna_coverage.py -- OUTPUT.json")

EXCLUDED_IDS = {
    "Library": "external linked-library authority requires scoped asset/library semantics",
    "Screen": "interactive UI layout is not persistent scene authoring semantics",
    "Text": "text datablocks can contain executable Python and require a trusted code domain",
    "WindowManager": "ambient interactive runtime state is outside headless authoring semantics",
    "WorkSpace": "interactive workspace state is outside headless authoring semantics",
}


def all_types():
    candidates = {}
    root = getattr(bpy.types, "bpy_struct", None)
    pending = [root] if root is not None else []
    seen = set()
    while pending and len(seen) < 100000:
        candidate = pending.pop()
        if candidate in seen:
            continue
        seen.add(candidate)
        rna = getattr(candidate, "bl_rna", None)
        identifier = str(getattr(rna, "identifier", "")) if rna is not None else ""
        if identifier:
            candidates.setdefault(identifier, candidate)
        try:
            pending.extend(candidate.__subclasses__())
        except Exception:
            pass
    for name in dir(bpy.types):
        if name.startswith("_"):
            continue
        candidate = getattr(bpy.types, name, None)
        rna = getattr(candidate, "bl_rna", None)
        identifier = str(getattr(rna, "identifier", "")) if rna is not None else ""
        if identifier:
            candidates.setdefault(identifier, candidate)
    return candidates


def descriptor(prop):
    value = semantic._property_descriptor(prop)
    # Reasons are already bounded deterministic strings; omit human labels to reduce churn.
    return value

root_by_type = {rna: root for root, rna in semantic.ROOTS.items()}
roots = {}
property_status = {}
for root, identifier in sorted(semantic.ROOTS.items()):
    collection = getattr(bpy.data, root, None)
    candidate = getattr(bpy.types, identifier, None)
    rna = getattr(candidate, "bl_rna", None)
    if collection is None or rna is None:
        roots[root] = {"rna_type": identifier, "status": "unavailable_in_pinned_build"}
        continue
    props = {}
    for prop in list(rna.properties):
        info = descriptor(prop)
        props[info["id"]] = info
        property_status[info["status"]] = property_status.get(info["status"], 0) + 1
    roots[root] = {"rna_type": identifier, "status": "managed_root", "properties": props}

types = all_types()
id_base = getattr(bpy.types, "ID", None)
persistent_ids = {}
unmapped = []
for identifier, candidate in sorted(types.items()):
    try:
        is_id = id_base is not None and issubclass(candidate, id_base)
    except TypeError:
        is_id = False
    if not is_id:
        continue
    if identifier in root_by_type:
        entry = {"status": "managed_root", "root": root_by_type[identifier]}
    elif identifier in EXCLUDED_IDS:
        entry = {"status": "unsupported_by_design", "reason": EXCLUDED_IDS[identifier]}
    else:
        entry = {"status": "unmapped_persistent_id", "reason": "persistent Blender ID not yet assigned to a managed root or explicit boundary"}
        unmapped.append(identifier)
    persistent_ids[identifier] = entry

payload = {
    "schema": 1,
    "package": "Blender RNA",
    "blender_version": bpy.app.version_string,
    "blender_version_tuple": list(bpy.app.version[:3]),
    "semantic_schema": "blender-rna-semantic/v1",
    "classification_contract": [
        "managed_root",
        "read_only",
        "relation",
        "runtime_owned",
        "unsupported_by_design",
        "unavailable_in_pinned_build",
    ],
    "roots": roots,
    "persistent_id_types": persistent_ids,
    "summary": {
        "managed_roots": sum(1 for item in roots.values() if item["status"] == "managed_root"),
        "unavailable_roots": sum(1 for item in roots.values() if item["status"] == "unavailable_in_pinned_build"),
        "persistent_id_types": len(persistent_ids),
        "unmapped_persistent_ids": len(unmapped),
        "unmapped_persistent_id_names": unmapped,
        "property_status": dict(sorted(property_status.items())),
    },
}
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(payload, sort_keys=True, indent=2) + "\n")
print(json.dumps(payload["summary"], sort_keys=True))
