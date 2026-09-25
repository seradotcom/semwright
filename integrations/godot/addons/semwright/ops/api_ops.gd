@tool
extends RefCounted

const MAX_SEARCH := 256
const MAX_MEMBERS := 256
const MAX_INHERITANCE := 32

static func search(ctx, args: Dictionary) -> Dictionary:
    var query := str(args.get("query", "")).to_lower()
    var source := str(args.get("source", "all"))
    var base := str(args.get("base", ""))
    var limit := clampi(int(args.get("limit", 64)), 1, MAX_SEARCH)
    var rows: Array = []
    var truncated := false
    if source in ["all", "engine"]:
        for raw_name in ClassDB.get_class_list():
            var name := str(raw_name)
            if not query.is_empty() and not name.to_lower().contains(query):
                continue
            if not base.is_empty() and name != base and not ClassDB.is_parent_class(name, base):
                continue
            if rows.size() >= limit:
                truncated = true
                break
            rows.append(_engine_class_summary(name))
    if source in ["all", "script"] and rows.size() < limit:
        for raw in ProjectSettings.get_global_class_list():
            var name := str(raw.get("class", ""))
            var parent := str(raw.get("base", ""))
            var path := str(raw.get("path", ""))
            if not query.is_empty() and not name.to_lower().contains(query):
                continue
            if not base.is_empty() and name != base and parent != base:
                continue
            if rows.size() >= limit:
                truncated = true
                break
            rows.append({
                "class": name,
                "parent": parent,
                "source": "script",
                "path": path,
                "language": str(raw.get("language", "")),
                "instantiable": true,
            })
    return {"stamp": ctx._stamp(), "data": {
        "engine": str(Engine.get_version_info().get("string", "unknown")),
        "query": query, "source": source, "base": base,
        "classes": rows, "truncated": truncated,
    }}

static func describe(ctx, args: Dictionary) -> Dictionary:
    var klass := str(args.get("class", ""))
    if ClassDB.class_exists(klass):
        return {"stamp": ctx._stamp(), "data": _describe_engine_class(klass)}
    for raw in ProjectSettings.get_global_class_list():
        if str(raw.get("class", "")) != klass:
            continue
        var path := str(raw.get("path", ""))
        if not ctx._safe_res(path) or not ResourceLoader.exists(path):
            return ctx._error("not_found", "global script class resource not found")
        var script = ResourceLoader.load(path, "Script", ResourceLoader.CACHE_MODE_IGNORE)
        if not (script is Script):
            return ctx._error("backend_failed", "global class is not a Script resource")
        return {"stamp": ctx._stamp(), "data": {
            "engine": str(Engine.get_version_info().get("string", "unknown")),
            "class": klass,
            "parent": str(raw.get("base", "")),
            "source": "script",
            "path": path,
            "language": str(raw.get("language", "")),
            "instantiable": script.can_instantiate(),
            "inheritance": _script_inheritance(script),
            "properties": _normalize_members(script.get_script_property_list(), "property"),
            "methods": _normalize_members(script.get_script_method_list(), "method"),
            "signals": _normalize_members(script.get_script_signal_list(), "signal"),
            "enums": [],
            "constants": [],
        }}
    return ctx._error("not_found", "Godot class not found")

static func _engine_class_summary(klass: String) -> Dictionary:
    var parent := str(ClassDB.get_parent_class(klass))
    var kind := "object"
    if klass == "Node" or ClassDB.is_parent_class(klass, "Node"):
        kind = "node"
    elif klass == "Resource" or ClassDB.is_parent_class(klass, "Resource"):
        kind = "resource"
    return {
        "class": klass,
        "parent": parent,
        "source": "engine",
        "api_type": int(ClassDB.class_get_api_type(klass)),
        "kind": kind,
        "instantiable": ClassDB.can_instantiate(klass),
    }

static func _describe_engine_class(klass: String) -> Dictionary:
    var chain: Array = []
    var current := klass
    while not current.is_empty() and chain.size() < MAX_INHERITANCE:
        chain.append(current)
        current = str(ClassDB.get_parent_class(current))
    var enums: Array = []
    for enum_name in ClassDB.class_get_enum_list(klass):
        if enums.size() >= MAX_MEMBERS:
            break
        var values := {}
        for constant_name in ClassDB.class_get_enum_constants(klass, enum_name):
            values[str(constant_name)] = ClassDB.class_get_integer_constant(klass, constant_name)
        enums.append({
            "name": str(enum_name),
            "bitfield": ClassDB.is_class_enum_bitfield(klass, enum_name),
            "values": values,
        })
    var constants: Array = []
    for constant_name in ClassDB.class_get_integer_constant_list(klass):
        if constants.size() >= MAX_MEMBERS:
            break
        constants.append({
            "name": str(constant_name),
            "value": ClassDB.class_get_integer_constant(klass, constant_name),
        })
    var data := _engine_class_summary(klass)
    data["engine"] = str(Engine.get_version_info().get("string", "unknown"))
    data["inheritance"] = chain
    data["properties"] = _normalize_members(ClassDB.class_get_property_list(klass), "property")
    data["methods"] = _normalize_members(ClassDB.class_get_method_list(klass), "method")
    data["signals"] = _normalize_members(ClassDB.class_get_signal_list(klass), "signal")
    data["enums"] = enums
    data["constants"] = constants
    return data

static func _normalize_members(raw_members: Array, kind: String) -> Array:
    var rows: Array = []
    for raw in raw_members:
        if rows.size() >= MAX_MEMBERS:
            break
        var row := {"name": str(raw.get("name", ""))}
        if raw.has("type"):
            row["type"] = int(raw.get("type", TYPE_NIL))
            row["type_name"] = type_string(int(raw.get("type", TYPE_NIL)))
        if raw.has("class_name"):
            row["class_name"] = str(raw.get("class_name", ""))
        if raw.has("hint"):
            row["hint"] = int(raw.get("hint", 0))
        if raw.has("hint_string"):
            row["hint_string"] = str(raw.get("hint_string", "")).left(1024)
        if raw.has("usage"):
            row["usage"] = int(raw.get("usage", 0))
        if kind in ["method", "signal"]:
            row["flags"] = int(raw.get("flags", 0))
            var args: Array = []
            for arg in raw.get("args", []):
                if args.size() >= 64:
                    break
                args.append(_normalize_argument(arg))
            row["args"] = args
            if raw.has("return"):
                row["return"] = _normalize_argument(raw.get("return", {}))
        rows.append(row)
    return rows

static func _normalize_argument(raw) -> Dictionary:
    if not (raw is Dictionary):
        return {"name": "", "type": TYPE_NIL, "type_name": "Nil", "class_name": ""}
    var t := int(raw.get("type", TYPE_NIL))
    return {
        "name": str(raw.get("name", "")),
        "type": t,
        "type_name": type_string(t),
        "class_name": str(raw.get("class_name", "")),
        "hint": int(raw.get("hint", 0)),
        "hint_string": str(raw.get("hint_string", "")).left(1024),
        "usage": int(raw.get("usage", 0)),
    }

static func _script_inheritance(script: Script) -> Array:
    var chain: Array = []
    var current: Script = script
    while current != null and chain.size() < MAX_INHERITANCE:
        chain.append(str(current.get_global_name()))
        current = current.get_base_script()
    return chain
