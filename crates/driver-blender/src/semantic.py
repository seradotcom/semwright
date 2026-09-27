"""Bounded RNA semantic substrate for the sandbox-owned Blender runtime.

This module deliberately exposes data, not Python execution. Mutable generic properties are
restricted to scalar/enum/numeric-array RNA values that do not carry paths, callbacks, pointers,
collections or executable text. Richer domains are layered on top with dedicated semantics.
"""
from __future__ import annotations

import base64
import json
import math
import os
import re
import stat
from pathlib import Path


MAX_NAME = 256
MAX_TEXT = 2048
MAX_ITEMS = 256
MAX_ARRAY = 32
REF_PREFIX = "blender-rna/v1"

# Owner-visible persistent authoring roots. Runtime/window-manager/preferences/text/script data are
# intentionally absent because they cross ambient UI/configuration or executable-code boundaries.
ROOTS = {
    "actions": "Action",
    "armatures": "Armature",
    "brushes": "Brush",
    "cache_files": "CacheFile",
    "cameras": "Camera",
    "collections": "Collection",
    "curves": "Curve",
    "fonts": "VectorFont",
    "grease_pencils": "GreasePencil",
    "grease_pencils_v3": "GreasePencilv3",
    "hair_curves": "Curves",
    "images": "Image",
    "lattices": "Lattice",
    "lights": "Light",
    "lightprobes": "LightProbe",
    "linestyles": "FreestyleLineStyle",
    "masks": "Mask",
    "materials": "Material",
    "meshes": "Mesh",
    "metaballs": "MetaBall",
    "movieclips": "MovieClip",
    "node_groups": "NodeTree",
    "objects": "Object",
    "paint_curves": "PaintCurve",
    "palettes": "Palette",
    "particles": "ParticleSettings",
    "pointclouds": "PointCloud",
    "scenes": "Scene",
    "shape_keys": "Key",
    "sounds": "Sound",
    "speakers": "Speaker",
    "textures": "Texture",
    "volumes": "Volume",
    "worlds": "World",
}

PATH_SUBTYPES = {"FILE_PATH", "DIR_PATH"}
EXECUTABLE_IDENTIFIERS = {
    "script",
    "expression",
    "driver_expression",
    "python",
    "code",
}
SENSITIVE_RELATIONS = {"driver", "drivers", "library", "script", "text"}
RUNTIME_RELATIONS = {"id_data", "original", "override_library", "library_weak_reference", "user"}
DOMAIN_SPECIFIC_POINTERS = {("Object", "data")}
SENSITIVE_STRING_FRAGMENTS = {"filepath", "directory", "filename", "url", "uri", "command", "module", "script", "expression"}


def _text(value, maximum=MAX_TEXT):
    return str(value or "").replace("\x00", "")[:maximum]


def _token(value):
    raw = value.encode("utf-8")
    return base64.urlsafe_b64encode(raw).decode("ascii").rstrip("=")


def _untoken(value):
    if not isinstance(value, str) or not value or len(value) > 512:
        raise ValueError("invalid ref token")
    padding = "=" * ((4 - len(value) % 4) % 4)
    raw = base64.urlsafe_b64decode((value + padding).encode("ascii"))
    decoded = raw.decode("utf-8")
    if not decoded or len(decoded) > MAX_NAME or "\x00" in decoded:
        raise ValueError("invalid ref name")
    return decoded


def _path_token(path):
    raw = json.dumps(path, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    if len(raw) > 512:
        raise ValueError("RNA relation path is too large")
    return base64.urlsafe_b64encode(raw).decode("ascii").rstrip("=")


def _path_from_token(value):
    padding = "=" * ((4 - len(value) % 4) % 4)
    raw = base64.urlsafe_b64decode((value + padding).encode("ascii"))
    if len(raw) > 512:
        raise ValueError("RNA relation path is too large")
    path = json.loads(raw.decode("utf-8"))
    if not isinstance(path, list) or len(path) > 8:
        raise ValueError("RNA relation path is invalid")
    for step in path:
        if not isinstance(step, list) or len(step) not in (2, 3):
            raise ValueError("RNA relation step is invalid")
        if step[0] not in ("p", "c") or not isinstance(step[1], str) or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", step[1]):
            raise ValueError("RNA relation step is invalid")
        if step[0] == "p" and len(step) != 2:
            raise ValueError("RNA pointer step is invalid")
        if step[0] == "c":
            if len(step) != 3 or isinstance(step[2], bool) or not isinstance(step[2], int) or not 0 <= step[2] <= 1_000_000:
                raise ValueError("RNA collection step is invalid")
    return path


def _property_status(prop):
    identifier = str(getattr(prop, "identifier", ""))
    ptype = str(getattr(prop, "type", ""))
    subtype = str(getattr(prop, "subtype", ""))
    if identifier == "rna_type":
        return "runtime_owned", "RNA type metadata is runtime-owned"
    if identifier.casefold() in SENSITIVE_RELATIONS or "script" in identifier.casefold():
        return "unsupported_by_design", "Executable/external relation requires a dedicated trusted domain"
    if identifier.casefold() in RUNTIME_RELATIONS:
        return "runtime_owned", "Runtime/back-reference relation is not persistent authoring state"
    if ptype in {"POINTER", "COLLECTION"}:
        return "relation", "Pointer/collection identity requires a typed domain relation"
    if bool(getattr(prop, "is_readonly", False)):
        return "read_only", "RNA marks this property read-only"
    if subtype in PATH_SUBTYPES:
        return "unsupported_by_design", "Filesystem paths require scoped asset/file semantics"
    if identifier.casefold() in EXECUTABLE_IDENTIFIERS:
        return "unsupported_by_design", "Executable or ambient text is not generic semantic data"
    if ptype == "STRING":
        folded = identifier.casefold()
        if identifier == "name":
            return "read_only", "Names require rename semantics so revision-bound refs can rotate safely"
        if any(fragment in folded for fragment in SENSITIVE_STRING_FRAGMENTS):
            return "unsupported_by_design", "String can carry filesystem, network or executable authority"
        return "managed", None
    if ptype in {"BOOLEAN", "INT", "FLOAT", "ENUM"}:
        length = int(getattr(prop, "array_length", 0) or 0)
        if length > MAX_ARRAY:
            return "unsupported_by_design", "RNA array exceeds the bounded generic value codec"
        return "managed", None
    return "unsupported_by_design", "RNA value kind is outside the bounded generic value codec"


def _enum_items(prop):
    if str(getattr(prop, "type", "")) != "ENUM":
        return []
    values = []
    try:
        for item in list(prop.enum_items)[:128]:
            values.append({"id": _text(item.identifier, 256), "name": _text(item.name, 512)})
    except Exception:
        pass
    return values


def _finite(value):
    return not isinstance(value, bool) and isinstance(value, (int, float)) and math.isfinite(float(value))


def _property_descriptor(prop, owner_identifier=None):
    status, reason = _property_status(prop)
    ptype = _text(getattr(prop, "type", ""), 64)
    identifier = _text(getattr(prop, "identifier", ""), 256)
    readonly = bool(getattr(prop, "is_readonly", False))
    domain_specific_pointer = (str(owner_identifier or ""), identifier) in DOMAIN_SPECIFIC_POINTERS
    out = {
        "id": identifier,
        "name": _text(getattr(prop, "name", ""), 512),
        "description": _text(getattr(prop, "description", "")),
        "type": ptype,
        "subtype": _text(getattr(prop, "subtype", ""), 64),
        "array_length": min(int(getattr(prop, "array_length", 0) or 0), 4096),
        "animatable": bool(getattr(prop, "is_animatable", False)),
        "status": status,
        "writable": status == "managed" or (status == "relation" and ptype == "POINTER" and not readonly and not domain_specific_pointer),
    }
    if status == "relation":
        out["relation_kind"] = ptype.casefold()
        out["relation_mutation"] = "pointer_set" if ptype == "POINTER" and not readonly and not domain_specific_pointer else "domain_specific"
    if reason:
        out["reason"] = reason
    for source, target in (("hard_min", "minimum"), ("hard_max", "maximum")):
        value = getattr(prop, source, None)
        if _finite(value):
            out[target] = value
    enum = _enum_items(prop)
    if enum:
        out["enum"] = enum
    return out


def _serialize(value, prop):
    ptype = str(getattr(prop, "type", ""))
    length = int(getattr(prop, "array_length", 0) or 0)
    if length:
        if length > MAX_ARRAY:
            raise ValueError("array too large")
        return [_serialize_scalar(item, ptype) for item in list(value)[:length]]
    return _serialize_scalar(value, ptype)


def _serialize_scalar(value, ptype):
    if ptype == "BOOLEAN":
        return bool(value)
    if ptype == "INT":
        return int(value)
    if ptype == "FLOAT":
        number = float(value)
        if not math.isfinite(number):
            raise ValueError("non-finite RNA value")
        return number
    if ptype in {"ENUM", "STRING"}:
        return _text(value, MAX_TEXT)
    raise ValueError("unsupported RNA value kind")


def _default_value(prop):
    length = int(getattr(prop, "array_length", 0) or 0)
    if length:
        raw = list(getattr(prop, "default_array", ()))
        if len(raw) != length:
            raise ValueError("RNA array default is unavailable")
        return _coerce(raw, prop)
    if not hasattr(prop, "default"):
        raise ValueError("RNA default is unavailable")
    return _coerce(getattr(prop, "default"), prop)


def _coerce(value, prop):
    status, _ = _property_status(prop)
    if status != "managed":
        raise ValueError("property is not generically mutable")
    ptype = str(getattr(prop, "type", ""))
    length = int(getattr(prop, "array_length", 0) or 0)
    if length:
        if not isinstance(value, list) or len(value) != length or length > MAX_ARRAY:
            raise ValueError("RNA array value has wrong length")
        return [_coerce_scalar(item, prop, ptype) for item in value]
    return _coerce_scalar(value, prop, ptype)


def _coerce_scalar(value, prop, ptype):
    if ptype == "BOOLEAN":
        if not isinstance(value, bool):
            raise ValueError("boolean required")
        return value
    if ptype == "INT":
        if isinstance(value, bool) or not isinstance(value, int):
            raise ValueError("integer required")
        lo, hi = getattr(prop, "hard_min", None), getattr(prop, "hard_max", None)
        if lo is not None and value < lo or hi is not None and value > hi:
            raise ValueError("integer outside RNA bounds")
        return value
    if ptype == "FLOAT":
        if not _finite(value):
            raise ValueError("finite number required")
        number = float(value)
        lo, hi = getattr(prop, "hard_min", None), getattr(prop, "hard_max", None)
        if lo is not None and number < lo or hi is not None and number > hi:
            raise ValueError("number outside RNA bounds")
        return number
    if ptype == "ENUM":
        if not isinstance(value, str) or len(value) > 256:
            raise ValueError("enum identifier required")
        allowed = {item["id"] for item in _enum_items(prop)}
        if value not in allowed:
            raise ValueError("enum value is not allowed by RNA")
        return value
    if ptype == "STRING":
        if not isinstance(value, str) or len(value) > MAX_TEXT or "\x00" in value:
            raise ValueError("bounded UTF-8 string required")
        return value
    raise ValueError("unsupported RNA value kind")


class SemanticError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


class SemanticStore:
    def __init__(self, bpy, workspace):
        self.bpy = bpy
        self.workspace = Path(workspace)
        if not self.workspace.is_absolute() or self.workspace == Path("/") or self.workspace.resolve(strict=True) != self.workspace:
            raise ValueError("Blender semantic workspace must be canonical and absolute")
        self.generation = 1

    def changed(self):
        self.generation += 1
        if self.generation > 2_147_483_647:
            self.generation = 1

    def _root(self, root):
        if root not in ROOTS:
            raise SemanticError("InvalidArgument", "Unknown semantic RNA root")
        value = getattr(self.bpy.data, root, None)
        if value is None:
            raise SemanticError("Unavailable", "RNA root is unavailable in this Blender build")
        return value

    def _ref(self, root, name, path=None):
        suffix = "" if not path else "/" + _path_token(path)
        return f"{REF_PREFIX}/{root}/{_token(name)}{suffix}@{self.generation}"

    def _parse_ref(self, reference):
        if not isinstance(reference, str) or len(reference) > 1536:
            raise SemanticError("InvalidArgument", "RNA reference is malformed")
        prefix = REF_PREFIX + "/"
        if not reference.startswith(prefix) or "@" not in reference:
            raise SemanticError("InvalidArgument", "RNA reference is malformed")
        body, raw_generation = reference[len(prefix):].rsplit("@", 1)
        parts = body.split("/")
        if len(parts) not in (2, 3):
            raise SemanticError("InvalidArgument", "RNA reference is malformed")
        root, token = parts[0], parts[1]
        try:
            generation = int(raw_generation)
            name = _untoken(token)
            path = _path_from_token(parts[2]) if len(parts) == 3 else []
        except Exception as error:
            raise SemanticError("InvalidArgument", "RNA reference is malformed") from error
        if generation != self.generation:
            raise SemanticError("StaleReference", "Blender RNA state changed; refresh semantic references")
        return root, name, path

    def _resolve(self, reference):
        root, name, path = self._parse_ref(reference)
        item = self._root(root).get(name)
        if item is None:
            raise SemanticError("StaleReference", "Referenced Blender datablock no longer exists")
        current = item
        for step in path:
            rna = getattr(current, "bl_rna", None)
            prop = rna.properties.get(step[1]) if rna is not None else None
            if prop is None or str(getattr(prop, "type", "")) not in {"POINTER", "COLLECTION"}:
                raise SemanticError("StaleReference", "RNA relation no longer resolves")
            try:
                relation = getattr(current, step[1])
                if step[0] == "p":
                    if str(getattr(prop, "type", "")) != "POINTER" or relation is None:
                        raise LookupError
                    current = relation
                else:
                    if str(getattr(prop, "type", "")) != "COLLECTION":
                        raise LookupError
                    current = relation[step[2]]
            except Exception as error:
                raise SemanticError("StaleReference", "RNA relation no longer resolves") from error
        return root, name, path, current

    def summary(self):
        return {
            "schema": "blender-rna-semantic/v1",
            "blender_version": list(self.bpy.app.version[:3]),
            "generation": self.generation,
            "roots": [
                {"root": root, "rna_type": rna}
                for root, rna in sorted(ROOTS.items())
                if getattr(self.bpy.data, root, None) is not None
            ],
            "generic_mutation": ["BOOLEAN", "INT", "FLOAT", "STRING", "ENUM", "bounded_arrays"],
            "arbitrary_python": False,
            "generic_operator_invoke": False,
        }

    def types(self, query="", limit=50):
        query = str(query or "").casefold()
        items = []
        for root, expected in sorted(ROOTS.items()):
            rna = getattr(self.bpy.types, expected, None)
            bl_rna = getattr(rna, "bl_rna", None)
            if bl_rna is None:
                continue
            haystack = f"{root} {expected} {getattr(bl_rna, 'name', '')}".casefold()
            if query and query not in haystack:
                continue
            items.append({
                "root": root,
                "identifier": expected,
                "name": _text(getattr(bl_rna, "name", expected), 512),
                "properties": min(len(list(bl_rna.properties)), 4096),
            })
        return {"items": items[:limit], "truncated": len(items) > limit}

    def type_describe(self, root):
        expected = ROOTS.get(root)
        if expected is None:
            raise SemanticError("InvalidArgument", "Unknown semantic RNA root")
        candidate = getattr(self.bpy.types, expected, None)
        rna = getattr(candidate, "bl_rna", None)
        if rna is None:
            raise SemanticError("Unavailable", "RNA type is unavailable")
        properties = []
        owner_identifier = _text(getattr(rna, "identifier", expected), 256)
        for prop in list(rna.properties)[:1024]:
            properties.append(_property_descriptor(prop, owner_identifier))
        return {
            "root": root,
            "identifier": expected,
            "name": _text(getattr(rna, "name", expected), 512),
            "description": _text(getattr(rna, "description", "")),
            "properties": properties,
            "truncated": len(list(rna.properties)) > 1024,
        }

    def objects(self, root, query="", limit=50, offset=0):
        query = str(query or "").casefold()
        source = self._root(root)
        matched = []
        for item in source:
            name = _text(getattr(item, "name", ""), MAX_NAME)
            if query and query not in name.casefold():
                continue
            matched.append({
                "ref": self._ref(root, name),
                "name": name,
                "rna_type": _text(getattr(getattr(item, "bl_rna", None), "identifier", ""), 256),
            })
        page = matched[offset:offset + limit]
        return {"items": page, "truncated": len(matched) > offset + len(page), "generation": self.generation, "offset": offset}

    def object_describe(self, reference):
        root, name, path, item = self._resolve(reference)
        rna = getattr(item, "bl_rna", None)
        properties = []
        if rna is not None:
            owner_identifier = _text(getattr(rna, "identifier", ""), 256)
            for prop in list(rna.properties)[:1024]:
                properties.append(_property_descriptor(prop, owner_identifier))
        return {
            "ref": self._ref(root, name, path),
            "root": root,
            "name": name,
            "rna_type": _text(getattr(rna, "identifier", ""), 256),
            "properties": properties,
            "truncated": bool(rna is not None and len(list(rna.properties)) > 1024),
        }

    def relations(self, reference, property_id, limit=50, offset=0):
        root, name, path, item = self._resolve(reference)
        rna = getattr(item, "bl_rna", None)
        prop = rna.properties.get(property_id) if rna is not None else None
        if prop is None or property_id == "rna_type":
            raise SemanticError("NotFound", "RNA relation property does not exist")
        ptype = str(getattr(prop, "type", ""))
        status, reason = _property_status(prop)
        if status != "relation" or ptype not in {"POINTER", "COLLECTION"}:
            if status == "unsupported_by_design":
                raise SemanticError("Unsupported", reason or "RNA relation is excluded")
            raise SemanticError("InvalidArgument", "RNA property is not a traversable relation")
        try:
            relation = getattr(item, property_id)
        except Exception as error:
            raise SemanticError("Unsupported", "RNA relation cannot be inspected safely") from error
        items = []
        if ptype == "POINTER":
            if relation is not None and offset == 0:
                child_path = path + [["p", property_id]]
                child_rna = getattr(relation, "bl_rna", None)
                items.append({
                    "ref": self._ref(root, name, child_path),
                    "name": _text(getattr(relation, "name", property_id), MAX_NAME),
                    "rna_type": _text(getattr(child_rna, "identifier", ""), 256),
                    "index": 0,
                })
            total = 1 if relation is not None else 0
        else:
            try:
                total = len(relation)
            except Exception as error:
                raise SemanticError("Unsupported", "RNA collection length is unavailable") from error
            end = min(total, offset + limit)
            for index in range(offset, end):
                try:
                    child = relation[index]
                except Exception as error:
                    raise SemanticError("StaleReference", "RNA collection changed during traversal") from error
                child_path = path + [["c", property_id, index]]
                child_rna = getattr(child, "bl_rna", None)
                items.append({
                    "ref": self._ref(root, name, child_path),
                    "name": _text(getattr(child, "name", f"#{index}"), MAX_NAME),
                    "rna_type": _text(getattr(child_rna, "identifier", ""), 256),
                    "index": index,
                })
        linker = getattr(relation, "link", None) if ptype == "COLLECTION" else None
        unlinker = getattr(relation, "unlink", None) if ptype == "COLLECTION" else None
        owner_identifier = _text(getattr(rna, "identifier", ""), 256) if rna is not None else ""
        domain_specific_pointer = (owner_identifier, property_id) in DOMAIN_SPECIFIC_POINTERS
        mutation = (
            "pointer_set"
            if ptype == "POINTER" and not bool(getattr(prop, "is_readonly", False)) and not domain_specific_pointer
            else "link_unlink"
            if ptype == "COLLECTION" and callable(linker) and callable(unlinker)
            else "domain_specific"
        )
        return {
            "items": items,
            "truncated": total > offset + len(items),
            "generation": self.generation,
            "offset": offset,
            "relation": property_id,
            "mutation": mutation,
        }

    def query(self, root, property_id, operator, expected, limit=50, offset=0):
        source = self._root(root)
        if operator not in {"eq", "ne", "lt", "lte", "gt", "gte", "contains"}:
            raise SemanticError("InvalidArgument", "Semantic query operator is invalid")
        matches = []
        required = offset + limit + 1
        for item in source:
            rna = getattr(item, "bl_rna", None)
            prop = rna.properties.get(property_id) if rna is not None else None
            if prop is None:
                continue
            status, _reason = _property_status(prop)
            if status in {"relation", "runtime_owned", "unsupported_by_design"}:
                continue
            try:
                current = _serialize(getattr(item, property_id), prop)
            except Exception:
                continue
            if isinstance(current, list):
                continue
            ok = False
            if operator == "eq":
                ok = current == expected
            elif operator == "ne":
                ok = current != expected
            elif operator in {"lt", "lte", "gt", "gte"}:
                if _finite(current) and _finite(expected):
                    left, right = float(current), float(expected)
                    ok = {"lt": left < right, "lte": left <= right, "gt": left > right, "gte": left >= right}[operator]
            elif operator == "contains":
                ok = isinstance(current, str) and isinstance(expected, str) and expected.casefold() in current.casefold()
            if not ok:
                continue
            name = _text(getattr(item, "name", ""), MAX_NAME)
            matches.append({"ref": self._ref(root, name), "name": name, "value": current})
            if len(matches) >= required:
                break
        page = matches[offset:offset + limit]
        return {
            "items": page,
            "offset": offset,
            "truncated": len(matches) > offset + len(page),
            "generation": self.generation,
            "property": property_id,
            "operator": operator,
        }

    def rna_describe(self, identifier):
        if not isinstance(identifier, str) or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]{0,255}", identifier):
            raise SemanticError("InvalidArgument", "RNA type identifier is invalid")
        candidate = getattr(self.bpy.types, identifier, None)
        if candidate is None:
            root_type = getattr(self.bpy.types, "bpy_struct", None)
            resolver = getattr(root_type, "bl_rna_get_subclass_py", None)
            if callable(resolver):
                try:
                    candidate = resolver(identifier, None)
                except Exception:
                    candidate = None
        rna = getattr(candidate, "bl_rna", None)
        if rna is None:
            raise SemanticError("NotFound", "Blender RNA type does not exist")
        base = getattr(rna, "base", None)
        owner_identifier = _text(getattr(rna, "identifier", identifier), 256)
        properties = [_property_descriptor(prop, owner_identifier) for prop in list(rna.properties)[:1024]]
        return {
            "identifier": _text(getattr(rna, "identifier", identifier), 256),
            "name": _text(getattr(rna, "name", identifier), 512),
            "description": _text(getattr(rna, "description", "")),
            "base": _text(getattr(base, "identifier", ""), 256) if base else None,
            "properties": properties,
            "truncated": len(list(rna.properties)) > 1024,
        }

    def rename(self, reference, new_name):
        root, anchor_name, path, item = self._resolve(reference)
        if not isinstance(new_name, str) or not new_name.strip() or len(new_name) > 128 or "\x00" in new_name:
            raise SemanticError("InvalidArgument", "RNA name is invalid")
        if not hasattr(item, "name"):
            raise SemanticError("Unsupported", "RNA object does not expose stable naming")
        if not path:
            collection = self._root(root)
            existing = collection.get(new_name)
            if existing is not None and existing is not item:
                raise SemanticError("Conflict", "Datablock name already exists; implicit suffixing is forbidden")
        old_name = str(getattr(item, "name", ""))
        try:
            item.name = new_name
        except Exception as error:
            raise SemanticError("Unsupported", "Blender rejected semantic rename") from error
        actual = str(getattr(item, "name", ""))
        if actual != new_name:
            try:
                item.name = old_name
            except Exception:
                pass
            raise SemanticError("Conflict", "Blender would suffix or rewrite the requested name")
        self.changed()
        ref_name = new_name if not path else anchor_name
        return {"ref": self._ref(root, ref_name, path), "name": new_name, "changed": old_name != new_name, "generation": self.generation}

    def _custom_key(self, key):
        if not isinstance(key, str) or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_.:-]{0,127}", key) or key == "_RNA_UI":
            raise SemanticError("InvalidArgument", "Custom property key is invalid")
        return key

    def _custom_scalar(self, value):
        if value is None:
            raise SemanticError("InvalidArgument", "Custom property null is not supported; remove the key explicitly")
        if isinstance(value, bool):
            return value
        if isinstance(value, int) and not isinstance(value, bool):
            if not -(2**53) <= value <= 2**53:
                raise SemanticError("InvalidArgument", "Custom integer exceeds exact JSON bounds")
            return value
        if isinstance(value, float):
            if not math.isfinite(value):
                raise SemanticError("InvalidArgument", "Custom number must be finite")
            return value
        if isinstance(value, str):
            if len(value) > MAX_TEXT or "\x00" in value:
                raise SemanticError("InvalidArgument", "Custom string exceeds bounded UTF-8 limits")
            return value
        raise SemanticError("Unsupported", "Custom property scalar is not representable as bounded JSON")

    def _custom_value(self, value):
        if hasattr(value, "to_list") and callable(value.to_list):
            value = value.to_list()
        if isinstance(value, (list, tuple)):
            if len(value) > MAX_ARRAY:
                raise SemanticError("InvalidArgument", "Custom array exceeds bounded length")
            return [self._custom_scalar(item) for item in value]
        return self._custom_scalar(value)

    def custom_list(self, reference, limit=50, offset=0):
        root, name, path, item = self._resolve(reference)
        try:
            keys = sorted(str(key) for key in item.keys() if str(key) != "_RNA_UI")
        except Exception as error:
            raise SemanticError("Unsupported", "RNA object does not expose custom properties") from error
        page = keys[offset:offset + limit]
        values = []
        for key in page:
            try:
                value = self._custom_value(item[key])
                values.append({"key": key, "status": "managed", "value": value})
            except SemanticError:
                values.append({"key": key, "status": "unsupported_by_design", "value": None})
        return {"ref": self._ref(root, name, path), "items": values, "offset": offset, "truncated": len(keys) > offset + len(page), "generation": self.generation}

    def custom_get(self, reference, key):
        key = self._custom_key(key)
        root, name, path, item = self._resolve(reference)
        try:
            if key not in item:
                raise SemanticError("NotFound", "Custom property does not exist")
            value = self._custom_value(item[key])
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("Unsupported", "Custom property cannot be represented safely") from error
        return {"ref": self._ref(root, name, path), "key": key, "value": value}

    def custom_set(self, reference, key, value):
        key = self._custom_key(key)
        root, name, path, item = self._resolve(reference)
        value = self._custom_value(value)
        try:
            item[key] = value
        except Exception as error:
            raise SemanticError("Unsupported", "Blender rejected bounded custom property mutation") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "key": key, "value": self._custom_value(item[key]), "changed": True, "generation": self.generation}

    def custom_remove(self, reference, key):
        key = self._custom_key(key)
        root, name, path, item = self._resolve(reference)
        try:
            if key not in item:
                raise SemanticError("NotFound", "Custom property does not exist")
            del item[key]
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected custom property removal") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "key": key, "changed": True, "generation": self.generation}

    def _asset_path(self, relative, extensions):
        if not isinstance(relative, str) or not relative or "\x00" in relative or relative.startswith("/"):
            raise SemanticError("PolicyDenied", "Asset path must be a clean relative workspace path")
        parts = Path(relative).parts
        if any(part in {"", ".", ".."} for part in parts):
            raise SemanticError("PolicyDenied", "Asset path traversal is denied")
        path = self.workspace.joinpath(*parts)
        if path.suffix.casefold() not in extensions:
            raise SemanticError("Unsupported", "Asset extension is outside the allowlist for this Blender datablock")
        try:
            metadata = path.lstat()
        except FileNotFoundError as error:
            raise SemanticError("NotFound", "Asset file does not exist") from error
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1 or stat.S_ISLNK(metadata.st_mode):
            raise SemanticError("PolicyDenied", "Asset must be a single-link regular file")
        if metadata.st_size > 512 * 1024 * 1024:
            raise SemanticError("PolicyDenied", "Asset exceeds the 512 MiB semantic import ceiling")
        try:
            resolved = path.resolve(strict=True)
            resolved.relative_to(self.workspace)
        except Exception as error:
            raise SemanticError("PolicyDenied", "Asset escaped the workspace root") from error
        return resolved, metadata.st_size

    def asset_load(self, root, relative, name):
        loaders = {
            "images": {".png", ".jpg", ".jpeg", ".webp", ".tif", ".tiff", ".bmp", ".exr", ".hdr"},
            "sounds": {".wav", ".flac", ".ogg", ".mp3"},
            "fonts": {".ttf", ".otf"},
            "movieclips": {".mp4", ".mov", ".avi", ".mkv", ".webm", ".png", ".jpg", ".jpeg", ".tif", ".tiff", ".exr"},
            "volumes": {".vdb"},
            "cache_files": {".abc"},
        }
        extensions = loaders.get(root)
        if extensions is None:
            raise SemanticError("Unsupported", "This semantic root is not a scoped file-backed asset")
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Asset datablock name is invalid")
        collection = self._root(root)
        if collection.get(name) is not None:
            raise SemanticError("Conflict", "Asset datablock name already exists")
        path, size = self._asset_path(relative, extensions)
        loader = getattr(collection, "load", None)
        if not callable(loader):
            raise SemanticError("Unsupported", "Blender does not expose a loader for this asset root")
        loaded = None
        try:
            try:
                loaded = loader(str(path), check_existing=False)
            except TypeError:
                loaded = loader(str(path))
            loaded.name = name
            if loaded.name != name:
                raise SemanticError("Conflict", "Blender rewrote the requested asset datablock name")
        except SemanticError:
            if loaded is not None:
                try:
                    collection.remove(loaded)
                except Exception:
                    pass
            raise
        except Exception as error:
            if loaded is not None:
                try:
                    collection.remove(loaded)
                except Exception:
                    pass
            raise SemanticError("Unsupported", "Blender rejected the scoped asset file") from error
        self.changed()
        return {
            "ref": self._ref(root, loaded.name),
            "root": root,
            "name": loaded.name,
            "bytes": size,
            "path": relative,
            "changed": True,
            "generation": self.generation,
        }

    def object_create(self, name, data_reference=None, collection_reference=None):
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Object name is invalid")
        if self.bpy.data.objects.get(name) is not None:
            raise SemanticError("Conflict", "Object name already exists; implicit suffixing is forbidden")

        object_data_roots = {
            "armatures",
            "cameras",
            "curves",
            "grease_pencils",
            "grease_pencils_v3",
            "hair_curves",
            "lattices",
            "lights",
            "lightprobes",
            "meshes",
            "metaballs",
            "pointclouds",
            "speakers",
            "volumes",
        }
        data = None
        if data_reference is not None:
            data_parts = self._resolve(data_reference)
            if data_parts[2] or data_parts[0] not in object_data_roots:
                raise SemanticError("InvalidArgument", "Object data_ref must reference a root-level compatible geometry/data datablock")
            data = data_parts[3]

        if collection_reference is None:
            collection = self.bpy.context.scene.collection
        else:
            collection_parts = self._resolve(collection_reference)
            if collection_parts[2] or collection_parts[0] != "collections":
                raise SemanticError("InvalidArgument", "collection_ref must reference a root Collection")
            collection = collection_parts[3]

        try:
            obj = self.bpy.data.objects.new(name, data)
            collection.objects.link(obj)
        except Exception as error:
            existing = self.bpy.data.objects.get(name)
            if existing is not None:
                try:
                    self.bpy.data.objects.remove(existing, do_unlink=True)
                except Exception:
                    pass
            raise SemanticError("InvalidArgument", "Blender rejected typed Object creation") from error

        self.changed()
        return {
            "ref": self._ref("objects", obj.name),
            "name": obj.name,
            "object_type": _text(getattr(obj, "type", ""), 64),
            "data_ref": None
            if data is None
            else self._ref(
                data_parts[0],
                data_parts[1],
                data_parts[2],
            ),
            "changed": True,
            "generation": self.generation,
        }

    def datablock_create(self, root, name, kind=None):
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Datablock name is invalid")
        collection = self._root(root)
        if collection.get(name) is not None:
            raise SemanticError("Conflict", "Datablock name already exists; implicit suffixing is forbidden")
        simple = {
            "actions", "armatures", "brushes", "cameras", "collections", "grease_pencils",
            "grease_pencils_v3", "hair_curves", "lattices", "linestyles", "masks", "materials", "meshes",
            "metaballs", "palettes", "particles", "pointclouds", "scenes", "speakers",
            "volumes", "worlds",
        }
        typed = {
            "curves": {"CURVE", "SURFACE", "FONT"},
            "lights": {"POINT", "SUN", "SPOT", "AREA"},
            "lightprobes": {"SPHERE", "PLANE", "VOLUME"},
            "node_groups": {"ShaderNodeTree", "GeometryNodeTree", "CompositorNodeTree", "TextureNodeTree"},
            "textures": {"NONE", "BLEND", "CLOUDS", "DISTORTED_NOISE", "IMAGE", "MAGIC", "MARBLE", "MUSGRAVE", "NOISE", "STUCCI", "VORONOI", "WOOD"},
        }
        try:
            if root in simple:
                if kind is not None:
                    raise SemanticError("InvalidArgument", "This datablock root does not accept kind")
                item = collection.new(name)
            elif root in typed:
                if kind not in typed[root]:
                    raise SemanticError("InvalidArgument", "Datablock kind is not allowed for this root")
                item = collection.new(name, kind)
            else:
                raise SemanticError("Unsupported", "Datablock creation requires a dedicated asset/domain capability")
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("Unsupported", "Blender rejected datablock creation for this root") from error
        self.changed()
        return {"ref": self._ref(root, item.name), "root": root, "name": item.name, "rna_type": _text(getattr(getattr(item, "bl_rna", None), "identifier", ""), 256), "changed": True, "generation": self.generation}

    def datablock_remove(self, reference):
        root, name, path, item = self._resolve(reference)
        if path:
            raise SemanticError("InvalidArgument", "Datablock removal requires a root datablock ref")
        if root == "scenes":
            if len(self.bpy.data.scenes) <= 1:
                raise SemanticError("Conflict", "Blender must retain at least one Scene")
            if item is self.bpy.context.scene:
                raise SemanticError("Conflict", "Active Scene cannot be removed through generic datablock semantics")
        collection = self._root(root)
        remover = getattr(collection, "remove", None)
        if not callable(remover):
            raise SemanticError("Unsupported", "Datablock root does not expose bounded removal")
        try:
            try:
                remover(item, do_unlink=True)
            except TypeError:
                remover(item)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected datablock removal") from error
        self.changed()
        return {"root": root, "name": name, "changed": True, "generation": self.generation}

    def _relation_collection(self, reference, property_id):
        root, name, path, item = self._resolve(reference)
        rna = getattr(item, "bl_rna", None)
        prop = rna.properties.get(property_id) if rna is not None else None
        if prop is None:
            raise SemanticError("NotFound", "RNA collection relation does not exist")
        status, reason = _property_status(prop)
        if status != "relation" or str(getattr(prop, "type", "")) != "COLLECTION":
            raise SemanticError("Unsupported", reason or "RNA property is not a mutable collection relation")
        try:
            relation = getattr(item, property_id)
        except Exception as error:
            raise SemanticError("Unsupported", "RNA collection relation cannot be resolved") from error
        return root, name, path, item, relation

    def relation_link(self, reference, property_id, target_reference):
        root, name, path, _item, relation = self._relation_collection(reference, property_id)
        target_parts = self._resolve(target_reference)
        linker = getattr(relation, "link", None)
        if not callable(linker):
            raise SemanticError("Unsupported", "RNA collection does not expose link semantics")
        try:
            linker(target_parts[3])
        except Exception as error:
            raise SemanticError("InvalidArgument", "Blender rejected the relation link") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "property": property_id, "target_ref": self._ref(target_parts[0], target_parts[1], target_parts[2]), "changed": True, "generation": self.generation}

    def relation_unlink(self, reference, property_id, target_reference):
        root, name, path, _item, relation = self._relation_collection(reference, property_id)
        target_parts = self._resolve(target_reference)
        unlinker = getattr(relation, "unlink", None)
        if not callable(unlinker):
            raise SemanticError("Unsupported", "RNA collection does not expose unlink semantics")
        try:
            unlinker(target_parts[3])
        except Exception as error:
            raise SemanticError("InvalidArgument", "Blender rejected the relation unlink") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "property": property_id, "target_ref": self._ref(target_parts[0], target_parts[1], target_parts[2]), "changed": True, "generation": self.generation}

    def relation_set(self, reference, property_id, target_reference):
        root, name, path, item = self._resolve(reference)
        rna = getattr(item, "bl_rna", None)
        prop = rna.properties.get(property_id) if rna is not None else None
        if prop is None:
            raise SemanticError("NotFound", "RNA pointer property does not exist")
        status, reason = _property_status(prop)
        owner_identifier = _text(getattr(rna, "identifier", ""), 256) if rna is not None else ""
        if (owner_identifier, property_id) in DOMAIN_SPECIFIC_POINTERS:
            raise SemanticError("Unsupported", "RNA pointer requires dedicated lifecycle semantics")
        if status != "relation" or str(getattr(prop, "type", "")) != "POINTER" or bool(getattr(prop, "is_readonly", False)):
            raise SemanticError("Unsupported", reason or "RNA pointer is not generically mutable")
        target = None
        target_parts = None
        if target_reference is not None:
            target_parts = self._resolve(target_reference)
            target = target_parts[3]
        try:
            setattr(item, property_id, target)
        except Exception as error:
            raise SemanticError("InvalidArgument", "RNA rejected the typed relation target") from error
        self.changed()
        return {
            "ref": self._ref(root, name, path),
            "property": property_id,
            "target_ref": None if target_parts is None else self._ref(target_parts[0], target_parts[1], target_parts[2]),
            "changed": True,
            "generation": self.generation,
        }

    def _require_object(self, reference):
        root, name, path, item = self._resolve(reference)
        if _text(getattr(getattr(item, "bl_rna", None), "identifier", ""), 256) != "Object":
            raise SemanticError("InvalidArgument", "Operation requires an Object RNA reference")
        return root, name, path, item

    def _collection_parent(self, reference, expected):
        root, name, path = self._parse_ref(reference)
        if not path or path[-1][0] != "c" or path[-1][1] != expected:
            raise SemanticError("InvalidArgument", f"Reference is not a {expected} collection item")
        parent_path = path[:-1]
        parent_ref = self._ref(root, name, parent_path)
        parent = self._resolve(parent_ref)[3]
        item = self._resolve(reference)[3]
        return root, name, parent_path, parent, item

    def _require_armature_object(self, reference):
        root, name, path, obj = self._require_object(reference)
        if getattr(obj, "type", None) != "ARMATURE" or getattr(obj, "data", None) is None:
            raise SemanticError("InvalidArgument", "Operation requires an Object with Armature data")
        return root, name, path, obj

    def _armature_edit(self, obj, action):
        context = self.bpy.context
        active = context.view_layer.objects.active
        selected = list(context.selected_objects)
        if active is not None and getattr(active, "mode", "OBJECT") != "OBJECT":
            raise SemanticError("Conflict", "Armature authoring requires the private Blender runtime to be in Object mode")
        try:
            for item in selected:
                item.select_set(False)
            obj.select_set(True)
            context.view_layer.objects.active = obj
            result = self.bpy.ops.object.mode_set(mode="EDIT")
            if "FINISHED" not in result:
                raise SemanticError("Conflict", "Blender could not enter Armature Edit Mode")
            return action(obj.data.edit_bones)
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected bounded armature edit") from error
        finally:
            try:
                if getattr(obj, "mode", "OBJECT") != "OBJECT":
                    self.bpy.ops.object.mode_set(mode="OBJECT")
            except Exception:
                pass
            try:
                for item in list(context.selected_objects):
                    item.select_set(False)
                for item in selected:
                    if self.bpy.data.objects.get(item.name) is item:
                        item.select_set(True)
                if active is not None and self.bpy.data.objects.get(active.name) is active:
                    context.view_layer.objects.active = active
            except Exception:
                pass

    def armature_bone_add(self, object_ref, name, head, tail, parent_name=None, connected=False):
        root, object_name, path, obj = self._require_armature_object(object_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Bone name is invalid")
        if obj.data.bones.get(name) is not None:
            raise SemanticError("Conflict", "Bone name already exists")
        for value, label in ((head, "head"), (tail, "tail")):
            if not isinstance(value, list) or len(value) != 3 or any(not _finite(component) or abs(float(component)) > 1_000_000 for component in value):
                raise SemanticError("InvalidArgument", f"Bone {label} must contain three bounded finite coordinates")
        if [float(v) for v in head] == [float(v) for v in tail]:
            raise SemanticError("InvalidArgument", "Bone head and tail must differ")
        if parent_name is not None and (not isinstance(parent_name, str) or len(parent_name) > 128 or obj.data.bones.get(parent_name) is None):
            raise SemanticError("NotFound", "Parent bone does not exist")
        if not isinstance(connected, bool):
            raise SemanticError("InvalidArgument", "connected must be boolean")

        def create(edit_bones):
            bone = edit_bones.new(name)
            bone.head = [float(v) for v in head]
            bone.tail = [float(v) for v in tail]
            if parent_name is not None:
                parent = edit_bones.get(parent_name)
                if parent is None:
                    raise SemanticError("StaleReference", "Parent bone changed before edit")
                bone.parent = parent
                bone.use_connect = connected
            return bone.name

        actual_name = self._armature_edit(obj, create)
        if actual_name != name or obj.data.bones.get(name) is None:
            raise SemanticError("Conflict", "Blender rewrote the requested bone name")
        index = list(obj.data.bones).index(obj.data.bones[name])
        self.changed()
        bone_path = path + [["p", "data"], ["c", "bones", index]]
        return {"ref": self._ref(root, object_name, bone_path), "name": name, "changed": True, "generation": self.generation}

    def armature_bone_remove(self, bone_ref):
        root, object_name, path = self._parse_ref(bone_ref)
        if len(path) < 2 or path[-1][0:2] != ["c", "bones"] or path[-2] != ["p", "data"]:
            raise SemanticError("InvalidArgument", "Reference is not a persistent Armature Bone")
        obj = self._resolve(self._ref(root, object_name, path[:-2]))[3]
        if getattr(obj, "type", None) != "ARMATURE":
            raise SemanticError("InvalidArgument", "Bone owner is not an Armature Object")
        bone = self._resolve(bone_ref)[3]
        bone_name = str(bone.name)

        def remove(edit_bones):
            edit = edit_bones.get(bone_name)
            if edit is None:
                raise SemanticError("StaleReference", "Bone changed before edit")
            edit_bones.remove(edit)

        self._armature_edit(obj, remove)
        self.changed()
        return {"object_ref": self._ref(root, object_name, path[:-2]), "changed": True, "generation": self.generation}

    def armature_bone_parent_set(self, bone_ref, parent_ref, connected=False):
        root, object_name, path = self._parse_ref(bone_ref)
        if len(path) < 2 or path[-1][0:2] != ["c", "bones"] or path[-2] != ["p", "data"]:
            raise SemanticError("InvalidArgument", "Reference is not a persistent Armature Bone")
        obj = self._resolve(self._ref(root, object_name, path[:-2]))[3]
        child = self._resolve(bone_ref)[3]
        child_name = str(child.name)
        parent_name = None
        if parent_ref is not None:
            parent_root, parent_object, parent_path, parent = self._resolve(parent_ref)
            if parent_root != root or parent_object != object_name or len(parent_path) < 2 or parent_path[-1][0:2] != ["c", "bones"]:
                raise SemanticError("InvalidArgument", "Parent bone must belong to the same Armature Object")
            parent_name = str(parent.name)
            if parent_name == child_name:
                raise SemanticError("InvalidArgument", "Bone cannot parent itself")
        if not isinstance(connected, bool):
            raise SemanticError("InvalidArgument", "connected must be boolean")

        def reparent(edit_bones):
            edit_child = edit_bones.get(child_name)
            edit_parent = edit_bones.get(parent_name) if parent_name is not None else None
            if edit_child is None or (parent_name is not None and edit_parent is None):
                raise SemanticError("StaleReference", "Bone hierarchy changed before edit")
            edit_child.use_connect = False
            edit_child.parent = edit_parent
            if edit_parent is not None:
                edit_child.use_connect = connected

        self._armature_edit(obj, reparent)
        index = list(obj.data.bones).index(obj.data.bones[child_name])
        self.changed()
        child_path = path[:-1] + [["c", "bones", index]]
        return {
            "ref": self._ref(root, object_name, child_path),
            "parent_name": parent_name,
            "connected": connected if parent_name is not None else False,
            "changed": True,
            "generation": self.generation,
        }

    def _require_pose_bone(self, reference):
        root, name, path, bone = self._resolve(reference)
        if _text(getattr(getattr(bone, "bl_rna", None), "identifier", ""), 256) != "PoseBone":
            raise SemanticError("InvalidArgument", "Operation requires a PoseBone RNA reference")
        return root, name, path, bone

    def pose_constraint_add(self, pose_bone_ref, name, constraint_type):
        root, anchor_name, path, bone = self._require_pose_bone(pose_bone_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Pose constraint name is invalid")
        if bone.constraints.get(name) is not None:
            raise SemanticError("Conflict", "Pose constraint name already exists")
        if not isinstance(constraint_type, str) or not re.fullmatch(r"[A-Z][A-Z0-9_]{0,63}", constraint_type):
            raise SemanticError("InvalidArgument", "Pose constraint type is invalid")
        try:
            constraint = bone.constraints.new(type=constraint_type)
            constraint.name = name
            if constraint.name != name:
                bone.constraints.remove(constraint)
                raise SemanticError("Conflict", "Blender rewrote the requested pose constraint name")
            index = list(bone.constraints).index(constraint)
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("Unsupported", "Pose constraint type is unavailable in this Blender build") from error
        self.changed()
        return {
            "ref": self._ref(root, anchor_name, path + [["c", "constraints", index]]),
            "name": constraint.name,
            "type": constraint.type,
            "changed": True,
            "generation": self.generation,
        }

    def pose_constraint_remove(self, constraint_ref):
        root, anchor_name, parent_path, bone, constraint = self._collection_parent(constraint_ref, "constraints")
        if _text(getattr(getattr(bone, "bl_rna", None), "identifier", ""), 256) != "PoseBone":
            raise SemanticError("InvalidArgument", "Pose constraint parent is not a PoseBone")
        try:
            bone.constraints.remove(constraint)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected pose constraint removal") from error
        self.changed()
        return {"pose_bone_ref": self._ref(root, anchor_name, parent_path), "changed": True, "generation": self.generation}

    def _require_armature_data(self, reference):
        root, name, path, armature = self._resolve(reference)
        if _text(getattr(getattr(armature, "bl_rna", None), "identifier", ""), 256) != "Armature":
            raise SemanticError("InvalidArgument", "Operation requires an Armature RNA reference")
        return root, name, path, armature

    def bone_collection_add(self, armature_ref, name, parent_ref=None):
        root, anchor_name, path, armature = self._require_armature_data(armature_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Bone collection name is invalid")
        if any(collection.name == name for collection in armature.collections_all):
            raise SemanticError("Conflict", "Bone collection name already exists")
        parent = None
        if parent_ref is not None:
            parent_parts = self._resolve(parent_ref)
            parent = parent_parts[3]
            if _text(getattr(getattr(parent, "bl_rna", None), "identifier", ""), 256) != "BoneCollection" or getattr(parent, "id_data", None) is not armature:
                raise SemanticError("InvalidArgument", "Parent bone collection must belong to the same Armature")
        try:
            collection = armature.collections.new(name, parent=parent)
            if collection.name != name:
                armature.collections.remove(collection)
                raise SemanticError("Conflict", "Blender rewrote the requested bone collection name")
            index = list(armature.collections_all).index(collection)
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected bone collection creation") from error
        self.changed()
        return {"ref": self._ref(root, anchor_name, path + [["c", "collections_all", index]]), "name": name, "changed": True, "generation": self.generation}

    def bone_collection_remove(self, collection_ref):
        root, anchor_name, parent_path, armature, collection = self._collection_parent(collection_ref, "collections_all")
        if _text(getattr(getattr(armature, "bl_rna", None), "identifier", ""), 256) != "Armature":
            raise SemanticError("InvalidArgument", "Bone collection parent is not an Armature")
        try:
            armature.collections.remove(collection)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected bone collection removal") from error
        self.changed()
        return {"armature_ref": self._ref(root, anchor_name, parent_path), "changed": True, "generation": self.generation}

    def bone_collection_assign(self, collection_ref, bone_ref, assign=True):
        c_root, c_name, c_path, collection = self._resolve(collection_ref)
        b_root, b_name, b_path, bone = self._resolve(bone_ref)
        if _text(getattr(getattr(collection, "bl_rna", None), "identifier", ""), 256) != "BoneCollection":
            raise SemanticError("InvalidArgument", "collection_ref is not a BoneCollection")
        if _text(getattr(getattr(bone, "bl_rna", None), "identifier", ""), 256) not in {"Bone", "EditBone", "PoseBone"}:
            raise SemanticError("InvalidArgument", "bone_ref is not a Blender bone")
        armature = getattr(collection, "id_data", None)
        bone_owner = getattr(bone, "id_data", None)
        if _text(getattr(getattr(bone, "bl_rna", None), "identifier", ""), 256) == "PoseBone":
            bone_owner = getattr(bone_owner, "data", None)
        if armature is None or bone_owner is not armature:
            raise SemanticError("InvalidArgument", "Bone and BoneCollection must belong to the same Armature")
        try:
            changed = bool(collection.assign(bone) if assign else collection.unassign(bone))
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected bone collection membership mutation") from error
        if changed:
            self.changed()
        return {
            "collection_ref": self._ref(c_root, c_name, c_path),
            "bone_ref": self._ref(b_root, b_name, b_path),
            "assigned": assign,
            "changed": changed,
            "generation": self.generation,
        }

    def _require_node_interface(self, interface_ref):
        root, name, path, interface = self._resolve(interface_ref)
        if _text(getattr(getattr(interface, "bl_rna", None), "identifier", ""), 256) != "NodeTreeInterface":
            raise SemanticError("InvalidArgument", "Operation requires a NodeTreeInterface RNA reference")
        return root, name, path, interface

    def _interface_parent(self, root, name, interface, parent_ref):
        if parent_ref is None:
            return None
        p_root, p_name, _p_path, parent = self._resolve(parent_ref)
        if (p_root, p_name) != (root, name) or _text(getattr(getattr(parent, "bl_rna", None), "identifier", ""), 256) != "NodeTreeInterfacePanel":
            raise SemanticError("InvalidArgument", "Interface parent must be a panel in the same NodeTree")
        if parent not in list(interface.items_tree):
            raise SemanticError("StaleReference", "Interface parent no longer belongs to this NodeTree")
        return parent

    def node_interface_socket_add(self, interface_ref, name, in_out="INPUT", socket_type="DEFAULT", description="", parent_ref=None):
        root, anchor_name, path, interface = self._require_node_interface(interface_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Interface socket name is invalid")
        if in_out not in {"INPUT", "OUTPUT"}:
            raise SemanticError("InvalidArgument", "Interface socket direction is invalid")
        if not isinstance(socket_type, str) or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]{0,127}", socket_type):
            raise SemanticError("InvalidArgument", "Interface socket type is invalid")
        if not isinstance(description, str) or len(description) > MAX_TEXT or "\x00" in description:
            raise SemanticError("InvalidArgument", "Interface socket description is invalid")
        parent = self._interface_parent(root, anchor_name, interface, parent_ref)
        try:
            socket = interface.new_socket(name=name, description=description, in_out=in_out, socket_type=socket_type, parent=parent)
            index = list(interface.items_tree).index(socket)
        except Exception as error:
            raise SemanticError("Unsupported", "Blender rejected NodeTree interface socket creation") from error
        self.changed()
        return {"ref": self._ref(root, anchor_name, path + [["c", "items_tree", index]]), "name": socket.name, "item_type": "SOCKET", "changed": True, "generation": self.generation}

    def node_interface_panel_add(self, interface_ref, name, description="", default_closed=False):
        root, anchor_name, path, interface = self._require_node_interface(interface_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Interface panel name is invalid")
        if not isinstance(description, str) or len(description) > MAX_TEXT or "\x00" in description or not isinstance(default_closed, bool):
            raise SemanticError("InvalidArgument", "Interface panel arguments are invalid")
        try:
            panel = interface.new_panel(name=name, description=description, default_closed=default_closed)
            index = list(interface.items_tree).index(panel)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected NodeTree interface panel creation") from error
        self.changed()
        return {"ref": self._ref(root, anchor_name, path + [["c", "items_tree", index]]), "name": panel.name, "item_type": "PANEL", "changed": True, "generation": self.generation}

    def node_interface_item_remove(self, item_ref, move_content_to_parent=True):
        root, anchor_name, parent_path, interface, item = self._collection_parent(item_ref, "items_tree")
        if _text(getattr(getattr(interface, "bl_rna", None), "identifier", ""), 256) != "NodeTreeInterface" or not isinstance(move_content_to_parent, bool):
            raise SemanticError("InvalidArgument", "Interface item reference or options are invalid")
        try:
            interface.remove(item, move_content_to_parent=move_content_to_parent)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected NodeTree interface item removal") from error
        self.changed()
        return {"interface_ref": self._ref(root, anchor_name, parent_path), "changed": True, "generation": self.generation}

    def node_interface_item_move(self, item_ref, to_position):
        root, anchor_name, parent_path, interface, item = self._collection_parent(item_ref, "items_tree")
        if _text(getattr(getattr(interface, "bl_rna", None), "identifier", ""), 256) != "NodeTreeInterface" or isinstance(to_position, bool) or not isinstance(to_position, int) or not 0 <= to_position <= 4096:
            raise SemanticError("InvalidArgument", "Interface item move arguments are invalid")
        try:
            interface.move(item, to_position)
            index = list(interface.items_tree).index(item)
        except Exception as error:
            raise SemanticError("InvalidArgument", "Blender rejected NodeTree interface item move") from error
        self.changed()
        return {"ref": self._ref(root, anchor_name, parent_path + [["c", "items_tree", index]]), "position": to_position, "changed": True, "generation": self.generation}

    def node_interface_item_move_to_parent(self, item_ref, parent_ref, to_position):
        root, anchor_name, parent_path, interface, item = self._collection_parent(item_ref, "items_tree")
        parent = self._interface_parent(root, anchor_name, interface, parent_ref)
        if parent is None or isinstance(to_position, bool) or not isinstance(to_position, int) or not 0 <= to_position <= 4096:
            raise SemanticError("InvalidArgument", "Interface reparent arguments are invalid")
        try:
            interface.move_to_parent(item, parent, to_position)
            index = list(interface.items_tree).index(item)
        except Exception as error:
            raise SemanticError("InvalidArgument", "Blender rejected NodeTree interface reparent") from error
        self.changed()
        return {"ref": self._ref(root, anchor_name, parent_path + [["c", "items_tree", index]]), "position": to_position, "changed": True, "generation": self.generation}

    def _animation_owner(self, owner_ref):
        root, name, path, owner = self._resolve(owner_ref)
        creator = getattr(owner, "animation_data_create", None)
        if not callable(creator):
            raise SemanticError("Unsupported", "RNA object cannot own animation data")
        return root, name, path, owner

    def _require_action(self, action_ref):
        root, name, path, action = self._resolve(action_ref)
        if root != "actions" or path or _text(getattr(getattr(action, "bl_rna", None), "identifier", ""), 256) != "Action":
            raise SemanticError("InvalidArgument", "Operation requires a root Action datablock ref")
        return root, name, path, action

    def action_slot_add(self, action_ref, id_type, name):
        root, action_name, path, action = self._require_action(action_ref)
        allowed = {
            "OBJECT", "MESH", "CURVE", "SURFACE", "FONT", "META", "ARMATURE", "LATTICE",
            "CAMERA", "LIGHT", "SPEAKER", "MATERIAL", "WORLD", "SCENE", "KEY", "POINTCLOUD",
            "CURVES", "GREASEPENCIL", "VOLUME",
        }
        if id_type not in allowed:
            raise SemanticError("InvalidArgument", "Action slot ID type is outside the managed persistent boundary")
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Action slot name is invalid")
        try:
            slot = action.slots.new(id_type=id_type, name=name)
            index = list(action.slots).index(slot)
        except Exception as error:
            raise SemanticError("Unsupported", "Blender rejected Action slot creation") from error
        self.changed()
        return {
            "ref": self._ref(root, action_name, path + [["c", "slots", index]]),
            "name": _text(getattr(slot, "name_display", name), 128),
            "id_type": _text(getattr(slot, "target_id_type", id_type), 64),
            "changed": True,
            "generation": self.generation,
        }

    def action_slot_remove(self, slot_ref):
        root, action_name, parent_path, action, slot = self._collection_parent(slot_ref, "slots")
        if _text(getattr(getattr(action, "bl_rna", None), "identifier", ""), 256) != "Action":
            raise SemanticError("InvalidArgument", "Action slot parent is not an Action")
        try:
            action.slots.remove(slot)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Action slot removal") from error
        self.changed()
        return {
            "action_ref": self._ref(root, action_name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def action_layer_add(self, action_ref, name):
        root, action_name, path, action = self._require_action(action_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Action layer name is invalid")
        if any(layer.name == name for layer in action.layers):
            raise SemanticError("Conflict", "Action layer name already exists")
        try:
            layer = action.layers.new(name)
            if layer.name != name:
                action.layers.remove(layer)
                raise SemanticError("Conflict", "Blender rewrote the requested Action layer name")
            index = list(action.layers).index(layer)
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("Unsupported", "Blender rejected Action layer creation") from error
        self.changed()
        return {
            "ref": self._ref(root, action_name, path + [["c", "layers", index]]),
            "name": layer.name,
            "changed": True,
            "generation": self.generation,
        }

    def action_layer_remove(self, layer_ref):
        root, action_name, parent_path, action, layer = self._collection_parent(layer_ref, "layers")
        if _text(getattr(getattr(action, "bl_rna", None), "identifier", ""), 256) != "Action":
            raise SemanticError("InvalidArgument", "Action layer parent is not an Action")
        try:
            action.layers.remove(layer)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Action layer removal") from error
        self.changed()
        return {
            "action_ref": self._ref(root, action_name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def action_strip_add(self, layer_ref):
        root, action_name, path, layer = self._resolve(layer_ref)
        if _text(getattr(getattr(layer, "bl_rna", None), "identifier", ""), 256) != "ActionLayer":
            raise SemanticError("InvalidArgument", "Operation requires an ActionLayer ref")
        if len(layer.strips) >= 1:
            raise SemanticError("Conflict", "Blender 4.5 ActionLayer supports at most one keyframe strip")
        try:
            strip = layer.strips.new(type="KEYFRAME")
            index = list(layer.strips).index(strip)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Action keyframe strip creation") from error
        self.changed()
        return {
            "ref": self._ref(root, action_name, path + [["c", "strips", index]]),
            "type": _text(getattr(strip, "type", "KEYFRAME"), 64),
            "changed": True,
            "generation": self.generation,
        }

    def action_strip_remove(self, strip_ref):
        root, action_name, parent_path, layer, strip = self._collection_parent(strip_ref, "strips")
        if _text(getattr(getattr(layer, "bl_rna", None), "identifier", ""), 256) != "ActionLayer":
            raise SemanticError("InvalidArgument", "Action strip parent is not an ActionLayer")
        try:
            layer.strips.remove(strip)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Action strip removal") from error
        self.changed()
        return {
            "layer_ref": self._ref(root, action_name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def action_channelbag_ensure(self, strip_ref, slot_ref):
        root, action_name, path, strip = self._resolve(strip_ref)
        slot_parts = self._resolve(slot_ref)
        if _text(getattr(getattr(strip, "bl_rna", None), "identifier", ""), 256) != "ActionKeyframeStrip":
            raise SemanticError("InvalidArgument", "Operation requires an ActionKeyframeStrip ref")
        slot = slot_parts[3]
        if _text(getattr(getattr(slot, "bl_rna", None), "identifier", ""), 256) != "ActionSlot":
            raise SemanticError("InvalidArgument", "slot_ref must reference an ActionSlot")
        if (slot_parts[0], slot_parts[1]) != (root, action_name):
            raise SemanticError("InvalidArgument", "Action strip and slot must belong to the same Action")
        existing = None
        for candidate in strip.channelbags:
            if getattr(candidate, "slot", None) is slot:
                existing = candidate
                break
        try:
            channelbag = existing if existing is not None else strip.channelbag(slot, ensure=True)
            if channelbag is None:
                raise SemanticError("BackendFailed", "Blender did not create an Action channelbag")
            index = list(strip.channelbags).index(channelbag)
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Action channelbag creation") from error
        changed = existing is None
        if changed:
            self.changed()
        return {
            "ref": self._ref(root, action_name, path + [["c", "channelbags", index]]),
            "created": changed,
            "generation": self.generation,
        }

    def action_fcurve_ensure(self, channelbag_ref, data_path, index=0, group_name=""):
        root, action_name, path, channelbag = self._resolve(channelbag_ref)
        if _text(getattr(getattr(channelbag, "bl_rna", None), "identifier", ""), 256) != "ActionChannelbag":
            raise SemanticError("InvalidArgument", "Operation requires an ActionChannelbag ref")
        if not isinstance(data_path, str) or not 1 <= len(data_path) <= 512 or "\x00" in data_path:
            raise SemanticError("InvalidArgument", "F-Curve data path is invalid")
        allowed_path = re.fullmatch(
            r'[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*|\["[A-Za-z0-9_.:-]{1,128}"\])*',
            data_path,
        )
        if allowed_path is None:
            raise SemanticError("PolicyDenied", "F-Curve data path is outside the bounded semantic path grammar")
        if isinstance(index, bool) or not isinstance(index, int) or not 0 <= index < MAX_ARRAY:
            raise SemanticError("InvalidArgument", "F-Curve array index is invalid")
        if not isinstance(group_name, str) or len(group_name) > 128 or "\x00" in group_name:
            raise SemanticError("InvalidArgument", "F-Curve group name is invalid")
        try:
            existing = channelbag.fcurves.find(data_path, index=index)
            curve = existing
            if curve is None:
                curve = channelbag.fcurves.new(data_path, index=index)
                if group_name:
                    group = channelbag.groups.get(group_name)
                    if group is None:
                        group = channelbag.groups.new(group_name)
                    curve.group = group
            position = list(channelbag.fcurves).index(curve)
        except Exception as error:
            raise SemanticError("InvalidArgument", "Blender 4.5 rejected Action F-Curve ensure") from error
        changed = existing is None
        if changed:
            self.changed()
        return {
            "ref": self._ref(root, action_name, path + [["c", "fcurves", position]]),
            "data_path": curve.data_path,
            "index": curve.array_index,
            "created": changed,
            "generation": self.generation,
        }

    def action_fcurve_remove(self, fcurve_ref):
        root, action_name, parent_path, channelbag, curve = self._collection_parent(fcurve_ref, "fcurves")
        if _text(getattr(getattr(channelbag, "bl_rna", None), "identifier", ""), 256) != "ActionChannelbag":
            raise SemanticError("InvalidArgument", "F-Curve parent is not an ActionChannelbag")
        try:
            channelbag.fcurves.remove(curve)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Action F-Curve removal") from error
        self.changed()
        return {
            "channelbag_ref": self._ref(root, action_name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def nla_track_add(self, owner_ref, name, previous_ref=None):
        root, anchor_name, path, owner = self._animation_owner(owner_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "NLA track name is invalid")
        try:
            animation = owner.animation_data_create()
            tracks = animation.nla_tracks
        except Exception as error:
            raise SemanticError("Unsupported", "Blender could not create animation data for this owner") from error
        if any(track.name == name for track in tracks):
            raise SemanticError("Conflict", "NLA track name already exists")
        previous = None
        if previous_ref is not None:
            previous = self._resolve(previous_ref)[3]
            if previous not in list(tracks):
                raise SemanticError("InvalidArgument", "Previous NLA track must belong to the same animation owner")
        try:
            track = tracks.new(prev=previous)
            track.name = name
            if track.name != name:
                tracks.remove(track)
                raise SemanticError("Conflict", "Blender rewrote the requested NLA track name")
            index = list(tracks).index(track)
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected NLA track creation") from error
        self.changed()
        track_path = path + [["p", "animation_data"], ["c", "nla_tracks", index]]
        return {"ref": self._ref(root, anchor_name, track_path), "name": track.name, "changed": True, "generation": self.generation}

    def nla_track_remove(self, track_ref):
        root, anchor_name, parent_path, animation, track = self._collection_parent(track_ref, "nla_tracks")
        if _text(getattr(getattr(animation, "bl_rna", None), "identifier", ""), 256) != "AnimData":
            raise SemanticError("InvalidArgument", "NLA track parent is not AnimData")
        try:
            animation.nla_tracks.remove(track)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected NLA track removal") from error
        owner_path = parent_path[:-1] if parent_path and parent_path[-1] == ["p", "animation_data"] else parent_path
        self.changed()
        return {"owner_ref": self._ref(root, anchor_name, owner_path), "changed": True, "generation": self.generation}

    def nla_strip_add(self, track_ref, name, start, action_ref):
        root, anchor_name, path, track = self._resolve(track_ref)
        if _text(getattr(getattr(track, "bl_rna", None), "identifier", ""), 256) != "NlaTrack":
            raise SemanticError("InvalidArgument", "Operation requires an NlaTrack ref")
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "NLA strip name is invalid")
        if isinstance(start, bool) or not isinstance(start, int) or not -1_000_000 <= start <= 1_000_000:
            raise SemanticError("InvalidArgument", "NLA strip start frame is invalid")
        action_parts = self._resolve(action_ref)
        if action_parts[0] != "actions" or action_parts[2] or _text(getattr(getattr(action_parts[3], "bl_rna", None), "identifier", ""), 256) != "Action":
            raise SemanticError("InvalidArgument", "action_ref must be a root Action datablock")
        if any(strip.name == name for strip in track.strips):
            raise SemanticError("Conflict", "NLA strip name already exists")
        try:
            strip = track.strips.new(name, start, action_parts[3])
            if strip.name != name:
                track.strips.remove(strip)
                raise SemanticError("Conflict", "Blender rewrote the requested NLA strip name")
            index = list(track.strips).index(strip)
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("InvalidArgument", "Blender rejected NLA strip creation") from error
        self.changed()
        return {"ref": self._ref(root, anchor_name, path + [["c", "strips", index]]), "name": strip.name, "start": int(strip.frame_start), "changed": True, "generation": self.generation}

    def nla_strip_remove(self, strip_ref):
        root, anchor_name, parent_path, track, strip = self._collection_parent(strip_ref, "strips")
        if _text(getattr(getattr(track, "bl_rna", None), "identifier", ""), 256) != "NlaTrack":
            raise SemanticError("InvalidArgument", "NLA strip parent is not an NlaTrack")
        try:
            track.strips.remove(strip)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected NLA strip removal") from error
        self.changed()
        return {"track_ref": self._ref(root, anchor_name, parent_path), "changed": True, "generation": self.generation}

    def _require_fcurve(self, reference):
        root, name, path, curve = self._resolve(reference)
        if _text(getattr(getattr(curve, "bl_rna", None), "identifier", ""), 256) != "FCurve":
            raise SemanticError("InvalidArgument", "Operation requires an FCurve RNA reference")
        return root, name, path, curve

    def fcurve_keyframe_add(self, fcurve_ref, frame, value, keyframe_type="KEYFRAME"):
        root, name, path, curve = self._require_fcurve(fcurve_ref)
        if not _finite(frame) or not -1_000_000 <= float(frame) <= 1_000_000:
            raise SemanticError("InvalidArgument", "F-Curve keyframe frame is outside bounded limits")
        if not _finite(value) or abs(float(value)) > 1_000_000_000:
            raise SemanticError("InvalidArgument", "F-Curve keyframe value is outside bounded limits")
        if keyframe_type not in {"KEYFRAME", "BREAKDOWN", "MOVING_HOLD", "EXTREME", "JITTER", "GENERATED"}:
            raise SemanticError("InvalidArgument", "F-Curve keyframe type is invalid")
        try:
            target_frame = float(frame)
            point = next(
                (
                    candidate
                    for candidate in curve.keyframe_points
                    if abs(float(candidate.co[0]) - target_frame) <= 1e-6
                ),
                None,
            )
            if point is None:
                point = curve.keyframe_points.insert(
                    target_frame,
                    float(value),
                    keyframe_type=keyframe_type,
                )
            else:
                point.co[1] = float(value)
                point.type = keyframe_type
            index = list(curve.keyframe_points).index(point)
            curve.update()
        except Exception as error:
            raise SemanticError("InvalidArgument", "Blender rejected F-Curve keyframe insertion") from error
        self.changed()
        return {
            "ref": self._ref(root, name, path + [["c", "keyframe_points", index]]),
            "frame": float(point.co[0]),
            "value": float(point.co[1]),
            "changed": True,
            "generation": self.generation,
        }

    def fcurve_keyframe_remove(self, keyframe_ref):
        root, name, parent_path, curve, point = self._collection_parent(keyframe_ref, "keyframe_points")
        if _text(getattr(getattr(curve, "bl_rna", None), "identifier", ""), 256) != "FCurve":
            raise SemanticError("InvalidArgument", "Keyframe parent is not an FCurve")
        try:
            curve.keyframe_points.remove(point, fast=False)
            curve.update()
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected F-Curve keyframe removal") from error
        self.changed()
        return {
            "fcurve_ref": self._ref(root, name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def fcurve_modifier_add(self, fcurve_ref, modifier_type):
        root, name, path, curve = self._require_fcurve(fcurve_ref)
        if modifier_type not in {"GENERATOR", "FNGENERATOR", "ENVELOPE", "CYCLES", "NOISE", "LIMITS", "STEPPED"}:
            raise SemanticError("InvalidArgument", "F-Curve modifier type is invalid")
        try:
            modifier = curve.modifiers.new(type=modifier_type)
            index = list(curve.modifiers).index(modifier)
        except Exception as error:
            raise SemanticError("Unsupported", "Blender rejected F-Curve modifier creation") from error
        self.changed()
        return {
            "ref": self._ref(root, name, path + [["c", "modifiers", index]]),
            "type": modifier.type,
            "changed": True,
            "generation": self.generation,
        }

    def fcurve_modifier_remove(self, modifier_ref):
        root, name, parent_path, curve, modifier = self._collection_parent(modifier_ref, "modifiers")
        if _text(getattr(getattr(curve, "bl_rna", None), "identifier", ""), 256) != "FCurve":
            raise SemanticError("InvalidArgument", "F-Curve modifier parent is not an FCurve")
        try:
            curve.modifiers.remove(modifier)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected F-Curve modifier removal") from error
        self.changed()
        return {
            "fcurve_ref": self._ref(root, name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def _require_scene(self, reference):
        root, name, path, scene = self._resolve(reference)
        if _text(getattr(getattr(scene, "bl_rna", None), "identifier", ""), 256) != "Scene":
            raise SemanticError("InvalidArgument", "Operation requires a Scene RNA reference")
        return root, name, path, scene

    def view_layer_add(self, scene_ref, name):
        root, scene_name, path, scene = self._require_scene(scene_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "View-layer name is invalid")
        if scene.view_layers.get(name) is not None:
            raise SemanticError("Conflict", "View-layer name already exists")
        try:
            layer = scene.view_layers.new(name)
            index = list(scene.view_layers).index(layer)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected view-layer creation") from error
        self.changed()
        return {
            "ref": self._ref(root, scene_name, path + [["c", "view_layers", index]]),
            "name": layer.name,
            "changed": True,
            "generation": self.generation,
        }

    def view_layer_remove(self, layer_ref):
        root, scene_name, parent_path, scene, layer = self._collection_parent(layer_ref, "view_layers")
        if _text(getattr(getattr(scene, "bl_rna", None), "identifier", ""), 256) != "Scene":
            raise SemanticError("InvalidArgument", "View-layer parent is not a Scene")
        if len(scene.view_layers) <= 1:
            raise SemanticError("Conflict", "Blender Scene must retain at least one view layer")
        try:
            scene.view_layers.remove(layer)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected view-layer removal") from error
        self.changed()
        return {
            "scene_ref": self._ref(root, scene_name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def view_layer_move(self, layer_ref, to_index):
        root, scene_name, parent_path, scene, layer = self._collection_parent(layer_ref, "view_layers")
        if _text(getattr(getattr(scene, "bl_rna", None), "identifier", ""), 256) != "Scene":
            raise SemanticError("InvalidArgument", "View-layer parent is not a Scene")
        if isinstance(to_index, bool) or not isinstance(to_index, int) or not 0 <= to_index < len(scene.view_layers):
            raise SemanticError("InvalidArgument", "View-layer target index is invalid")
        try:
            from_index = list(scene.view_layers).index(layer)
            scene.view_layers.move(from_index, to_index)
            new_index = list(scene.view_layers).index(layer)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected view-layer reorder") from error
        self.changed()
        return {
            "ref": self._ref(root, scene_name, parent_path + [["c", "view_layers", new_index]]),
            "index": new_index,
            "changed": from_index != new_index,
            "generation": self.generation,
        }

    def timeline_marker_add(self, scene_ref, name, frame):
        root, scene_name, path, scene = self._require_scene(scene_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Timeline-marker name is invalid")
        if isinstance(frame, bool) or not isinstance(frame, int) or not -1_048_574 <= frame <= 1_048_574:
            raise SemanticError("InvalidArgument", "Timeline-marker frame is invalid")
        if any(marker.name == name for marker in scene.timeline_markers):
            raise SemanticError("Conflict", "Timeline-marker name already exists")
        try:
            marker = scene.timeline_markers.new(name, frame=frame)
            index = list(scene.timeline_markers).index(marker)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected timeline-marker creation") from error
        self.changed()
        return {
            "ref": self._ref(root, scene_name, path + [["c", "timeline_markers", index]]),
            "name": marker.name,
            "frame": marker.frame,
            "changed": True,
            "generation": self.generation,
        }

    def timeline_marker_remove(self, marker_ref):
        root, scene_name, parent_path, scene, marker = self._collection_parent(marker_ref, "timeline_markers")
        if _text(getattr(getattr(scene, "bl_rna", None), "identifier", ""), 256) != "Scene":
            raise SemanticError("InvalidArgument", "Timeline-marker parent is not a Scene")
        try:
            scene.timeline_markers.remove(marker)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected timeline-marker removal") from error
        self.changed()
        return {
            "scene_ref": self._ref(root, scene_name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def _sequence_editor(self, scene_ref, create=True):
        root, scene_name, path, scene = self._require_scene(scene_ref)
        editor = scene.sequence_editor
        created = False
        if editor is None and create:
            try:
                editor = scene.sequence_editor_create()
                created = True
            except Exception as error:
                raise SemanticError("BackendFailed", "Blender rejected Sequence Editor creation") from error
        if editor is None:
            raise SemanticError("NotFound", "Scene does not have a Sequence Editor")
        return root, scene_name, path, scene, editor, created

    def sequence_editor_ensure(self, scene_ref):
        root, scene_name, path, _scene, _editor, created = self._sequence_editor(scene_ref, True)
        if created:
            self.changed()
        return {
            "ref": self._ref(root, scene_name, path + [["p", "sequence_editor"]]),
            "created": created,
            "generation": self.generation,
        }

    def _sequence_name(self, editor, name):
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Sequence strip name is invalid")
        if any(strip.name == name for strip in editor.strips):
            raise SemanticError("Conflict", "Sequence strip name already exists")
        return name

    def _sequence_position(self, channel, frame_start):
        if isinstance(channel, bool) or not isinstance(channel, int) or not 1 <= channel <= 128:
            raise SemanticError("InvalidArgument", "Sequence channel must be in [1,128]")
        if isinstance(frame_start, bool) or not isinstance(frame_start, int) or not -1_048_574 <= frame_start <= 1_048_574:
            raise SemanticError("InvalidArgument", "Sequence start frame is invalid")
        return channel, frame_start

    def _sequence_strip_ref(self, root, scene_name, scene_path, editor, strip):
        index = list(editor.strips).index(strip)
        return self._ref(root, scene_name, scene_path + [["p", "sequence_editor"], ["c", "strips", index]])

    def sequence_media_add(self, scene_ref, kind, name, relative_path, channel, frame_start, fit_method="ORIGINAL"):
        root, scene_name, path, _scene, editor, _created = self._sequence_editor(scene_ref, True)
        self._sequence_name(editor, name)
        channel, frame_start = self._sequence_position(channel, frame_start)
        if fit_method not in {"FIT", "FILL", "STRETCH", "ORIGINAL"}:
            raise SemanticError("InvalidArgument", "Sequence fit method is invalid")
        extensions = {
            "IMAGE": {".png", ".jpg", ".jpeg", ".webp", ".tif", ".tiff", ".bmp", ".exr", ".hdr"},
            "MOVIE": {".mp4", ".mov", ".avi", ".mkv", ".webm"},
            "SOUND": {".wav", ".flac", ".ogg", ".mp3"},
        }
        if kind not in extensions:
            raise SemanticError("InvalidArgument", "Sequence media kind is invalid")
        source, _size = self._asset_path(relative_path, extensions[kind])
        strip = None
        try:
            if kind == "IMAGE":
                strip = editor.strips.new_image(name, str(source), channel, frame_start, fit_method=fit_method)
            elif kind == "MOVIE":
                strip = editor.strips.new_movie(name, str(source), channel, frame_start, fit_method=fit_method)
            else:
                strip = editor.strips.new_sound(name, str(source), channel, frame_start)
            if strip.name != name:
                editor.strips.remove(strip)
                raise SemanticError("Conflict", "Blender rewrote the requested Sequence strip name")
        except SemanticError:
            raise
        except Exception as error:
            if strip is not None:
                try:
                    editor.strips.remove(strip)
                except Exception:
                    pass
            raise SemanticError("InvalidArgument", "Blender rejected scoped Sequence media") from error
        self.changed()
        return {
            "ref": self._sequence_strip_ref(root, scene_name, path, editor, strip),
            "kind": kind,
            "name": strip.name,
            "channel": strip.channel,
            "frame_start": int(strip.frame_start),
            "path": relative_path,
            "changed": True,
            "generation": self.generation,
        }

    def sequence_datablock_add(self, scene_ref, kind, name, source_ref, channel, frame_start):
        root, scene_name, path, scene, editor, _created = self._sequence_editor(scene_ref, True)
        self._sequence_name(editor, name)
        channel, frame_start = self._sequence_position(channel, frame_start)
        allowed = {"SCENE": ("scenes", "Scene"), "CLIP": ("movieclips", "MovieClip"), "MASK": ("masks", "Mask")}
        if kind not in allowed:
            raise SemanticError("InvalidArgument", "Sequence datablock kind is invalid")
        source_parts = self._resolve(source_ref)
        expected_root, expected_type = allowed[kind]
        if source_parts[0] != expected_root or source_parts[2]:
            raise SemanticError("InvalidArgument", "Sequence source_ref must be a root datablock of the requested kind")
        source = source_parts[3]
        if _text(getattr(getattr(source, "bl_rna", None), "identifier", ""), 256) != expected_type:
            raise SemanticError("InvalidArgument", "Sequence source_ref type does not match the requested kind")
        if kind == "SCENE" and source is scene:
            raise SemanticError("Conflict", "A Scene cannot sequence itself")
        strip = None
        try:
            if kind == "SCENE":
                strip = editor.strips.new_scene(name, source, channel, frame_start)
            elif kind == "CLIP":
                strip = editor.strips.new_clip(name, source, channel, frame_start)
            else:
                strip = editor.strips.new_mask(name, source, channel, frame_start)
            if strip.name != name:
                editor.strips.remove(strip)
                raise SemanticError("Conflict", "Blender rewrote the requested Sequence strip name")
        except SemanticError:
            raise
        except Exception as error:
            if strip is not None:
                try:
                    editor.strips.remove(strip)
                except Exception:
                    pass
            raise SemanticError("InvalidArgument", "Blender rejected Sequence datablock strip creation") from error
        self.changed()
        return {
            "ref": self._sequence_strip_ref(root, scene_name, path, editor, strip),
            "kind": kind,
            "name": strip.name,
            "channel": strip.channel,
            "frame_start": int(strip.frame_start),
            "changed": True,
            "generation": self.generation,
        }

    def sequence_meta_add(self, scene_ref, name, channel, frame_start):
        root, scene_name, path, _scene, editor, _created = self._sequence_editor(scene_ref, True)
        self._sequence_name(editor, name)
        channel, frame_start = self._sequence_position(channel, frame_start)
        try:
            strip = editor.strips.new_meta(name, channel, frame_start)
        except Exception as error:
            raise SemanticError("InvalidArgument", "Blender rejected meta-strip creation") from error
        self.changed()
        return {
            "ref": self._sequence_strip_ref(root, scene_name, path, editor, strip),
            "name": strip.name,
            "channel": strip.channel,
            "frame_start": int(strip.frame_start),
            "changed": True,
            "generation": self.generation,
        }

    def _sequence_input(self, root, scene_name, reference):
        if reference is None:
            return None
        parts = self._resolve(reference)
        if (parts[0], parts[1]) != (root, scene_name):
            raise SemanticError("InvalidArgument", "Sequence effect inputs must belong to the same Scene")
        strip_type = getattr(self.bpy.types, "Strip", None)
        if strip_type is None or not isinstance(parts[3], strip_type):
            raise SemanticError("InvalidArgument", "Sequence effect input must be a Strip ref")
        return parts[3]

    def sequence_effect_add(self, scene_ref, name, effect_type, channel, frame_start, frame_end=0, input1_ref=None, input2_ref=None):
        root, scene_name, path, _scene, editor, _created = self._sequence_editor(scene_ref, True)
        self._sequence_name(editor, name)
        channel, frame_start = self._sequence_position(channel, frame_start)
        if isinstance(frame_end, bool) or not isinstance(frame_end, int) or not 0 <= frame_end <= 1_048_574:
            raise SemanticError("InvalidArgument", "Sequence effect end frame is invalid")
        effects = {
            "CROSS", "ADD", "SUBTRACT", "ALPHA_OVER", "ALPHA_UNDER", "GAMMA_CROSS",
            "MULTIPLY", "WIPE", "GLOW", "TRANSFORM", "COLOR", "SPEED", "MULTICAM",
            "ADJUSTMENT", "GAUSSIAN_BLUR", "TEXT", "COLORMIX",
        }
        if effect_type not in effects:
            raise SemanticError("InvalidArgument", "Sequence effect type is invalid")
        input1 = self._sequence_input(root, scene_name, input1_ref)
        input2 = self._sequence_input(root, scene_name, input2_ref)
        try:
            strip = editor.strips.new_effect(
                name,
                effect_type,
                channel,
                frame_start,
                frame_end=frame_end,
                input1=input1,
                input2=input2,
            )
        except Exception as error:
            raise SemanticError("InvalidArgument", "Blender rejected Sequence effect creation or inputs") from error
        self.changed()
        return {
            "ref": self._sequence_strip_ref(root, scene_name, path, editor, strip),
            "name": strip.name,
            "effect_type": effect_type,
            "channel": strip.channel,
            "frame_start": int(strip.frame_start),
            "changed": True,
            "generation": self.generation,
        }

    def sequence_strip_remove(self, strip_ref):
        root, scene_name, parent_path, editor, strip = self._collection_parent(strip_ref, "strips")
        if _text(getattr(getattr(editor, "bl_rna", None), "identifier", ""), 256) != "SequenceEditor":
            raise SemanticError("InvalidArgument", "Sequence strip parent is not a SequenceEditor")
        try:
            editor.strips.remove(strip)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Sequence strip removal") from error
        self.changed()
        return {
            "editor_ref": self._ref(root, scene_name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def _require_strip(self, reference):
        root, scene_name, path, strip = self._resolve(reference)
        strip_type = getattr(self.bpy.types, "Strip", None)
        if strip_type is None or not isinstance(strip, strip_type):
            raise SemanticError("InvalidArgument", "Operation requires a Sequence Strip ref")
        return root, scene_name, path, strip

    def sequence_modifier_add(self, strip_ref, name, modifier_type):
        root, scene_name, path, strip = self._require_strip(strip_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Sequence modifier name is invalid")
        if strip.modifiers.get(name) is not None:
            raise SemanticError("Conflict", "Sequence modifier name already exists")
        if not isinstance(modifier_type, str) or not re.fullmatch(r"[A-Z][A-Z0-9_]{0,63}", modifier_type):
            raise SemanticError("InvalidArgument", "Sequence modifier type is invalid")
        try:
            modifier = strip.modifiers.new(name, modifier_type)
            if modifier.name != name:
                strip.modifiers.remove(modifier)
                raise SemanticError("Conflict", "Blender rewrote the requested Sequence modifier name")
            index = list(strip.modifiers).index(modifier)
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("Unsupported", "Blender rejected Sequence modifier creation") from error
        self.changed()
        return {
            "ref": self._ref(root, scene_name, path + [["c", "modifiers", index]]),
            "name": modifier.name,
            "type": modifier.type,
            "changed": True,
            "generation": self.generation,
        }

    def sequence_modifier_remove(self, modifier_ref):
        root, scene_name, parent_path, strip, modifier = self._collection_parent(modifier_ref, "modifiers")
        strip_type = getattr(self.bpy.types, "Strip", None)
        if strip_type is None or not isinstance(strip, strip_type):
            raise SemanticError("InvalidArgument", "Sequence modifier parent is not a Strip")
        try:
            strip.modifiers.remove(modifier)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Sequence modifier removal") from error
        self.changed()
        return {
            "strip_ref": self._ref(root, scene_name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def _require_mask(self, reference):
        root, name, path, mask = self._resolve(reference)
        if root != "masks" or path or _text(getattr(getattr(mask, "bl_rna", None), "identifier", ""), 256) != "Mask":
            raise SemanticError("InvalidArgument", "Operation requires a root Mask datablock ref")
        return root, name, path, mask

    def mask_layer_add(self, mask_ref, name):
        root, mask_name, path, mask = self._require_mask(mask_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Mask layer name is invalid")
        if any(layer.name == name for layer in mask.layers):
            raise SemanticError("Conflict", "Mask layer name already exists")
        try:
            layer = mask.layers.new(name=name)
            if layer.name != name:
                mask.layers.remove(layer)
                raise SemanticError("Conflict", "Blender rewrote the requested Mask layer name")
            index = list(mask.layers).index(layer)
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Mask layer creation") from error
        self.changed()
        return {
            "ref": self._ref(root, mask_name, path + [["c", "layers", index]]),
            "name": layer.name,
            "changed": True,
            "generation": self.generation,
        }

    def mask_layer_remove(self, layer_ref):
        root, mask_name, parent_path, mask, layer = self._collection_parent(layer_ref, "layers")
        if _text(getattr(getattr(mask, "bl_rna", None), "identifier", ""), 256) != "Mask":
            raise SemanticError("InvalidArgument", "Mask layer parent is not a Mask")
        try:
            mask.layers.remove(layer)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Mask layer removal") from error
        self.changed()
        return {
            "mask_ref": self._ref(root, mask_name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def mask_spline_add(self, layer_ref, points=1):
        root, mask_name, path, layer = self._resolve(layer_ref)
        if _text(getattr(getattr(layer, "bl_rna", None), "identifier", ""), 256) != "MaskLayer":
            raise SemanticError("InvalidArgument", "Operation requires a MaskLayer ref")
        if isinstance(points, bool) or not isinstance(points, int) or not 1 <= points <= 10000:
            raise SemanticError("InvalidArgument", "Mask spline point count is outside bounded limits")
        try:
            spline = layer.splines.new()
            if points > 1:
                spline.points.add(points - 1)
            index = list(layer.splines).index(spline)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Mask spline creation") from error
        self.changed()
        return {
            "ref": self._ref(root, mask_name, path + [["c", "splines", index]]),
            "points": len(spline.points),
            "changed": True,
            "generation": self.generation,
        }

    def mask_spline_remove(self, spline_ref):
        root, mask_name, parent_path, layer, spline = self._collection_parent(spline_ref, "splines")
        if _text(getattr(getattr(layer, "bl_rna", None), "identifier", ""), 256) != "MaskLayer":
            raise SemanticError("InvalidArgument", "Mask spline parent is not a MaskLayer")
        try:
            layer.splines.remove(spline)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Mask spline removal") from error
        self.changed()
        return {
            "layer_ref": self._ref(root, mask_name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def mask_points_add(self, spline_ref, count):
        root, mask_name, path, spline = self._resolve(spline_ref)
        if _text(getattr(getattr(spline, "bl_rna", None), "identifier", ""), 256) != "MaskSpline":
            raise SemanticError("InvalidArgument", "Operation requires a MaskSpline ref")
        if isinstance(count, bool) or not isinstance(count, int) or not 1 <= count <= 10000:
            raise SemanticError("InvalidArgument", "Mask point count is outside bounded limits")
        before = len(spline.points)
        if before + count > 10000:
            raise SemanticError("InvalidArgument", "Mask spline would exceed the bounded point ceiling")
        try:
            spline.points.add(count)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Mask point creation") from error
        self.changed()
        return {
            "ref": self._ref(root, mask_name, path),
            "points_added": count,
            "point_count": len(spline.points),
            "changed": True,
            "generation": self.generation,
        }

    def mask_point_remove(self, point_ref):
        root, mask_name, parent_path, spline, point = self._collection_parent(point_ref, "points")
        if _text(getattr(getattr(spline, "bl_rna", None), "identifier", ""), 256) != "MaskSpline":
            raise SemanticError("InvalidArgument", "Mask point parent is not a MaskSpline")
        if len(spline.points) <= 1:
            raise SemanticError("Conflict", "Mask spline must retain at least one point")
        try:
            spline.points.remove(point)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Mask point removal") from error
        self.changed()
        return {
            "spline_ref": self._ref(root, mask_name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def _require_tracking(self, reference):
        root, name, path, tracking = self._resolve(reference)
        if _text(getattr(getattr(tracking, "bl_rna", None), "identifier", ""), 256) != "MovieTracking":
            raise SemanticError("InvalidArgument", "Operation requires a MovieTracking RNA ref")
        return root, name, path, tracking

    def tracking_object_add(self, tracking_ref, name):
        root, clip_name, path, tracking = self._require_tracking(tracking_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Tracking object name is invalid")
        if tracking.objects.get(name) is not None:
            raise SemanticError("Conflict", "Tracking object name already exists")
        try:
            obj = tracking.objects.new(name)
            if obj.name != name:
                tracking.objects.remove(obj)
                raise SemanticError("Conflict", "Blender rewrote the requested tracking object name")
            index = list(tracking.objects).index(obj)
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected tracking object creation") from error
        self.changed()
        return {
            "ref": self._ref(root, clip_name, path + [["c", "objects", index]]),
            "name": obj.name,
            "changed": True,
            "generation": self.generation,
        }

    def tracking_object_remove(self, object_ref):
        root, clip_name, parent_path, tracking, obj = self._collection_parent(object_ref, "objects")
        if _text(getattr(getattr(tracking, "bl_rna", None), "identifier", ""), 256) != "MovieTracking":
            raise SemanticError("InvalidArgument", "Tracking object parent is not MovieTracking")
        try:
            tracking.objects.remove(obj)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected tracking object removal") from error
        self.changed()
        return {
            "tracking_ref": self._ref(root, clip_name, parent_path),
            "changed": True,
            "generation": self.generation,
        }

    def tracking_track_add(self, object_ref, name, frame=1, co=None):
        root, clip_name, path, obj = self._resolve(object_ref)
        if _text(getattr(getattr(obj, "bl_rna", None), "identifier", ""), 256) != "MovieTrackingObject":
            raise SemanticError("InvalidArgument", "Operation requires a MovieTrackingObject ref")
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Tracking track name is invalid")
        if isinstance(frame, bool) or not isinstance(frame, int) or not 0 <= frame <= 1_048_574:
            raise SemanticError("InvalidArgument", "Tracking frame is outside Blender bounds")
        if co is None:
            co = [0.0, 0.0]
        if not isinstance(co, list) or len(co) != 2 or any(not _finite(v) or not -1.0 <= float(v) <= 1.0 for v in co):
            raise SemanticError("InvalidArgument", "Tracking coordinates must be two normalized finite values")
        if obj.tracks.get(name) is not None:
            raise SemanticError("Conflict", "Tracking track name already exists")
        try:
            track = obj.tracks.new(name=name, frame=frame)
            marker = track.markers.find_frame(frame, exact=True)
            if marker is None:
                raise SemanticError("BackendFailed", "Blender did not create the initial tracking marker")
            marker.co = [float(co[0]), float(co[1])]
            index = list(obj.tracks).index(track)
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected tracking track creation") from error
        self.changed()
        return {
            "ref": self._ref(root, clip_name, path + [["c", "tracks", index]]),
            "name": track.name,
            "frame": frame,
            "co": [float(marker.co[0]), float(marker.co[1])],
            "changed": True,
            "generation": self.generation,
        }

    def tracking_marker_add(self, track_ref, frame, co):
        root, clip_name, path, track = self._resolve(track_ref)
        if _text(getattr(getattr(track, "bl_rna", None), "identifier", ""), 256) != "MovieTrackingTrack":
            raise SemanticError("InvalidArgument", "Operation requires a MovieTrackingTrack ref")
        if isinstance(frame, bool) or not isinstance(frame, int) or not 0 <= frame <= 1_048_574:
            raise SemanticError("InvalidArgument", "Tracking marker frame is outside Blender bounds")
        if not isinstance(co, list) or len(co) != 2 or any(not _finite(v) or not -1.0 <= float(v) <= 1.0 for v in co):
            raise SemanticError("InvalidArgument", "Tracking marker coordinates must be normalized finite values")
        if track.markers.find_frame(frame, exact=True) is not None:
            raise SemanticError("Conflict", "Tracking marker already exists at this frame")
        try:
            marker = track.markers.insert_frame(frame, co=[float(co[0]), float(co[1])])
            index = list(track.markers).index(marker)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected tracking marker insertion") from error
        self.changed()
        return {
            "ref": self._ref(root, clip_name, path + [["c", "markers", index]]),
            "frame": int(marker.frame),
            "co": [float(marker.co[0]), float(marker.co[1])],
            "changed": True,
            "generation": self.generation,
        }

    def tracking_marker_remove(self, marker_ref):
        root, clip_name, parent_path, track, marker = self._collection_parent(marker_ref, "markers")
        if _text(getattr(getattr(track, "bl_rna", None), "identifier", ""), 256) != "MovieTrackingTrack":
            raise SemanticError("InvalidArgument", "Tracking marker parent is not a MovieTrackingTrack")
        frame = int(marker.frame)
        try:
            track.markers.delete_frame(frame)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected tracking marker deletion") from error
        self.changed()
        return {
            "track_ref": self._ref(root, clip_name, parent_path),
            "frame": frame,
            "changed": True,
            "generation": self.generation,
        }
    def _require_grease_pencil(self, reference):
        root, name, path, grease = self._resolve(reference)
        identifier = _text(getattr(getattr(grease, "bl_rna", None), "identifier", ""), 256)
        if root != "grease_pencils_v3" or path or identifier != "GreasePencilv3":
            raise SemanticError("InvalidArgument", "Operation requires a root modern GreasePencilv3 ref")
        return root, name, path, grease

    def grease_layer_add(self, grease_ref, name, set_active=True):
        root, anchor_name, path, grease = self._require_grease_pencil(grease_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name or not isinstance(set_active, bool):
            raise SemanticError("InvalidArgument", "Grease Pencil layer arguments are invalid")
        if any(layer.name == name for layer in grease.layers):
            raise SemanticError("Conflict", "Grease Pencil layer name already exists")
        try:
            layer = grease.layers.new(name, set_active=set_active)
            if layer.name != name:
                grease.layers.remove(layer)
                raise SemanticError("Conflict", "Blender rewrote the requested Grease Pencil layer name")
            index = list(grease.layers).index(layer)
        except SemanticError:
            raise
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Grease Pencil layer creation") from error
        self.changed()
        return {"ref": self._ref(root, anchor_name, path + [["c", "layers", index]]), "name": layer.name, "changed": True, "generation": self.generation}

    def grease_layer_remove(self, layer_ref):
        root, anchor_name, parent_path, grease, layer = self._collection_parent(layer_ref, "layers")
        identifier = _text(getattr(getattr(grease, "bl_rna", None), "identifier", ""), 256)
        if root != "grease_pencils_v3" or identifier != "GreasePencilv3":
            raise SemanticError("InvalidArgument", "Layer parent is not modern GreasePencilv3 data")
        try:
            grease.layers.remove(layer)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Grease Pencil layer removal") from error
        self.changed()
        return {"grease_ref": self._ref(root, anchor_name, parent_path), "changed": True, "generation": self.generation}

    def grease_frame_add(self, layer_ref, frame_number):
        root, anchor_name, path, layer = self._resolve(layer_ref)
        if _text(getattr(getattr(layer, "bl_rna", None), "identifier", ""), 256) != "GreasePencilLayer":
            raise SemanticError("InvalidArgument", "Operation requires a GreasePencilLayer ref")
        if isinstance(frame_number, bool) or not isinstance(frame_number, int) or not -1_048_574 <= frame_number <= 1_048_574:
            raise SemanticError("InvalidArgument", "Grease Pencil frame number is outside Blender bounds")
        if any(frame.frame_number == frame_number for frame in layer.frames):
            raise SemanticError("Conflict", "Grease Pencil frame already exists")
        try:
            frame = layer.frames.new(frame_number)
            index = list(layer.frames).index(frame)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Grease Pencil frame creation") from error
        self.changed()
        return {"ref": self._ref(root, anchor_name, path + [["c", "frames", index]]), "frame": frame_number, "changed": True, "generation": self.generation}

    def grease_frame_remove(self, frame_ref):
        root, anchor_name, parent_path, layer, frame = self._collection_parent(frame_ref, "frames")
        if _text(getattr(getattr(layer, "bl_rna", None), "identifier", ""), 256) != "GreasePencilLayer":
            raise SemanticError("InvalidArgument", "Frame parent is not a GreasePencilLayer")
        frame_number = int(frame.frame_number)
        try:
            layer.frames.remove(frame_number)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Grease Pencil frame removal") from error
        self.changed()
        return {"layer_ref": self._ref(root, anchor_name, parent_path), "frame": frame_number, "changed": True, "generation": self.generation}

    def _require_grease_drawing(self, drawing_ref):
        root, name, path, drawing = self._resolve(drawing_ref)
        identifier = _text(getattr(getattr(drawing, "bl_rna", None), "identifier", ""), 256)
        if root != "grease_pencils_v3" or identifier != "GreasePencilDrawing":
            raise SemanticError("InvalidArgument", "Operation requires a modern GreasePencilDrawing ref")
        return root, name, path, drawing

    def grease_strokes_add(self, drawing_ref, sizes):
        root, name, path, drawing = self._require_grease_drawing(drawing_ref)
        if not isinstance(sizes, list) or not sizes or len(sizes) > 10000 or any(isinstance(size, bool) or not isinstance(size, int) or not 1 <= size <= 1_000_000 for size in sizes) or sum(sizes) > 1_000_000:
            raise SemanticError("InvalidArgument", "Grease Pencil stroke sizes exceed bounded limits")
        before = len(drawing.strokes)
        try:
            drawing.add_strokes(sizes)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Grease Pencil stroke creation") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "strokes_added": len(drawing.strokes) - before, "points_added": sum(sizes), "stroke_count": len(drawing.strokes), "changed": True, "generation": self.generation}

    def grease_strokes_remove(self, drawing_ref, indices):
        root, name, path, drawing = self._require_grease_drawing(drawing_ref)
        if not isinstance(indices, list) or not indices or len(indices) > 10000 or any(isinstance(index, bool) or not isinstance(index, int) or index < 0 or index >= len(drawing.strokes) for index in indices):
            raise SemanticError("InvalidArgument", "Grease Pencil stroke indices are invalid")
        unique = sorted(set(indices))
        try:
            drawing.remove_strokes(indices=unique)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Grease Pencil stroke removal") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "strokes_removed": len(unique), "stroke_count": len(drawing.strokes), "changed": True, "generation": self.generation}

    def grease_strokes_resize(self, drawing_ref, sizes, indices):
        root, name, path, drawing = self._require_grease_drawing(drawing_ref)
        if not isinstance(sizes, list) or not isinstance(indices, list) or not sizes or len(sizes) != len(indices) or len(sizes) > 10000:
            raise SemanticError("InvalidArgument", "Grease Pencil resize arrays must be equal and bounded")
        if any(isinstance(size, bool) or not isinstance(size, int) or not 1 <= size <= 1_000_000 for size in sizes) or sum(sizes) > 1_000_000:
            raise SemanticError("InvalidArgument", "Grease Pencil stroke sizes are invalid")
        if any(isinstance(index, bool) or not isinstance(index, int) or index < 0 or index >= len(drawing.strokes) for index in indices):
            raise SemanticError("InvalidArgument", "Grease Pencil stroke indices are invalid")
        try:
            drawing.resize_strokes(sizes, indices=indices)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Grease Pencil stroke resize") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "strokes_resized": len(indices), "changed": True, "generation": self.generation}

    def _require_hair_curves(self, reference):
        root, name, path, curves = self._resolve(reference)
        if _text(getattr(getattr(curves, "bl_rna", None), "identifier", ""), 256) != "Curves":
            raise SemanticError("InvalidArgument", "Operation requires a Curves datablock ref")
        return root, name, path, curves

    def hair_curves_add(self, curves_ref, sizes):
        root, name, path, curves = self._require_hair_curves(curves_ref)
        if not isinstance(sizes, list) or not sizes or len(sizes) > 10000 or any(isinstance(size, bool) or not isinstance(size, int) or not 1 <= size <= 1_000_000 for size in sizes) or sum(sizes) > 1_000_000:
            raise SemanticError("InvalidArgument", "Curves sizes exceed bounded limits")
        before = len(curves.curves)
        try:
            curves.add_curves(sizes)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Curves creation") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "curves_added": len(curves.curves) - before, "points_added": sum(sizes), "curve_count": len(curves.curves), "changed": True, "generation": self.generation}

    def hair_curves_remove(self, curves_ref, indices):
        root, name, path, curves = self._require_hair_curves(curves_ref)
        if not isinstance(indices, list) or not indices or len(indices) > 10000 or any(isinstance(index, bool) or not isinstance(index, int) or index < 0 or index >= len(curves.curves) for index in indices):
            raise SemanticError("InvalidArgument", "Curves indices are invalid")
        unique = sorted(set(indices))
        try:
            curves.remove_curves(indices=unique)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Curves removal") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "curves_removed": len(unique), "curve_count": len(curves.curves), "changed": True, "generation": self.generation}

    def hair_curves_resize(self, curves_ref, sizes, indices):
        root, name, path, curves = self._require_hair_curves(curves_ref)
        if not isinstance(sizes, list) or not isinstance(indices, list) or not sizes or len(sizes) != len(indices) or len(sizes) > 10000:
            raise SemanticError("InvalidArgument", "Curves resize arrays must be equal and bounded")
        if any(isinstance(size, bool) or not isinstance(size, int) or not 1 <= size <= 1_000_000 for size in sizes) or sum(sizes) > 1_000_000:
            raise SemanticError("InvalidArgument", "Curves resize sizes are invalid")
        if any(isinstance(index, bool) or not isinstance(index, int) or index < 0 or index >= len(curves.curves) for index in indices):
            raise SemanticError("InvalidArgument", "Curves resize indices are invalid")
        try:
            curves.resize_curves(sizes, indices=indices)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Curves resize") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "curves_resized": len(indices), "changed": True, "generation": self.generation}

    def hair_curves_reorder(self, curves_ref, new_indices):
        root, name, path, curves = self._require_hair_curves(curves_ref)
        count = len(curves.curves)
        if not isinstance(new_indices, list) or len(new_indices) != count or sorted(new_indices) != list(range(count)):
            raise SemanticError("InvalidArgument", "Curves reorder must be a complete permutation")
        try:
            curves.reorder_curves(new_indices)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Curves reorder") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "curve_count": count, "changed": True, "generation": self.generation}

    def hair_curves_set_types(self, curves_ref, curve_type, indices):
        root, name, path, curves = self._require_hair_curves(curves_ref)
        if curve_type not in {"CATMULL_ROM", "POLY", "BEZIER", "NURBS"}:
            raise SemanticError("InvalidArgument", "Curves type is invalid")
        if not isinstance(indices, list) or not indices or len(indices) > 10000 or any(isinstance(index, bool) or not isinstance(index, int) or index < 0 or index >= len(curves.curves) for index in indices):
            raise SemanticError("InvalidArgument", "Curves type indices are invalid")
        try:
            curves.set_types(type=curve_type, indices=indices)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected Curves type mutation") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "type": curve_type, "curves_changed": len(indices), "changed": True, "generation": self.generation}

    def _require_metaball(self, reference):
        root, name, path, metaball = self._resolve(reference)
        if _text(getattr(getattr(metaball, "bl_rna", None), "identifier", ""), 256) != "MetaBall":
            raise SemanticError("InvalidArgument", "Operation requires a MetaBall datablock ref")
        return root, name, path, metaball

    def metaball_element_add(self, metaball_ref, element_type="BALL"):
        root, name, path, metaball = self._require_metaball(metaball_ref)
        if element_type not in {"BALL", "CAPSULE", "PLANE", "ELLIPSOID", "CUBE"}:
            raise SemanticError("InvalidArgument", "Metaball element type is invalid")
        try:
            element = metaball.elements.new(type=element_type)
            index = list(metaball.elements).index(element)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected MetaBall element creation") from error
        self.changed()
        return {"ref": self._ref(root, name, path + [["c", "elements", index]]), "type": element.type, "changed": True, "generation": self.generation}

    def metaball_element_remove(self, element_ref):
        root, name, parent_path, metaball, element = self._collection_parent(element_ref, "elements")
        if _text(getattr(getattr(metaball, "bl_rna", None), "identifier", ""), 256) != "MetaBall":
            raise SemanticError("InvalidArgument", "MetaBall element parent is invalid")
        try:
            metaball.elements.remove(element)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected MetaBall element removal") from error
        self.changed()
        return {"metaball_ref": self._ref(root, name, parent_path), "changed": True, "generation": self.generation}

    def _require_mesh(self, reference):
        root, name, path, item = self._resolve(reference)
        if _text(getattr(getattr(item, "bl_rna", None), "identifier", ""), 256) != "Mesh":
            raise SemanticError("InvalidArgument", "Operation requires a Mesh RNA reference")
        return root, name, path, item

    def mesh_summary(self, mesh_ref):
        root, name, path, mesh = self._require_mesh(mesh_ref)
        return {
            "ref": self._ref(root, name, path),
            "vertices": len(mesh.vertices),
            "edges": len(mesh.edges),
            "faces": len(mesh.polygons),
            "shape_keys": len(mesh.shape_keys.key_blocks) if mesh.shape_keys else 0,
        }

    def mesh_geometry_replace(self, mesh_ref, vertices, edges, faces):
        root, name, path, mesh = self._require_mesh(mesh_ref)
        if not isinstance(vertices, list) or len(vertices) > 10000:
            raise SemanticError("InvalidArgument", "Mesh vertex list exceeds bounded size")
        if not isinstance(edges, list) or len(edges) > 30000:
            raise SemanticError("InvalidArgument", "Mesh edge list exceeds bounded size")
        if not isinstance(faces, list) or len(faces) > 10000:
            raise SemanticError("InvalidArgument", "Mesh face list exceeds bounded size")
        clean_vertices = []
        for vertex in vertices:
            if not isinstance(vertex, list) or len(vertex) != 3 or any(not _finite(v) or abs(float(v)) > 1_000_000 for v in vertex):
                raise SemanticError("InvalidArgument", "Mesh vertex must be three bounded finite coordinates")
            clean_vertices.append([float(v) for v in vertex])
        count = len(clean_vertices)
        clean_edges = []
        for edge in edges:
            if not isinstance(edge, list) or len(edge) != 2 or any(isinstance(i, bool) or not isinstance(i, int) or i < 0 or i >= count for i in edge):
                raise SemanticError("InvalidArgument", "Mesh edge references an invalid vertex index")
            clean_edges.append(edge)
        clean_faces = []
        for face in faces:
            if not isinstance(face, list) or not 3 <= len(face) <= 64 or any(isinstance(i, bool) or not isinstance(i, int) or i < 0 or i >= count for i in face):
                raise SemanticError("InvalidArgument", "Mesh face references invalid or excessive vertices")
            if len(set(face)) < 3:
                raise SemanticError("InvalidArgument", "Mesh face must contain at least three distinct vertices")
            clean_faces.append(face)
        try:
            mesh.clear_geometry()
            mesh.from_pydata(clean_vertices, clean_edges, clean_faces)
            mesh.update()
        except Exception as error:
            raise SemanticError("InvalidArgument", "Blender rejected bounded mesh topology") from error
        self.changed()
        return {
            "ref": self._ref(root, name, path),
            "vertices": len(mesh.vertices),
            "edges": len(mesh.edges),
            "faces": len(mesh.polygons),
            "changed": True,
            "generation": self.generation,
        }

    def mesh_attribute_add(self, mesh_ref, name, data_type, domain):
        root, mesh_name, path, mesh = self._require_mesh(mesh_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Mesh attribute name is invalid")
        if mesh.attributes.get(name) is not None:
            raise SemanticError("Conflict", "Mesh attribute name already exists")
        if not isinstance(data_type, str) or not re.fullmatch(r"[A-Z][A-Z0-9_]{0,63}", data_type):
            raise SemanticError("InvalidArgument", "Mesh attribute data type is invalid")
        if domain not in {"POINT", "EDGE", "FACE", "CORNER"}:
            raise SemanticError("InvalidArgument", "Mesh attribute domain is invalid")
        try:
            attribute = mesh.attributes.new(name=name, type=data_type, domain=domain)
            index = list(mesh.attributes).index(attribute)
        except Exception as error:
            raise SemanticError("Unsupported", "Blender rejected this mesh attribute type/domain") from error
        self.changed()
        child_path = path + [["c", "attributes", index]]
        return {
            "ref": self._ref(root, mesh_name, child_path),
            "name": attribute.name,
            "data_type": attribute.data_type,
            "domain": attribute.domain,
            "changed": True,
            "generation": self.generation,
        }

    def mesh_attribute_remove(self, attribute_ref):
        root, name, parent_path, mesh, attribute = self._collection_parent(attribute_ref, "attributes")
        if _text(getattr(getattr(mesh, "bl_rna", None), "identifier", ""), 256) != "Mesh":
            raise SemanticError("InvalidArgument", "Attribute parent is not a Mesh")
        try:
            mesh.attributes.remove(attribute)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected mesh attribute removal") from error
        self.changed()
        return {"mesh_ref": self._ref(root, name, parent_path), "changed": True, "generation": self.generation}

    def mesh_uv_layer_add(self, mesh_ref, name, do_init=True):
        root, mesh_name, path, mesh = self._require_mesh(mesh_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "UV layer name is invalid")
        if mesh.uv_layers.get(name) is not None:
            raise SemanticError("Conflict", "UV layer name already exists")
        if not isinstance(do_init, bool):
            raise SemanticError("InvalidArgument", "do_init must be boolean")
        try:
            layer = mesh.uv_layers.new(name=name, do_init=do_init)
            index = list(mesh.uv_layers).index(layer)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected UV layer creation") from error
        self.changed()
        child_path = path + [["c", "uv_layers", index]]
        return {"ref": self._ref(root, mesh_name, child_path), "name": layer.name, "changed": True, "generation": self.generation}

    def mesh_uv_layer_remove(self, layer_ref):
        root, name, parent_path, mesh, layer = self._collection_parent(layer_ref, "uv_layers")
        if _text(getattr(getattr(mesh, "bl_rna", None), "identifier", ""), 256) != "Mesh":
            raise SemanticError("InvalidArgument", "UV layer parent is not a Mesh")
        try:
            mesh.uv_layers.remove(layer)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected UV layer removal") from error
        self.changed()
        return {"mesh_ref": self._ref(root, name, parent_path), "changed": True, "generation": self.generation}

    def _require_object_vertex_groups(self, object_ref):
        root, name, path, obj = self._require_object(object_ref)
        if getattr(obj, "type", None) != "MESH":
            raise SemanticError("Unsupported", "Vertex groups currently require a mesh Object")
        return root, name, path, obj

    def vertex_group_add(self, object_ref, name):
        root, object_name, path, obj = self._require_object_vertex_groups(object_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Vertex-group name is invalid")
        if obj.vertex_groups.get(name) is not None:
            raise SemanticError("Conflict", "Vertex-group name already exists")
        try:
            group = obj.vertex_groups.new(name=name)
            index = list(obj.vertex_groups).index(group)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected vertex-group creation") from error
        self.changed()
        child_path = path + [["c", "vertex_groups", index]]
        return {"ref": self._ref(root, object_name, child_path), "name": group.name, "changed": True, "generation": self.generation}

    def vertex_group_remove(self, group_ref):
        root, name, parent_path, obj, group = self._collection_parent(group_ref, "vertex_groups")
        if _text(getattr(getattr(obj, "bl_rna", None), "identifier", ""), 256) != "Object":
            raise SemanticError("InvalidArgument", "Vertex-group parent is not an Object")
        try:
            obj.vertex_groups.remove(group)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected vertex-group removal") from error
        self.changed()
        return {"object_ref": self._ref(root, name, parent_path), "changed": True, "generation": self.generation}

    def vertex_group_weights_set(self, group_ref, indices, weight, mode="REPLACE"):
        root, name, parent_path, obj, group = self._collection_parent(group_ref, "vertex_groups")
        if _text(getattr(getattr(obj, "bl_rna", None), "identifier", ""), 256) != "Object" or getattr(obj, "type", None) != "MESH":
            raise SemanticError("InvalidArgument", "Vertex-group parent must be a mesh Object")
        if not isinstance(indices, list) or len(indices) > 10000 or any(isinstance(index, bool) or not isinstance(index, int) or index < 0 or index >= len(obj.data.vertices) for index in indices):
            raise SemanticError("InvalidArgument", "Vertex-group indices are invalid or exceed bounded size")
        if not _finite(weight) or not 0.0 <= float(weight) <= 1.0:
            raise SemanticError("InvalidArgument", "Vertex-group weight must be in [0,1]")
        if mode not in {"REPLACE", "ADD", "SUBTRACT"}:
            raise SemanticError("InvalidArgument", "Vertex-group mode is invalid")
        try:
            group.add(indices, float(weight), mode)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected vertex-group weight update") from error
        self.changed()
        return {"ref": self._ref(root, name, parent_path + [["c", "vertex_groups", list(obj.vertex_groups).index(group)]]), "vertices": len(indices), "weight": float(weight), "mode": mode, "changed": True, "generation": self.generation}

    def vertex_group_weights_remove(self, group_ref, indices):
        root, name, parent_path, obj, group = self._collection_parent(group_ref, "vertex_groups")
        if _text(getattr(getattr(obj, "bl_rna", None), "identifier", ""), 256) != "Object" or getattr(obj, "type", None) != "MESH":
            raise SemanticError("InvalidArgument", "Vertex-group parent must be a mesh Object")
        if not isinstance(indices, list) or len(indices) > 10000 or any(isinstance(index, bool) or not isinstance(index, int) or index < 0 or index >= len(obj.data.vertices) for index in indices):
            raise SemanticError("InvalidArgument", "Vertex-group indices are invalid or exceed bounded size")
        try:
            group.remove(indices)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected vertex-group weight removal") from error
        self.changed()
        return {"ref": self._ref(root, name, parent_path + [["c", "vertex_groups", list(obj.vertex_groups).index(group)]]), "vertices": len(indices), "changed": True, "generation": self.generation}

    def shape_key_add(self, object_ref, name, from_mix=False):
        root, object_name, path, obj = self._require_object(object_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Shape-key name is invalid")
        if not isinstance(from_mix, bool):
            raise SemanticError("InvalidArgument", "from_mix must be boolean")
        keys = getattr(getattr(obj, "data", None), "shape_keys", None)
        if keys is not None and keys.key_blocks.get(name) is not None:
            raise SemanticError("Conflict", "Shape-key name already exists")
        try:
            key = obj.shape_key_add(name=name, from_mix=from_mix)
            keys = obj.data.shape_keys
            index = list(keys.key_blocks).index(key)
        except Exception as error:
            raise SemanticError("Unsupported", "Object data does not support shape keys") from error
        self.changed()
        child_path = path + [["p", "data"], ["p", "shape_keys"], ["c", "key_blocks", index]]
        return {"ref": self._ref(root, object_name, child_path), "name": key.name, "changed": True, "generation": self.generation}

    def shape_key_remove(self, key_ref):
        root, object_name, path = self._parse_ref(key_ref)
        if len(path) < 3 or path[-1][0:2] != ["c", "key_blocks"]:
            raise SemanticError("InvalidArgument", "Reference is not a shape key")
        obj = self._resolve(self._ref(root, object_name, path[:-3]))[3]
        key = self._resolve(key_ref)[3]
        if _text(getattr(getattr(obj, "bl_rna", None), "identifier", ""), 256) != "Object":
            raise SemanticError("InvalidArgument", "Shape-key owner is not an Object")
        try:
            obj.shape_key_remove(key)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected shape-key removal") from error
        self.changed()
        return {"object_ref": self._ref(root, object_name, path[:-3]), "changed": True, "generation": self.generation}

    def _require_curve(self, reference):
        root, name, path, item = self._resolve(reference)
        identifier = _text(getattr(getattr(item, "bl_rna", None), "identifier", ""), 256)
        if identifier not in {"Curve", "SurfaceCurve"}:
            raise SemanticError("InvalidArgument", "Operation requires a Curve RNA reference")
        return root, name, path, item

    def spline_add(self, curve_ref, spline_type, points=1):
        root, name, path, curve = self._require_curve(curve_ref)
        if spline_type not in {"POLY", "BEZIER", "NURBS"}:
            raise SemanticError("InvalidArgument", "Spline type is not supported")
        if isinstance(points, bool) or not isinstance(points, int) or not 1 <= points <= 10000:
            raise SemanticError("InvalidArgument", "Spline point count is outside bounded limits")
        try:
            spline = curve.splines.new(type=spline_type)
            if points > 1:
                target = spline.bezier_points if spline_type == "BEZIER" else spline.points
                target.add(points - 1)
            index = list(curve.splines).index(spline)
        except Exception as error:
            raise SemanticError("Unsupported", "Blender rejected spline creation") from error
        self.changed()
        child_path = path + [["c", "splines", index]]
        return {"ref": self._ref(root, name, child_path), "type": spline.type, "points": points, "changed": True, "generation": self.generation}

    def spline_remove(self, spline_ref):
        root, name, parent_path, curve, spline = self._collection_parent(spline_ref, "splines")
        identifier = _text(getattr(getattr(curve, "bl_rna", None), "identifier", ""), 256)
        if identifier not in {"Curve", "SurfaceCurve"}:
            raise SemanticError("InvalidArgument", "Spline parent is not a Curve")
        try:
            curve.splines.remove(spline)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected spline removal") from error
        self.changed()
        return {"curve_ref": self._ref(root, name, parent_path), "changed": True, "generation": self.generation}

    def modifier_add(self, object_ref, name, modifier_type):
        root, object_name, path, obj = self._require_object(object_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Modifier name is invalid")
        if obj.modifiers.get(name) is not None:
            raise SemanticError("Conflict", "Modifier name already exists; implicit suffixing is forbidden")
        if not isinstance(modifier_type, str) or not re.fullmatch(r"[A-Z][A-Z0-9_]{0,63}", modifier_type):
            raise SemanticError("InvalidArgument", "Modifier type is invalid")
        try:
            modifier = obj.modifiers.new(name=name, type=modifier_type)
            index = list(obj.modifiers).index(modifier)
        except Exception as error:
            raise SemanticError("Unsupported", "Modifier type is unavailable in this Blender build") from error
        self.changed()
        child_path = path + [["c", "modifiers", index]]
        return {"ref": self._ref(root, object_name, child_path), "name": modifier.name, "type": modifier.type, "changed": True, "generation": self.generation}

    def modifier_remove(self, modifier_ref):
        root, name, parent_path, obj, modifier = self._collection_parent(modifier_ref, "modifiers")
        if _text(getattr(getattr(obj, "bl_rna", None), "identifier", ""), 256) != "Object":
            raise SemanticError("InvalidArgument", "Modifier parent is not an Object")
        try:
            obj.modifiers.remove(modifier)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected modifier removal") from error
        self.changed()
        return {"object_ref": self._ref(root, name, parent_path), "changed": True, "generation": self.generation}

    def constraint_add(self, object_ref, name, constraint_type):
        root, object_name, path, obj = self._require_object(object_ref)
        if not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name:
            raise SemanticError("InvalidArgument", "Constraint name is invalid")
        if obj.constraints.get(name) is not None:
            raise SemanticError("Conflict", "Constraint name already exists; implicit suffixing is forbidden")
        if not isinstance(constraint_type, str) or not re.fullmatch(r"[A-Z][A-Z0-9_]{0,63}", constraint_type):
            raise SemanticError("InvalidArgument", "Constraint type is invalid")
        try:
            constraint = obj.constraints.new(type=constraint_type)
            constraint.name = name
            index = list(obj.constraints).index(constraint)
        except Exception as error:
            raise SemanticError("Unsupported", "Constraint type is unavailable in this Blender build") from error
        self.changed()
        child_path = path + [["c", "constraints", index]]
        return {"ref": self._ref(root, object_name, child_path), "name": constraint.name, "type": constraint.type, "changed": True, "generation": self.generation}

    def constraint_remove(self, constraint_ref):
        root, name, parent_path, obj, constraint = self._collection_parent(constraint_ref, "constraints")
        if _text(getattr(getattr(obj, "bl_rna", None), "identifier", ""), 256) != "Object":
            raise SemanticError("InvalidArgument", "Constraint parent is not an Object")
        try:
            obj.constraints.remove(constraint)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected constraint removal") from error
        self.changed()
        return {"object_ref": self._ref(root, name, parent_path), "changed": True, "generation": self.generation}

    def keyframe_insert(self, reference, property_id, frame, index=-1):
        root, name, path, item = self._resolve(reference)
        rna = getattr(item, "bl_rna", None)
        prop = rna.properties.get(property_id) if rna is not None else None
        if prop is None:
            raise SemanticError("NotFound", "RNA property does not exist")
        status, reason = _property_status(prop)
        if status != "managed" or not bool(getattr(prop, "is_animatable", False)):
            raise SemanticError("Unsupported", reason or "RNA property is not safely animatable")
        if not _finite(frame) or not -1_000_000 <= float(frame) <= 1_000_000:
            raise SemanticError("InvalidArgument", "Keyframe is outside the bounded timeline")
        if isinstance(index, bool) or not isinstance(index, int) or not -1 <= index < MAX_ARRAY:
            raise SemanticError("InvalidArgument", "Keyframe array index is invalid")
        try:
            changed = bool(item.keyframe_insert(data_path=property_id, index=index, frame=float(frame)))
        except Exception as error:
            raise SemanticError("Unsupported", "Blender rejected keyframe insertion for this RNA property") from error
        if not changed:
            raise SemanticError("BackendFailed", "Blender did not insert the requested keyframe")
        self.changed()
        return {"ref": self._ref(root, name, path), "property": property_id, "frame": float(frame), "index": index, "changed": True, "generation": self.generation}

    def keyframe_delete(self, reference, property_id, frame, index=-1):
        root, name, path, item = self._resolve(reference)
        rna = getattr(item, "bl_rna", None)
        prop = rna.properties.get(property_id) if rna is not None else None
        if prop is None:
            raise SemanticError("NotFound", "RNA property does not exist")
        status, reason = _property_status(prop)
        if status != "managed" or not bool(getattr(prop, "is_animatable", False)):
            raise SemanticError("Unsupported", reason or "RNA property is not safely animatable")
        if not _finite(frame) or not -1_000_000 <= float(frame) <= 1_000_000:
            raise SemanticError("InvalidArgument", "Keyframe is outside the bounded timeline")
        if isinstance(index, bool) or not isinstance(index, int) or not -1 <= index < MAX_ARRAY:
            raise SemanticError("InvalidArgument", "Keyframe array index is invalid")
        try:
            changed = bool(item.keyframe_delete(data_path=property_id, index=index, frame=float(frame)))
        except Exception as error:
            raise SemanticError("Unsupported", "Blender rejected keyframe deletion for this RNA property") from error
        if not changed:
            raise SemanticError("NotFound", "Requested keyframe does not exist")
        self.changed()
        return {"ref": self._ref(root, name, path), "property": property_id, "frame": float(frame), "index": index, "changed": True, "generation": self.generation}

    def node_types(self, query="", limit=50):
        query = str(query or "").casefold()
        base = getattr(self.bpy.types, "Node", None)
        if base is None:
            return {"items": [], "truncated": False}

        candidates = []
        pending = list(base.__subclasses__())
        seen_classes = set()
        while pending and len(seen_classes) < 10000:
            candidate = pending.pop()
            if candidate in seen_classes:
                continue
            seen_classes.add(candidate)
            candidates.append(candidate)
            try:
                pending.extend(candidate.__subclasses__())
            except Exception:
                pass

        # Blender's generated RNA classes are not guaranteed to all appear in
        # Python __subclasses__() in background mode. Supplement with bpy.types
        # and still require a real Node subclass.
        for attr in dir(self.bpy.types):
            if attr.startswith("_"):
                continue
            candidate = getattr(self.bpy.types, attr, None)
            if candidate in seen_classes:
                continue
            try:
                if not isinstance(candidate, type) or not issubclass(candidate, base):
                    continue
            except TypeError:
                continue
            seen_classes.add(candidate)
            candidates.append(candidate)

        by_identifier = {}
        for candidate in candidates:
            rna = getattr(candidate, "bl_rna", None)
            identifier = _text(
                getattr(candidate, "bl_idname", "")
                or getattr(rna, "identifier", ""),
                256,
            )
            if not identifier:
                continue
            label = _text(
                getattr(candidate, "bl_label", "")
                or getattr(rna, "name", "")
                or identifier,
                512,
            )
            if query and query not in f"{identifier} {label}".casefold():
                continue
            safe = "script" not in identifier.casefold()
            by_identifier[identifier] = {
                "id": identifier,
                "name": label,
                "status": "managed" if safe else "unsupported_by_design",
                "reason": None if safe else "script nodes cross the executable-code boundary",
            }
        items = [by_identifier[key] for key in sorted(by_identifier)]
        return {"items": items[:limit], "truncated": len(items) > limit}

    def _require_node_tree(self, reference):
        root, name, path, item = self._resolve(reference)
        node_tree = getattr(self.bpy.types, "NodeTree", None)
        if node_tree is None or not isinstance(item, node_tree):
            raise SemanticError("InvalidArgument", "Operation requires a NodeTree RNA reference")
        return root, name, path, item

    def node_add(self, tree_ref, node_type, name=None):
        root, root_name, path, tree = self._require_node_tree(tree_ref)
        if not isinstance(node_type, str) or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]{0,127}", node_type) or "script" in node_type.casefold():
            raise SemanticError("Unsupported", "Node type is invalid or crosses the executable-code boundary")
        if name is not None and (not isinstance(name, str) or not name.strip() or len(name) > 128 or "\x00" in name):
            raise SemanticError("InvalidArgument", "Node name is invalid")
        if name is not None and tree.nodes.get(name) is not None:
            raise SemanticError("Conflict", "Node name already exists; implicit suffixing is forbidden")
        try:
            node = tree.nodes.new(type=node_type)
            if name is not None:
                node.name = name
            index = list(tree.nodes).index(node)
        except Exception as error:
            raise SemanticError("Unsupported", "Node type is unavailable for this node tree") from error
        self.changed()
        child_path = path + [["c", "nodes", index]]
        return {"ref": self._ref(root, root_name, child_path), "name": node.name, "node_type": _text(getattr(node, "bl_idname", ""), 256), "changed": True, "generation": self.generation}

    def node_remove(self, node_ref):
        root, name, parent_path, tree, node = self._collection_parent(node_ref, "nodes")
        node_tree = getattr(self.bpy.types, "NodeTree", None)
        if node_tree is None or not isinstance(tree, node_tree):
            raise SemanticError("InvalidArgument", "Node parent is not a NodeTree")
        try:
            tree.nodes.remove(node)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected node removal") from error
        self.changed()
        return {"tree_ref": self._ref(root, name, parent_path), "changed": True, "generation": self.generation}

    def node_link(self, tree_ref, from_socket_ref, to_socket_ref):
        root, name, path, tree = self._require_node_tree(tree_ref)
        from_parts = self._resolve(from_socket_ref)
        to_parts = self._resolve(to_socket_ref)
        socket_type = getattr(self.bpy.types, "NodeSocket", None)
        if socket_type is None or not isinstance(from_parts[3], socket_type) or not isinstance(to_parts[3], socket_type):
            raise SemanticError("InvalidArgument", "Node link endpoints must be NodeSocket refs")
        if from_parts[:2] != (root, name) or to_parts[:2] != (root, name):
            raise SemanticError("InvalidArgument", "Node link endpoints must belong to the same root datablock")
        try:
            link = tree.links.new(from_parts[3], to_parts[3])
            index = list(tree.links).index(link)
        except Exception as error:
            raise SemanticError("InvalidArgument", "Blender rejected the node link endpoints") from error
        self.changed()
        child_path = path + [["c", "links", index]]
        return {"ref": self._ref(root, name, child_path), "changed": True, "generation": self.generation}

    def node_unlink(self, link_ref):
        root, name, parent_path, tree, link = self._collection_parent(link_ref, "links")
        node_tree = getattr(self.bpy.types, "NodeTree", None)
        if node_tree is None or not isinstance(tree, node_tree):
            raise SemanticError("InvalidArgument", "Link parent is not a NodeTree")
        try:
            tree.links.remove(link)
        except Exception as error:
            raise SemanticError("BackendFailed", "Blender rejected node-link removal") from error
        self.changed()
        return {"tree_ref": self._ref(root, name, parent_path), "changed": True, "generation": self.generation}

    def property_get(self, reference, property_id):
        root, name, path, item = self._resolve(reference)
        rna = getattr(item, "bl_rna", None)
        prop = rna.properties.get(property_id) if rna is not None else None
        if prop is None or property_id == "rna_type":
            raise SemanticError("NotFound", "RNA property does not exist")
        status, reason = _property_status(prop)
        if status in {"relation", "unsupported_by_design", "runtime_owned"}:
            raise SemanticError("Unsupported", reason or "RNA property is not readable through this surface")
        try:
            value = _serialize(getattr(item, property_id), prop)
        except Exception as error:
            raise SemanticError("Unsupported", "RNA value cannot be represented safely") from error
        owner_identifier = _text(getattr(rna, "identifier", ""), 256) if rna is not None else ""
        return {"ref": self._ref(root, name, path), "property": property_id, "value": value, "descriptor": _property_descriptor(prop, owner_identifier)}

    def property_set(self, reference, property_id, value):
        root, name, path, item = self._resolve(reference)
        rna = getattr(item, "bl_rna", None)
        prop = rna.properties.get(property_id) if rna is not None else None
        if prop is None or property_id == "rna_type":
            raise SemanticError("NotFound", "RNA property does not exist")
        try:
            coerced = _coerce(value, prop)
            setattr(item, property_id, coerced)
        except SemanticError:
            raise
        except Exception as error:
            status, reason = _property_status(prop)
            if status != "managed":
                raise SemanticError("Unsupported", reason or "RNA property is not generically mutable") from error
            raise SemanticError("InvalidArgument", "RNA rejected the typed property value") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "property": property_id, "value": _serialize(getattr(item, property_id), prop), "changed": True, "generation": self.generation}

    def property_reset(self, reference, property_id):
        root, name, path, item = self._resolve(reference)
        rna = getattr(item, "bl_rna", None)
        prop = rna.properties.get(property_id) if rna is not None else None
        if prop is None or property_id == "rna_type":
            raise SemanticError("NotFound", "RNA property does not exist")
        status, reason = _property_status(prop)
        if status != "managed":
            raise SemanticError("Unsupported", reason or "RNA property is not generically mutable")
        try:
            setattr(item, property_id, _default_value(prop))
        except Exception as error:
            raise SemanticError("Unsupported", "RNA property does not expose a bounded reset default") from error
        self.changed()
        return {"ref": self._ref(root, name, path), "property": property_id, "value": _serialize(getattr(item, property_id), prop), "changed": True, "generation": self.generation}
