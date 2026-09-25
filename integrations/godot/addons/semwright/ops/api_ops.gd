@tool
extends RefCounted

const MAX_CLASSES := 256
const MAX_PROPERTIES := 512
const MAX_METHODS := 512
const MAX_SIGNALS := 256
const MAX_ENUMS := 128
const MAX_CONSTANTS := 512
const MAX_SCRIPT_CLASSES := 256

static func search(ctx, args: Dictionary) -> Dictionary:
    var query := str(args.get("query", "")).strip_edges().to_lower()
    var base := str(args.get("base", "")).strip_edges()
    var limit := mini(int(args.get("limit", 128)), MAX_CLASSES)
    if limit < 1:
        return ctx._error("invalid_argument", "api.search limit must be positive")
    if not base.is_empty() and not ClassDB.class_exists(base):
        return ctx._error("not_found", "Godot base class does not exist")
    var rows: Array = []
    var total := 0
    var classes := Array(ClassDB.get_class_list())
    classes.sort()
    for raw in classes:
        var name := str(raw)
        if not query.is_empty() and not name.to_lower().contains(query):
            continue
        if not base.is_empty() and name != base and not ClassDB.is_parent_class(name, base):
            continue
        total += 1
        if rows.size() >= limit:
            continue
        rows.append({
            "name": name,
            "parent": str(ClassDB.get_parent_class(name)),
            "api_type": _api_type_name(ClassDB.class_get_api_type(name)),
            "can_instantiate": ClassDB.can_instantiate(name),
            "enabled": ClassDB.is_class_enabled(name),
        })
    return {"stamp": ctx._stamp(), "data": {
        "engine_version": str(Engine.get_version_info().get("string", "unknown")),
        "query": query,
        "base": base,
        "classes": rows,
        "total_matches": total,
        "truncated": total > rows.size(),
    }}

static func describe(ctx, args: Dictionary) -> Dictionary:
    var class_name := str(args.get("class", ""))
    if not ClassDB.class_exists(class_name):
        return ctx._error("not_found", "Godot class does not exist")
    var inherited := bool(args.get("include_inherited", true))
    var no_inheritance := not inherited
    var data := {
        "engine_version": str(Engine.get_version_info().get("string", "unknown")),
        "class": class_name,
        "parent": str(ClassDB.get_parent_class(class_name)),
        "api_type": _api_type_name(ClassDB.class_get_api_type(class_name)),
        "can_instantiate": ClassDB.can_instantiate(class_name),
        "enabled": ClassDB.is_class_enabled(class_name),
        "properties": [],
        "methods": [],
        "signals": [],
        "enums": [],
    }
    if bool(args.get("properties", true)):
        data["properties"] = _class_properties(ctx, class_name, no_inheritance)
    if bool(args.get("methods", true)):
        data["methods"] = _methods(ctx, ClassDB.class_get_method_list(class_name, no_inheritance), MAX_METHODS)
    if bool(args.get("signals", true)):
        data["signals"] = _methods(ctx, ClassDB.class_get_signal_list(class_name, no_inheritance), MAX_SIGNALS)
    if bool(args.get("enums", true)):
        data["enums"] = _class_enums(class_name, no_inheritance)
    return {"stamp": ctx._stamp(), "data": data}

static func project_class_list(ctx, args: Dictionary) -> Dictionary:
    var query := str(args.get("query", "")).strip_edges().to_lower()
    var limit := mini(int(args.get("limit", 128)), MAX_SCRIPT_CLASSES)
    if limit < 1:
        return ctx._error("invalid_argument", "project.class.list limit must be positive")
    var rows: Array = []
    var total := 0
    var classes := Array(ProjectSettings.get_global_class_list())
    classes.sort_custom(func(a, b): return str(a.get("class", "")) < str(b.get("class", "")))
    for row in classes:
        var name := str(row.get("class", ""))
        if not query.is_empty() and not name.to_lower().contains(query):
            continue
        total += 1
        if rows.size() >= limit:
            continue
        rows.append(_global_class_row(row))
    return {"stamp": ctx._stamp(), "data": {
        "classes": rows,
        "total_matches": total,
        "truncated": total > rows.size(),
    }}

static func project_class_describe(ctx, args: Dictionary) -> Dictionary:
    var wanted := str(args.get("name", ""))
    var found := {}
    for row in ProjectSettings.get_global_class_list():
        if str(row.get("class", "")) == wanted:
            found = row
            break
    if found.is_empty():
        return ctx._error("not_found", "project global class does not exist")
    var data := _global_class_row(found)
    var path := str(found.get("path", ""))
    if not ctx._safe_res(path) or not ResourceLoader.exists(path):
        data["metadata_available"] = false
        return {"stamp": ctx._stamp(), "data": data}
    var script = ResourceLoader.load(path, "Script", ResourceLoader.CACHE_MODE_IGNORE)
    if not (script is Script):
        data["metadata_available"] = false
        return {"stamp": ctx._stamp(), "data": data}
    data["metadata_available"] = true
    data["global_name"] = str(script.get_global_name())
    data["instance_base_type"] = str(script.get_instance_base_type())
    data["can_instantiate"] = script.can_instantiate()
    data["tool"] = script.is_tool()
    data["abstract"] = script.is_abstract()
    data["has_source_code"] = script.has_source_code()
    data["properties"] = _properties_from_metadata(ctx, script.get_script_property_list(), MAX_PROPERTIES)
    data["methods"] = _methods(ctx, script.get_script_method_list(), MAX_METHODS)
    data["signals"] = _methods(ctx, script.get_script_signal_list(), MAX_SIGNALS)
    var constants: Array = []
    for name in script.get_script_constant_map():
        if constants.size() >= MAX_CONSTANTS:
            break
        constants.append({
            "name": str(name),
            "value": ctx._encode_value(script.get_script_constant_map()[name]),
        })
    constants.sort_custom(func(a, b): return str(a["name"]) < str(b["name"]))
    data["constants"] = constants
    data["constants_truncated"] = script.get_script_constant_map().size() > MAX_CONSTANTS
    data["rpc_config"] = ctx._encode_value(script.get_rpc_config())
    return {"stamp": ctx._stamp(), "data": data}

static func _class_properties(ctx, class_name: String, no_inheritance: bool) -> Array:
    var rows := _properties_from_metadata(
        ctx,
        ClassDB.class_get_property_list(class_name, no_inheritance),
        MAX_PROPERTIES,
    )
    for row in rows:
        var name := str(row.get("name", ""))
        if not name.is_empty():
            row["default"] = ctx._encode_value(
                ClassDB.class_get_property_default_value(class_name, name)
            )
            row["getter"] = str(ClassDB.class_get_property_getter(class_name, name))
            row["setter"] = str(ClassDB.class_get_property_setter(class_name, name))
    return rows

static func _properties_from_metadata(ctx, metadata: Array, limit: int) -> Array:
    var rows: Array = []
    for raw in metadata:
        if rows.size() >= limit:
            break
        if typeof(raw) != TYPE_DICTIONARY:
            continue
        rows.append(_property_metadata(ctx, raw))
    return rows

static func _property_metadata(_ctx, meta: Dictionary) -> Dictionary:
    return {
        "name": str(meta.get("name", "")),
        "type": int(meta.get("type", TYPE_NIL)),
        "type_name": type_string(int(meta.get("type", TYPE_NIL))),
        "class_name": str(meta.get("class_name", "")),
        "hint": int(meta.get("hint", 0)),
        "hint_string": str(meta.get("hint_string", "")).left(1024),
        "usage": int(meta.get("usage", 0)),
        "read_only": int(meta.get("usage", 0)) & PROPERTY_USAGE_READ_ONLY != 0,
    }

static func _methods(ctx, metadata: Array, limit: int) -> Array:
    var rows: Array = []
    for raw in metadata:
        if rows.size() >= limit:
            break
        if typeof(raw) != TYPE_DICTIONARY:
            continue
        var args: Array = []
        for arg in raw.get("args", []):
            if args.size() >= 64 or typeof(arg) != TYPE_DICTIONARY:
                break
            args.append(_property_metadata(ctx, arg))
        var defaults: Array = []
        for value in raw.get("default_args", []):
            if defaults.size() >= 64:
                break
            defaults.append(ctx._encode_value(value))
        var ret = raw.get("return", {})
        rows.append({
            "name": str(raw.get("name", "")),
            "flags": int(raw.get("flags", 0)),
            "id": int(raw.get("id", 0)),
            "args": args,
            "default_args": defaults,
            "return": _property_metadata(ctx, ret) if typeof(ret) == TYPE_DICTIONARY else {},
        })
    return rows

static func _class_enums(class_name: String, no_inheritance: bool) -> Array:
    var rows: Array = []
    for raw_enum in ClassDB.class_get_enum_list(class_name, no_inheritance):
        if rows.size() >= MAX_ENUMS:
            break
        var enum_name := str(raw_enum)
        var constants: Array = []
        for raw_constant in ClassDB.class_get_enum_constants(class_name, enum_name, no_inheritance):
            if constants.size() >= MAX_CONSTANTS:
                break
            var constant := str(raw_constant)
            constants.append({
                "name": constant,
                "value": ClassDB.class_get_integer_constant(class_name, constant),
            })
        rows.append({
            "name": enum_name,
            "bitfield": ClassDB.is_class_enum_bitfield(class_name, enum_name, no_inheritance),
            "constants": constants,
        })
    return rows

static func _global_class_row(row: Dictionary) -> Dictionary:
    return {
        "name": str(row.get("class", "")),
        "base": str(row.get("base", "")),
        "language": str(row.get("language", "")),
        "path": str(row.get("path", "")),
        "icon": str(row.get("icon", "")),
    }

static func _api_type_name(value: int) -> String:
    match value:
        ClassDB.API_CORE: return "core"
        ClassDB.API_EDITOR: return "editor"
        ClassDB.API_EXTENSION: return "extension"
        ClassDB.API_EDITOR_EXTENSION: return "editor_extension"
        _: return "unknown"
