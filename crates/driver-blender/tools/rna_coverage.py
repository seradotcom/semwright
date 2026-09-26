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
    "ID": "abstract Blender ID base is classified through concrete persistent subclasses",
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

root_by_type = {}
root_classes = {}
roots = {}
property_status = {}
for root, identifier in sorted(semantic.ROOTS.items()):
    collection = getattr(bpy.data, root, None)
    candidate = getattr(bpy.types, identifier, None)
    rna = getattr(candidate, "bl_rna", None)
    if collection is None or rna is None:
        roots[root] = {"rna_type": identifier, "status": "unavailable_in_pinned_build"}
        continue
    actual_identifier = str(getattr(rna, "identifier", identifier))
    root_by_type[actual_identifier] = root
    root_classes[root] = candidate
    props = {}
    for prop in list(rna.properties):
        info = descriptor(prop)
        props[info["id"]] = info
    roots[root] = {"rna_type": actual_identifier, "declared_type": identifier, "status": "managed_root", "properties": props}

types = all_types()

subtypes = {}
for child_identifier, child in types.items():
    try:
        bases = child.__mro__[1:]
    except Exception:
        bases = ()
    for base in bases:
        base_rna = getattr(base, "bl_rna", None)
        base_identifier = str(getattr(base_rna, "identifier", "")) if base_rna is not None else ""
        if base_identifier and base_identifier in types:
            subtypes.setdefault(base_identifier, set()).add(child_identifier)

reachable_types = {}
missing_relation_targets = set()
pending = [identifier for identifier in sorted(root_by_type) if identifier in types]
seen = set()
while pending:
    identifier = pending.pop()
    if identifier in seen:
        continue
    seen.add(identifier)
    candidate = types.get(identifier)
    rna = getattr(candidate, "bl_rna", None)
    if rna is None:
        continue
    for subtype in sorted(subtypes.get(identifier, ())):
        if subtype not in seen:
            pending.append(subtype)
    props = {}
    relation_targets = {}
    for prop in list(rna.properties):
        info = descriptor(prop)
        props[info["id"]] = info
        property_status[info["status"]] = property_status.get(info["status"], 0) + 1
        if info["status"] != "relation":
            continue
        fixed = getattr(prop, "fixed_type", None)
        target = str(getattr(fixed, "identifier", "")) if fixed is not None else ""
        if target:
            relation_targets[info["id"]] = target
            if target in types and target not in seen:
                pending.append(target)
            elif target not in types:
                missing_relation_targets.add(target)
        else:
            relation_targets[info["id"]] = None
    base = getattr(rna, "base", None)
    reachable_types[identifier] = {
        "name": str(getattr(rna, "name", identifier))[:512],
        "base": str(getattr(base, "identifier", ""))[:256] if base else None,
        "properties": props,
        "relation_targets": relation_targets,
    }

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
    else:
        inherited_root = None
        for root, root_class in sorted(root_classes.items()):
            try:
                if candidate is not root_class and issubclass(candidate, root_class):
                    inherited_root = root
                    break
            except TypeError:
                pass
        if inherited_root is not None:
            entry = {"status": "managed_via_root", "root": inherited_root}
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
        "managed_via_root",
        "managed",
        "read_only",
        "relation",
        "runtime_owned",
        "unsupported_by_design",
        "unavailable_in_pinned_build",
    ],
    "roots": roots,
    "reachable_rna_types": reachable_types,
    "persistent_id_types": persistent_ids,
    "summary": {
        "managed_roots": sum(1 for item in roots.values() if item["status"] == "managed_root"),
        "unavailable_roots": sum(1 for item in roots.values() if item["status"] == "unavailable_in_pinned_build"),
        "persistent_id_types": len(persistent_ids),
        "unmapped_persistent_ids": len(unmapped),
        "unmapped_persistent_id_names": unmapped,
        "reachable_rna_types": len(reachable_types),
        "missing_relation_targets": sorted(missing_relation_targets),
        "property_status": dict(sorted(property_status.items())),
    },
}
output.parent.mkdir(parents=True, exist_ok=True)
output.write_text(json.dumps(payload, sort_keys=True, indent=2) + "\n")
print(json.dumps(payload["summary"], sort_keys=True))
