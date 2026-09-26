"""Bounded RNA semantic substrate for the sandbox-owned Blender runtime.

This module deliberately exposes data, not Python execution. Mutable generic properties are
restricted to scalar/enum/numeric-array RNA values that do not carry paths, callbacks, pointers,
collections or executable text. Richer domains are layered on top with dedicated semantics.
"""
from __future__ import annotations

import base64
import json
import math
import re


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
    "hair_curves": "Curves",
    "images": "Image",
    "lattices": "Lattice",
    "lights": "Light",
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
    "text",
}


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
    if bool(getattr(prop, "is_readonly", False)):
        return "read_only", "RNA marks this property read-only"
    if ptype in {"POINTER", "COLLECTION"}:
        return "relation", "Pointer/collection identity requires a typed domain relation"
    if subtype in PATH_SUBTYPES:
        return "unsupported_by_design", "Filesystem paths require scoped asset/file semantics"
    if identifier.casefold() in EXECUTABLE_IDENTIFIERS:
        return "unsupported_by_design", "Executable or ambient text is not generic semantic data"
    if ptype == "STRING":
        return "read_only", "Generic strings stay read-only; domain overlays own bounded text mutation"
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


def _property_descriptor(prop):
    status, reason = _property_status(prop)
    out = {
        "id": _text(getattr(prop, "identifier", ""), 256),
        "name": _text(getattr(prop, "name", ""), 512),
        "description": _text(getattr(prop, "description", "")),
        "type": _text(getattr(prop, "type", ""), 64),
        "subtype": _text(getattr(prop, "subtype", ""), 64),
        "array_length": min(int(getattr(prop, "array_length", 0) or 0), 4096),
        "animatable": bool(getattr(prop, "is_animatable", False)),
        "status": status,
    }
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
    raise ValueError("unsupported RNA value kind")


class SemanticError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


class SemanticStore:
    def __init__(self, bpy):
        self.bpy = bpy
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
            "generic_mutation": ["BOOLEAN", "INT", "FLOAT", "ENUM", "bounded_numeric_arrays"],
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
        for prop in list(rna.properties)[:1024]:
            properties.append(_property_descriptor(prop))
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
            for prop in list(rna.properties)[:1024]:
                properties.append(_property_descriptor(prop))
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
        if ptype not in {"POINTER", "COLLECTION"}:
            raise SemanticError("InvalidArgument", "RNA property is not a relation")
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
        return {
            "items": items,
            "truncated": total > offset + len(items),
            "generation": self.generation,
            "offset": offset,
            "relation": property_id,
        }

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
        return {"ref": self._ref(root, name, path), "property": property_id, "value": value, "descriptor": _property_descriptor(prop)}

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
