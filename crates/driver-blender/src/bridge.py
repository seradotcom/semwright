"""Fixed Blender-side bridge for the sandboxed Semwright DriverProvider.

This file is compiled into the Rust driver. It accepts only the driver's curated command namespace
and bounded introspection. There is no eval/exec, arbitrary Python, generic operator invocation, TCP
listener, or user-site import.
"""
import json
import math
import os
import socket
import struct
import sys

import addon_utils
import bpy

MAX_FRAME = 1_048_576
MAX_TEXT = 2048

try:
    marker = sys.argv.index("--")
    socket_path, workspace, runtime = sys.argv[marker + 1:marker + 4]
except (ValueError, IndexError):
    raise SystemExit(2)

sys.path.insert(0, runtime)
from semwright_blender_runtime.commands import Commands  # noqa: E402
from semwright_blender_runtime.validation import CommandError  # noqa: E402
from semwright_blender_runtime.semantic import SemanticError, SemanticStore  # noqa: E402

commands = Commands(bpy, workspace)
semantic = SemanticStore(bpy, commands.workspace.root)


def exact(stream, size):
    pieces = []
    while size:
        block = stream.recv(size)
        if not block:
            raise EOFError("truncated frame")
        pieces.append(block)
        size -= len(block)
    return b"".join(pieces)


def read_frame(stream):
    length = struct.unpack(">I", exact(stream, 4))[0]
    if not 0 < length <= MAX_FRAME:
        raise ValueError("invalid frame size")
    return json.loads(
        exact(stream, length),
        object_pairs_hook=_unique_object,
        parse_constant=lambda _: (_ for _ in ()).throw(ValueError("non-finite number")),
    )


def write_frame(stream, value):
    body = json.dumps(
        value, separators=(",", ":"), ensure_ascii=False, allow_nan=False
    ).encode()
    if not 0 < len(body) <= MAX_FRAME:
        raise ValueError("output exceeds frame limit")
    stream.sendall(struct.pack(">I", len(body)) + body)


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result


def text(value, maximum=MAX_TEXT):
    value = str(value or "").replace("\x00", "")
    return value[:maximum]


def limit_arg(args):
    value = args.get("limit", 50)
    if isinstance(value, bool) or not isinstance(value, int) or not 1 <= value <= 256:
        raise CommandError("InvalidArgument", "Introspection limit is invalid")
    return value


def query_arg(args):
    value = args.get("query", "")
    if not isinstance(value, str) or len(value) > 256 or "\x00" in value:
        raise CommandError("InvalidArgument", "Introspection query is invalid")
    return value.casefold()


def operator_parts(identifier):
    if (
        not isinstance(identifier, str)
        or len(identifier) > 256
        or identifier.count(".") != 1
    ):
        raise CommandError("InvalidArgument", "Operator identifier is invalid")
    category, name = identifier.split(".")
    allowed = "abcdefghijklmnopqrstuvwxyz0123456789_"
    if not category or not name or any(ch not in allowed for ch in category + name):
        raise CommandError("InvalidArgument", "Operator identifier is invalid")
    return category, name


def operator_object(identifier):
    category_name, operator_name = operator_parts(identifier)
    category = getattr(bpy.ops, category_name, None)
    operator = getattr(category, operator_name, None) if category is not None else None
    if operator is None or not hasattr(operator, "get_rna_type"):
        raise CommandError("NotFound", "Blender operator does not exist")
    try:
        rna = operator.get_rna_type()
    except Exception as error:
        raise CommandError("NotFound", "Blender operator RNA is unavailable") from error
    return operator, rna


def operator_available(operator):
    try:
        return bool(operator.poll())
    except Exception:
        return False


def property_info(prop):
    result = {
        "id": text(getattr(prop, "identifier", ""), 256),
        "name": text(getattr(prop, "name", ""), 512),
        "description": text(getattr(prop, "description", "")),
        "type": text(getattr(prop, "type", ""), 64),
        "subtype": text(getattr(prop, "subtype", ""), 64),
        "readonly": bool(getattr(prop, "is_readonly", False)),
        "required": bool(getattr(prop, "is_required", False)),
    }
    for source, target in [
        ("hard_min", "minimum"),
        ("hard_max", "maximum"),
        ("array_length", "array_length"),
    ]:
        value = getattr(prop, source, None)
        if isinstance(value, (int, float)) and not isinstance(value, bool):
            if isinstance(value, int) or math.isfinite(value):
                result[target] = value
    if result["type"] == "ENUM":
        try:
            values = []
            for item in list(prop.enum_items)[:64]:
                values.append(
                    {
                        "id": text(item.identifier, 256),
                        "name": text(item.name, 512),
                    }
                )
            result["enum"] = values
        except Exception:
            result["enum"] = []
    return result


def describe_operator(identifier):
    operator, rna = operator_object(identifier)
    properties = []
    for prop in list(rna.properties):
        if getattr(prop, "identifier", "") == "rna_type":
            continue
        properties.append(property_info(prop))
        if len(properties) == 128:
            break
    return {
        "id": identifier,
        "name": text(getattr(rna, "name", ""), 512),
        "description": text(getattr(rna, "description", "")),
        "available": operator_available(operator),
        "properties": properties,
    }

def iter_operators():
    for category_name in sorted(name for name in dir(bpy.ops) if not name.startswith("_")):
        category = getattr(bpy.ops, category_name, None)
        if category is None:
            continue
        for operator_name in sorted(name for name in dir(category) if not name.startswith("_")):
            operator = getattr(category, operator_name, None)
            if operator is None or not hasattr(operator, "get_rna_type"):
                continue
            try:
                rna = operator.get_rna_type()
            except Exception:
                continue
            yield f"{category_name}.{operator_name}", operator, rna


def search_operators(args):
    query = query_arg(args)
    limit = limit_arg(args)
    items = []
    matched = 0
    for identifier, operator, rna in iter_operators():
        haystack = " ".join(
            [identifier, str(getattr(rna, "name", "")), str(getattr(rna, "description", ""))]
        ).casefold()
        if query and query not in haystack:
            continue
        matched += 1
        if len(items) < limit:
            count = max(0, len(list(rna.properties)) - 1)
            items.append(
                {
                    "id": identifier,
                    "name": text(getattr(rna, "name", ""), 512),
                    "description": text(getattr(rna, "description", "")),
                    "available": operator_available(operator),
                    "properties": min(count, 4096),
                }
            )
    return {"items": items, "truncated": matched > len(items)}


def iter_types(exact_identifier=None):
    # bpy.types exposes part of its RNA surface lazily, so dir(bpy.types) alone can omit
    # valid classes such as Mesh on some Blender builds. Resolve an exact identifier first,
    # then combine visible attributes with the actual bpy_struct subclass graph.
    candidates = {}

    def remember(candidate):
        rna = getattr(candidate, "bl_rna", None)
        identifier = str(getattr(rna, "identifier", "")) if rna is not None else ""
        if identifier:
            candidates.setdefault(identifier, (candidate, rna))

    if (
        isinstance(exact_identifier, str)
        and exact_identifier
        and len(exact_identifier) <= 256
        and exact_identifier.replace("_", "").isalnum()
    ):
        remember(getattr(bpy.types, exact_identifier, None))
        root_type = getattr(bpy.types, "bpy_struct", None)
        resolver = getattr(root_type, "bl_rna_get_subclass_py", None)
        if callable(resolver):
            try:
                remember(resolver(exact_identifier, None))
            except Exception:
                pass

    for attr_name in sorted(name for name in dir(bpy.types) if not name.startswith("_")):
        remember(getattr(bpy.types, attr_name, None))

    root = getattr(bpy.types, "bpy_struct", None)
    pending = [root] if root is not None else []
    seen = set()
    while pending and len(seen) < 100000:
        candidate = pending.pop()
        if candidate in seen:
            continue
        seen.add(candidate)
        remember(candidate)
        try:
            pending.extend(candidate.__subclasses__())
        except Exception:
            pass

    identifiers = sorted(candidates)
    if exact_identifier in candidates:
        yield candidates[exact_identifier]
        identifiers.remove(exact_identifier)
    for identifier in identifiers:
        yield candidates[identifier]


def search_types(args):
    raw_query = args.get("query", "")
    query = query_arg(args)
    limit = limit_arg(args)
    items = []
    matched = 0
    for _candidate, rna in iter_types(raw_query):
        identifier = text(getattr(rna, "identifier", ""), 256)
        name = text(getattr(rna, "name", ""), 512)
        description = text(getattr(rna, "description", ""))
        haystack = " ".join([identifier, name, description]).casefold()
        if query and query not in haystack:
            continue
        matched += 1
        if len(items) < limit:
            base = getattr(rna, "base", None)
            items.append(
                {
                    "identifier": identifier,
                    "name": name,
                    "description": description,
                    "base": text(getattr(base, "identifier", ""), 256) if base else None,
                    "properties": min(len(list(rna.properties)), 4096),
                }
            )
    return {"items": items, "truncated": matched > len(items)}


def addon_modules():
    try:
        return list(addon_utils.modules(refresh=False))
    except TypeError:
        return list(addon_utils.modules())


def search_addons(args):
    query = query_arg(args)
    limit = limit_arg(args)
    enabled = set(bpy.context.preferences.addons.keys())
    items = []
    matched = 0
    for module in sorted(addon_modules(), key=lambda value: getattr(value, "__name__", "")):
        module_name = text(getattr(module, "__name__", ""), 512)
        info = getattr(module, "bl_info", {}) or {}
        name = text(info.get("name", module_name), 512)
        version = info.get("version", ())
        if isinstance(version, (tuple, list)):
            version = ".".join(str(part) for part in version[:8])
        version = text(version, 128)
        if query and query not in f"{module_name} {name}".casefold():
            continue
        matched += 1
        if len(items) < limit:
            items.append(
                {
                    "module": module_name,
                    "name": name,
                    "version": version,
                    "enabled": module_name in enabled,
                }
            )
    return {"items": items, "truncated": matched > len(items)}


def summary():
    type_count = sum(1 for _ in iter_types())
    categories = set()
    operator_count = 0
    for identifier, _operator, _rna in iter_operators():
        operator_count += 1
        categories.add(identifier.split(".", 1)[0])
    modules = addon_modules()
    enabled = set(bpy.context.preferences.addons.keys())
    return {
        "version": list(bpy.app.version[:3]),
        "version_string": text(bpy.app.version_string, 128),
        "rna_types": type_count,
        "operator_categories": len(categories),
        "operators": operator_count,
        "available_addons": len(modules),
        "enabled_addons": len(enabled),
        "arbitrary_python": False,
        "generic_operator_invoke": False,
    }


def dispatch(command, args):
    if not isinstance(command, str) or not isinstance(args, dict):
        raise CommandError("InvalidArgument", "Blender driver request is malformed")
    if command == "driver.blender.semantic.summary":
        if args:
            raise CommandError("InvalidArgument", "Semantic summary accepts no arguments")
        return semantic.summary()
    if command == "driver.blender.semantic.types":
        return semantic.types(args.get("query", ""), limit_arg(args))
    if command == "driver.blender.semantic.type.describe":
        if set(args) != {"root"}:
            raise CommandError("InvalidArgument", "Semantic type description requires root")
        return semantic.type_describe(args["root"])
    if command == "driver.blender.semantic.objects":
        if not set(args).issubset({"root", "query", "limit", "offset"}) or "root" not in args:
            raise CommandError("InvalidArgument", "Semantic object listing requires root")
        offset = args.get("offset", 0)
        if isinstance(offset, bool) or not isinstance(offset, int) or not 0 <= offset <= 1_000_000:
            raise CommandError("InvalidArgument", "Semantic object offset is invalid")
        return semantic.objects(args["root"], args.get("query", ""), limit_arg(args), offset)
    if command == "driver.blender.semantic.object.describe":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Semantic object description requires ref")
        return semantic.object_describe(args["ref"])
    if command == "driver.blender.semantic.relations":
        if not set(args).issubset({"ref", "property", "limit", "offset"}) or not {"ref", "property"}.issubset(args):
            raise CommandError("InvalidArgument", "Semantic relation traversal requires ref and property")
        offset = args.get("offset", 0)
        if isinstance(offset, bool) or not isinstance(offset, int) or not 0 <= offset <= 1_000_000:
            raise CommandError("InvalidArgument", "Semantic relation offset is invalid")
        return semantic.relations(args["ref"], args["property"], limit_arg(args), offset)
    if command == "driver.blender.semantic.query":
        if not set(args).issubset({"root", "property", "operator", "value", "limit", "offset"}) or not {"root", "property", "operator", "value"}.issubset(args):
            raise CommandError("InvalidArgument", "Semantic query requires root, property, operator and value")
        offset = args.get("offset", 0)
        if isinstance(offset, bool) or not isinstance(offset, int) or not 0 <= offset <= 1_000_000:
            raise CommandError("InvalidArgument", "Semantic query offset is invalid")
        return semantic.query(args["root"], args["property"], args["operator"], args["value"], limit_arg(args), offset)
    if command == "driver.blender.semantic.rna.describe":
        if set(args) != {"identifier"}:
            raise CommandError("InvalidArgument", "RNA type description requires identifier")
        return semantic.rna_describe(args["identifier"])
    if command == "driver.blender.semantic.rename":
        if set(args) != {"ref", "name"}:
            raise CommandError("InvalidArgument", "Semantic rename requires ref and name")
        return semantic.rename(args["ref"], args["name"])
    if command == "driver.blender.semantic.custom.list":
        if not set(args).issubset({"ref", "limit", "offset"}) or "ref" not in args:
            raise CommandError("InvalidArgument", "Custom property listing requires ref")
        offset = args.get("offset", 0)
        if isinstance(offset, bool) or not isinstance(offset, int) or not 0 <= offset <= 1_000_000:
            raise CommandError("InvalidArgument", "Custom property offset is invalid")
        return semantic.custom_list(args["ref"], limit_arg(args), offset)
    if command == "driver.blender.semantic.custom.get":
        if set(args) != {"ref", "key"}:
            raise CommandError("InvalidArgument", "Custom property read requires ref and key")
        return semantic.custom_get(args["ref"], args["key"])
    if command == "driver.blender.semantic.custom.set":
        if set(args) != {"ref", "key", "value"}:
            raise CommandError("InvalidArgument", "Custom property write requires ref, key and value")
        return semantic.custom_set(args["ref"], args["key"], args["value"])
    if command == "driver.blender.semantic.custom.remove":
        if set(args) != {"ref", "key"}:
            raise CommandError("InvalidArgument", "Custom property removal requires ref and key")
        return semantic.custom_remove(args["ref"], args["key"])
    if command == "driver.blender.asset.load":
        if set(args) != {"root", "path", "name"}:
            raise CommandError("InvalidArgument", "Asset load requires root, path and name")
        return semantic.asset_load(args["root"], args["path"], args["name"])
    if command == "driver.blender.semantic.object.create":
        if not set(args).issubset({"name", "data_ref", "collection_ref"}) or "name" not in args:
            raise CommandError("InvalidArgument", "Semantic Object create requires name")
        return semantic.object_create(args["name"], args.get("data_ref"), args.get("collection_ref"))
    if command == "driver.blender.semantic.datablock.create":
        if not set(args).issubset({"root", "name", "kind"}) or not {"root", "name"}.issubset(args):
            raise CommandError("InvalidArgument", "Datablock create requires root and name")
        return semantic.datablock_create(args["root"], args["name"], args.get("kind"))
    if command == "driver.blender.semantic.datablock.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Datablock remove requires ref")
        return semantic.datablock_remove(args["ref"])
    if command == "driver.blender.semantic.relation.link":
        if set(args) != {"ref", "property", "target_ref"}:
            raise CommandError("InvalidArgument", "Semantic relation link requires ref, property and target_ref")
        return semantic.relation_link(args["ref"], args["property"], args["target_ref"])
    if command == "driver.blender.semantic.relation.unlink":
        if set(args) != {"ref", "property", "target_ref"}:
            raise CommandError("InvalidArgument", "Semantic relation unlink requires ref, property and target_ref")
        return semantic.relation_unlink(args["ref"], args["property"], args["target_ref"])
    if command == "driver.blender.semantic.relation.set":
        if set(args) != {"ref", "property", "target_ref"}:
            raise CommandError("InvalidArgument", "Semantic relation write requires ref, property and target_ref")
        return semantic.relation_set(args["ref"], args["property"], args["target_ref"])
    if command == "driver.blender.mesh.summary":
        if set(args) != {"mesh_ref"}:
            raise CommandError("InvalidArgument", "Mesh summary requires mesh_ref")
        return semantic.mesh_summary(args["mesh_ref"])
    if command == "driver.blender.mesh.geometry.replace":
        if set(args) != {"mesh_ref", "vertices", "edges", "faces"}:
            raise CommandError("InvalidArgument", "Mesh geometry replace requires mesh_ref, vertices, edges and faces")
        return semantic.mesh_geometry_replace(args["mesh_ref"], args["vertices"], args["edges"], args["faces"])
    if command == "driver.blender.mesh.attribute.add":
        if set(args) != {"mesh_ref", "name", "data_type", "domain"}:
            raise CommandError("InvalidArgument", "Mesh attribute add requires mesh_ref, name, data_type and domain")
        return semantic.mesh_attribute_add(args["mesh_ref"], args["name"], args["data_type"], args["domain"])
    if command == "driver.blender.mesh.attribute.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Mesh attribute remove requires ref")
        return semantic.mesh_attribute_remove(args["ref"])
    if command == "driver.blender.mesh.uv_layer.add":
        if not set(args).issubset({"mesh_ref", "name", "do_init"}) or not {"mesh_ref", "name"}.issubset(args):
            raise CommandError("InvalidArgument", "UV layer add requires mesh_ref and name")
        return semantic.mesh_uv_layer_add(args["mesh_ref"], args["name"], args.get("do_init", True))
    if command == "driver.blender.mesh.uv_layer.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "UV layer remove requires ref")
        return semantic.mesh_uv_layer_remove(args["ref"])
    if command == "driver.blender.vertex_group.add":
        if set(args) != {"object_ref", "name"}:
            raise CommandError("InvalidArgument", "Vertex-group add requires object_ref and name")
        return semantic.vertex_group_add(args["object_ref"], args["name"])
    if command == "driver.blender.vertex_group.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Vertex-group remove requires ref")
        return semantic.vertex_group_remove(args["ref"])
    if command == "driver.blender.vertex_group.weights.set":
        if not set(args).issubset({"ref", "indices", "weight", "mode"}) or not {"ref", "indices", "weight"}.issubset(args):
            raise CommandError("InvalidArgument", "Vertex-group weight set requires ref, indices and weight")
        return semantic.vertex_group_weights_set(args["ref"], args["indices"], args["weight"], args.get("mode", "REPLACE"))
    if command == "driver.blender.vertex_group.weights.remove":
        if set(args) != {"ref", "indices"}:
            raise CommandError("InvalidArgument", "Vertex-group weight remove requires ref and indices")
        return semantic.vertex_group_weights_remove(args["ref"], args["indices"])
    if command == "driver.blender.shape_key.add":
        if not set(args).issubset({"object_ref", "name", "from_mix"}) or not {"object_ref", "name"}.issubset(args):
            raise CommandError("InvalidArgument", "Shape-key add requires object_ref and name")
        return semantic.shape_key_add(args["object_ref"], args["name"], args.get("from_mix", False))
    if command == "driver.blender.shape_key.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Shape-key remove requires ref")
        return semantic.shape_key_remove(args["ref"])
    if command == "driver.blender.curve.spline.add":
        if not set(args).issubset({"curve_ref", "type", "points"}) or not {"curve_ref", "type"}.issubset(args):
            raise CommandError("InvalidArgument", "Spline add requires curve_ref and type")
        return semantic.spline_add(args["curve_ref"], args["type"], args.get("points", 1))
    if command == "driver.blender.curve.spline.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Spline remove requires ref")
        return semantic.spline_remove(args["ref"])
    if command == "driver.blender.armature.bone.add":
        if not set(args).issubset({"object_ref", "name", "head", "tail", "parent_name", "connected"}) or not {"object_ref", "name", "head", "tail"}.issubset(args):
            raise CommandError("InvalidArgument", "Bone add requires object_ref, name, head and tail")
        return semantic.armature_bone_add(args["object_ref"], args["name"], args["head"], args["tail"], args.get("parent_name"), args.get("connected", False))
    if command == "driver.blender.armature.bone.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Bone remove requires ref")
        return semantic.armature_bone_remove(args["ref"])
    if command == "driver.blender.armature.bone.parent.set":
        if not set(args).issubset({"ref", "parent_ref", "connected"}) or not {"ref", "parent_ref"}.issubset(args):
            raise CommandError("InvalidArgument", "Bone parent set requires ref and parent_ref")
        return semantic.armature_bone_parent_set(args["ref"], args["parent_ref"], args.get("connected", False))
    if command == "driver.blender.pose_constraint.add":
        if set(args) != {"pose_bone_ref", "name", "type"}:
            raise CommandError("InvalidArgument", "Pose constraint add requires pose_bone_ref, name and type")
        return semantic.pose_constraint_add(args["pose_bone_ref"], args["name"], args["type"])
    if command == "driver.blender.pose_constraint.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Pose constraint remove requires ref")
        return semantic.pose_constraint_remove(args["ref"])
    if command == "driver.blender.bone_collection.add":
        if not set(args).issubset({"armature_ref", "name", "parent_ref"}) or not {"armature_ref", "name"}.issubset(args):
            raise CommandError("InvalidArgument", "Bone collection add requires armature_ref and name")
        return semantic.bone_collection_add(args["armature_ref"], args["name"], args.get("parent_ref"))
    if command == "driver.blender.bone_collection.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Bone collection remove requires ref")
        return semantic.bone_collection_remove(args["ref"])
    if command == "driver.blender.bone_collection.assign":
        if set(args) != {"collection_ref", "bone_ref"}:
            raise CommandError("InvalidArgument", "Bone collection assign requires collection_ref and bone_ref")
        return semantic.bone_collection_assign(args["collection_ref"], args["bone_ref"], True)
    if command == "driver.blender.bone_collection.unassign":
        if set(args) != {"collection_ref", "bone_ref"}:
            raise CommandError("InvalidArgument", "Bone collection unassign requires collection_ref and bone_ref")
        return semantic.bone_collection_assign(args["collection_ref"], args["bone_ref"], False)
    if command == "driver.blender.action.slot.add":
        if set(args) != {"action_ref", "id_type", "name"}:
            raise CommandError("InvalidArgument", "Action slot add requires action_ref, id_type and name")
        return semantic.action_slot_add(args["action_ref"], args["id_type"], args["name"])
    if command == "driver.blender.action.slot.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Action slot remove requires ref")
        return semantic.action_slot_remove(args["ref"])
    if command == "driver.blender.action.layer.add":
        if set(args) != {"action_ref", "name"}:
            raise CommandError("InvalidArgument", "Action layer add requires action_ref and name")
        return semantic.action_layer_add(args["action_ref"], args["name"])
    if command == "driver.blender.action.layer.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Action layer remove requires ref")
        return semantic.action_layer_remove(args["ref"])
    if command == "driver.blender.action.strip.add":
        if set(args) != {"layer_ref"}:
            raise CommandError("InvalidArgument", "Action strip add requires layer_ref")
        return semantic.action_strip_add(args["layer_ref"])
    if command == "driver.blender.action.strip.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Action strip remove requires ref")
        return semantic.action_strip_remove(args["ref"])
    if command == "driver.blender.action.channelbag.ensure":
        if set(args) != {"strip_ref", "slot_ref"}:
            raise CommandError("InvalidArgument", "Action channelbag ensure requires strip_ref and slot_ref")
        return semantic.action_channelbag_ensure(args["strip_ref"], args["slot_ref"])
    if command == "driver.blender.action.fcurve.ensure":
        if not set(args).issubset({"channelbag_ref", "data_path", "index", "group_name"}) or not {"channelbag_ref", "data_path"}.issubset(args):
            raise CommandError("InvalidArgument", "Action F-Curve ensure requires channelbag_ref and data_path")
        return semantic.action_fcurve_ensure(args["channelbag_ref"], args["data_path"], args.get("index", 0), args.get("group_name", ""))
    if command == "driver.blender.action.fcurve.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Action F-Curve remove requires ref")
        return semantic.action_fcurve_remove(args["ref"])
    if command == "driver.blender.nla.track.add":
        if not set(args).issubset({"owner_ref", "name", "previous_ref"}) or not {"owner_ref", "name"}.issubset(args):
            raise CommandError("InvalidArgument", "NLA track add requires owner_ref and name")
        return semantic.nla_track_add(args["owner_ref"], args["name"], args.get("previous_ref"))
    if command == "driver.blender.nla.track.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "NLA track remove requires ref")
        return semantic.nla_track_remove(args["ref"])
    if command == "driver.blender.nla.strip.add":
        if set(args) != {"track_ref", "name", "start", "action_ref"}:
            raise CommandError("InvalidArgument", "NLA strip add requires track_ref, name, start and action_ref")
        return semantic.nla_strip_add(args["track_ref"], args["name"], args["start"], args["action_ref"])
    if command == "driver.blender.nla.strip.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "NLA strip remove requires ref")
        return semantic.nla_strip_remove(args["ref"])
    if command == "driver.blender.fcurve.keyframe.add":
        if not set(args).issubset({"fcurve_ref", "frame", "value", "keyframe_type"}) or not {"fcurve_ref", "frame", "value"}.issubset(args):
            raise CommandError("InvalidArgument", "F-Curve keyframe add requires fcurve_ref, frame and value")
        return semantic.fcurve_keyframe_add(args["fcurve_ref"], args["frame"], args["value"], args.get("keyframe_type", "KEYFRAME"))
    if command == "driver.blender.fcurve.keyframe.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "F-Curve keyframe remove requires ref")
        return semantic.fcurve_keyframe_remove(args["ref"])
    if command == "driver.blender.fcurve.modifier.add":
        if set(args) != {"fcurve_ref", "type"}:
            raise CommandError("InvalidArgument", "F-Curve modifier add requires fcurve_ref and type")
        return semantic.fcurve_modifier_add(args["fcurve_ref"], args["type"])
    if command == "driver.blender.fcurve.modifier.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "F-Curve modifier remove requires ref")
        return semantic.fcurve_modifier_remove(args["ref"])
    if command == "driver.blender.scene.view_layer.add":
        if set(args) != {"scene_ref", "name"}:
            raise CommandError("InvalidArgument", "View-layer add requires scene_ref and name")
        return semantic.view_layer_add(args["scene_ref"], args["name"])
    if command == "driver.blender.scene.view_layer.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "View-layer remove requires ref")
        return semantic.view_layer_remove(args["ref"])
    if command == "driver.blender.scene.view_layer.move":
        if set(args) != {"ref", "to_index"}:
            raise CommandError("InvalidArgument", "View-layer move requires ref and to_index")
        return semantic.view_layer_move(args["ref"], args["to_index"])
    if command == "driver.blender.scene.marker.add":
        if set(args) != {"scene_ref", "name", "frame"}:
            raise CommandError("InvalidArgument", "Timeline-marker add requires scene_ref, name and frame")
        return semantic.timeline_marker_add(args["scene_ref"], args["name"], args["frame"])
    if command == "driver.blender.scene.marker.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Timeline-marker remove requires ref")
        return semantic.timeline_marker_remove(args["ref"])
    if command == "driver.blender.sequence.ensure":
        if set(args) != {"scene_ref"}:
            raise CommandError("InvalidArgument", "Sequence ensure requires scene_ref")
        return semantic.sequence_editor_ensure(args["scene_ref"])
    if command == "driver.blender.sequence.media.add":
        if not set(args).issubset({"scene_ref", "kind", "name", "path", "channel", "frame_start", "fit_method"}) or not {"scene_ref", "kind", "name", "path", "channel", "frame_start"}.issubset(args):
            raise CommandError("InvalidArgument", "Sequence media add requires scene_ref, kind, name, path, channel and frame_start")
        return semantic.sequence_media_add(args["scene_ref"], args["kind"], args["name"], args["path"], args["channel"], args["frame_start"], args.get("fit_method", "ORIGINAL"))
    if command == "driver.blender.sequence.datablock.add":
        if set(args) != {"scene_ref", "kind", "name", "source_ref", "channel", "frame_start"}:
            raise CommandError("InvalidArgument", "Sequence datablock add requires scene_ref, kind, name, source_ref, channel and frame_start")
        return semantic.sequence_datablock_add(args["scene_ref"], args["kind"], args["name"], args["source_ref"], args["channel"], args["frame_start"])
    if command == "driver.blender.sequence.meta.add":
        if set(args) != {"scene_ref", "name", "channel", "frame_start"}:
            raise CommandError("InvalidArgument", "Sequence meta add requires scene_ref, name, channel and frame_start")
        return semantic.sequence_meta_add(args["scene_ref"], args["name"], args["channel"], args["frame_start"])
    if command == "driver.blender.sequence.effect.add":
        if not set(args).issubset({"scene_ref", "name", "type", "channel", "frame_start", "frame_end", "input1_ref", "input2_ref"}) or not {"scene_ref", "name", "type", "channel", "frame_start"}.issubset(args):
            raise CommandError("InvalidArgument", "Sequence effect add requires scene_ref, name, type, channel and frame_start")
        return semantic.sequence_effect_add(args["scene_ref"], args["name"], args["type"], args["channel"], args["frame_start"], args.get("frame_end", 0), args.get("input1_ref"), args.get("input2_ref"))
    if command == "driver.blender.sequence.strip.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Sequence strip remove requires ref")
        return semantic.sequence_strip_remove(args["ref"])
    if command == "driver.blender.sequence.modifier.add":
        if set(args) != {"strip_ref", "name", "type"}:
            raise CommandError("InvalidArgument", "Sequence modifier add requires strip_ref, name and type")
        return semantic.sequence_modifier_add(args["strip_ref"], args["name"], args["type"])
    if command == "driver.blender.sequence.modifier.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Sequence modifier remove requires ref")
        return semantic.sequence_modifier_remove(args["ref"])
    if command == "driver.blender.mask.layer.add":
        if set(args) != {"mask_ref", "name"}:
            raise CommandError("InvalidArgument", "Mask layer add requires mask_ref and name")
        return semantic.mask_layer_add(args["mask_ref"], args["name"])
    if command == "driver.blender.mask.layer.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Mask layer remove requires ref")
        return semantic.mask_layer_remove(args["ref"])
    if command == "driver.blender.mask.spline.add":
        if not set(args).issubset({"layer_ref", "points"}) or "layer_ref" not in args:
            raise CommandError("InvalidArgument", "Mask spline add requires layer_ref")
        return semantic.mask_spline_add(args["layer_ref"], args.get("points", 1))
    if command == "driver.blender.mask.spline.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Mask spline remove requires ref")
        return semantic.mask_spline_remove(args["ref"])
    if command == "driver.blender.mask.points.add":
        if set(args) != {"spline_ref", "count"}:
            raise CommandError("InvalidArgument", "Mask points add requires spline_ref and count")
        return semantic.mask_points_add(args["spline_ref"], args["count"])
    if command == "driver.blender.mask.point.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Mask point remove requires ref")
        return semantic.mask_point_remove(args["ref"])
    if command == "driver.blender.modifier.add":
        if set(args) != {"object_ref", "name", "type"}:
            raise CommandError("InvalidArgument", "Modifier add requires object_ref, name and type")
        return semantic.modifier_add(args["object_ref"], args["name"], args["type"])
    if command == "driver.blender.modifier.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Modifier remove requires ref")
        return semantic.modifier_remove(args["ref"])
    if command == "driver.blender.constraint.add":
        if set(args) != {"object_ref", "name", "type"}:
            raise CommandError("InvalidArgument", "Constraint add requires object_ref, name and type")
        return semantic.constraint_add(args["object_ref"], args["name"], args["type"])
    if command == "driver.blender.constraint.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Constraint remove requires ref")
        return semantic.constraint_remove(args["ref"])
    if command == "driver.blender.animation.keyframe.insert":
        if not set(args).issubset({"ref", "property", "frame", "index"}) or not {"ref", "property", "frame"}.issubset(args):
            raise CommandError("InvalidArgument", "Keyframe insert requires ref, property and frame")
        return semantic.keyframe_insert(args["ref"], args["property"], args["frame"], args.get("index", -1))
    if command == "driver.blender.animation.keyframe.delete":
        if not set(args).issubset({"ref", "property", "frame", "index"}) or not {"ref", "property", "frame"}.issubset(args):
            raise CommandError("InvalidArgument", "Keyframe delete requires ref, property and frame")
        return semantic.keyframe_delete(args["ref"], args["property"], args["frame"], args.get("index", -1))
    if command == "driver.blender.node.types":
        return semantic.node_types(args.get("query", ""), limit_arg(args))
    if command == "driver.blender.node.interface.socket.add":
        if not set(args).issubset({"interface_ref", "name", "in_out", "socket_type", "description", "parent_ref"}) or not {"interface_ref", "name"}.issubset(args):
            raise CommandError("InvalidArgument", "Interface socket add requires interface_ref and name")
        return semantic.node_interface_socket_add(args["interface_ref"], args["name"], args.get("in_out", "INPUT"), args.get("socket_type", "DEFAULT"), args.get("description", ""), args.get("parent_ref"))
    if command == "driver.blender.node.interface.panel.add":
        if not set(args).issubset({"interface_ref", "name", "description", "default_closed"}) or not {"interface_ref", "name"}.issubset(args):
            raise CommandError("InvalidArgument", "Interface panel add requires interface_ref and name")
        return semantic.node_interface_panel_add(args["interface_ref"], args["name"], args.get("description", ""), args.get("default_closed", False))
    if command == "driver.blender.node.interface.item.remove":
        if not set(args).issubset({"ref", "move_content_to_parent"}) or "ref" not in args:
            raise CommandError("InvalidArgument", "Interface item remove requires ref")
        return semantic.node_interface_item_remove(args["ref"], args.get("move_content_to_parent", True))
    if command == "driver.blender.node.interface.item.move":
        if set(args) != {"ref", "to_position"}:
            raise CommandError("InvalidArgument", "Interface item move requires ref and to_position")
        return semantic.node_interface_item_move(args["ref"], args["to_position"])
    if command == "driver.blender.node.interface.item.move_to_parent":
        if set(args) != {"ref", "parent_ref", "to_position"}:
            raise CommandError("InvalidArgument", "Interface item reparent requires ref, parent_ref and to_position")
        return semantic.node_interface_item_move_to_parent(args["ref"], args["parent_ref"], args["to_position"])
    if command == "driver.blender.node.add":
        if not set(args).issubset({"tree_ref", "type", "name"}) or not {"tree_ref", "type"}.issubset(args):
            raise CommandError("InvalidArgument", "Node add requires tree_ref and type")
        return semantic.node_add(args["tree_ref"], args["type"], args.get("name"))
    if command == "driver.blender.node.remove":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Node remove requires ref")
        return semantic.node_remove(args["ref"])
    if command == "driver.blender.node.link":
        if set(args) != {"tree_ref", "from_socket_ref", "to_socket_ref"}:
            raise CommandError("InvalidArgument", "Node link requires tree_ref and two socket refs")
        return semantic.node_link(args["tree_ref"], args["from_socket_ref"], args["to_socket_ref"])
    if command == "driver.blender.node.unlink":
        if set(args) != {"ref"}:
            raise CommandError("InvalidArgument", "Node unlink requires ref")
        return semantic.node_unlink(args["ref"])
    if command == "driver.blender.semantic.property.get":
        if set(args) != {"ref", "property"}:
            raise CommandError("InvalidArgument", "Semantic property read requires ref and property")
        return semantic.property_get(args["ref"], args["property"])
    if command == "driver.blender.semantic.property.set":
        if set(args) != {"ref", "property", "value"}:
            raise CommandError("InvalidArgument", "Semantic property write requires ref, property and value")
        return semantic.property_set(args["ref"], args["property"], args["value"] )
    if command == "driver.blender.semantic.property.reset":
        if set(args) != {"ref", "property"}:
            raise CommandError("InvalidArgument", "Semantic property reset requires ref and property")
        return semantic.property_reset(args["ref"], args["property"])
    if command == "driver.blender.introspect.summary":
        if args:
            raise CommandError("InvalidArgument", "Summary accepts no arguments")
        return summary()
    if command == "driver.blender.introspect.operators":
        return search_operators(args)
    if command == "driver.blender.introspect.operator.describe":
        if set(args) != {"id"}:
            raise CommandError("InvalidArgument", "Operator description requires only id")
        return describe_operator(args["id"])
    if command == "driver.blender.introspect.types":
        return search_types(args)
    if command == "driver.blender.introspect.addons":
        return search_addons(args)
    if command.startswith("driver.blender."):
        data = commands("blender." + command[len("driver.blender."):], args)
        if command in {
            "driver.blender.object.create", "driver.blender.object.delete",
            "driver.blender.object.transform", "driver.blender.collection.create",
            "driver.blender.collection.link", "driver.blender.material.create",
            "driver.blender.material.assign", "driver.blender.render.settings",
            "driver.blender.file.open",
        }:
            semantic.changed()
        return data
    raise CommandError("Unsupported", "Command is outside the Blender driver namespace")


if os.path.exists(socket_path) or os.path.islink(socket_path):
    raise SystemExit(3)

listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
listener.bind(socket_path)
os.chmod(socket_path, 0o600)
listener.listen(4)

while True:
    connection, _ = listener.accept()
    with connection:
        try:
            request = read_frame(connection)
            if (
                not isinstance(request, dict)
                or set(request) != {"command", "args"}
            ):
                raise CommandError("InvalidArgument", "Malformed Blender driver frame")
            data = dispatch(request["command"], request["args"])
            write_frame(connection, {"ok": True, "data": data})
        except (CommandError, SemanticError) as error:
            write_frame(connection, {"ok": False, "error": {"code": error.code}})
        except (ValueError, TypeError, EOFError, OSError):
            try:
                write_frame(connection, {"ok": False, "error": {"code": "BackendFailed"}})
            except Exception:
                pass
