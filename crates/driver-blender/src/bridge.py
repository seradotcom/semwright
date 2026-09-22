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

commands = Commands(bpy, workspace)


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
        return commands("blender." + command[len("driver.blender."):], args)
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
        except CommandError as error:
            write_frame(connection, {"ok": False, "error": {"code": error.code}})
        except (ValueError, TypeError, EOFError, OSError):
            try:
                write_frame(connection, {"ok": False, "error": {"code": "BackendFailed"}})
            except Exception:
                pass
