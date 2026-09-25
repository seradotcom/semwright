@tool
extends RefCounted

const PRESETS_PATH := "res://export_presets.cfg"
const MAX_OPTIONS := 128

static func list(ctx, _args: Dictionary) -> Dictionary:
    var loaded = _load()
    if loaded.has("_error"):
        return ctx._error(loaded["_code"], loaded["_error"])
    var cfg: ConfigFile = loaded["cfg"]
    var rows: Array = []
    for section in cfg.get_sections():
        var name := str(section)
        if not name.begins_with("preset.") or name.ends_with(".options"):
            continue
        rows.append(_preset_summary(cfg, name))
    rows.sort_custom(func(a, b): return str(a["name"]) < str(b["name"]))
    return {"stamp": ctx._stamp(), "data": {"presets": rows}}

static func inspect(ctx, args: Dictionary) -> Dictionary:
    var loaded = _load()
    if loaded.has("_error"):
        return ctx._error(loaded["_code"], loaded["_error"])
    var cfg: ConfigFile = loaded["cfg"]
    var section := _find_section(cfg, str(args.get("name", "")))
    if section.is_empty():
        return ctx._error("not_found", "export preset not found")
    var data := _preset_summary(cfg, section)
    var options := {}
    var option_section := section + ".options"
    var count := 0
    for key in cfg.get_section_keys(option_section):
        if count >= MAX_OPTIONS:
            break
        var value = cfg.get_value(option_section, key)
        options[str(key)] = value if ctx._json_safe(value) else str(value)
        count += 1
    data["options"] = options
    data["options_truncated"] = cfg.get_section_keys(option_section).size() > MAX_OPTIONS
    return {"stamp": ctx._stamp(), "data": data}
static func configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var loaded = _load()
    if loaded.has("_error"):
        return ctx._error(loaded["_code"], loaded["_error"])
    var cfg: ConfigFile = loaded["cfg"]
    var section := _find_section(cfg, str(args.get("name", "")))
    if section.is_empty():
        return ctx._error("not_found", "export preset not found")
    var touched: Array[String] = []
    var fields := {
        "export_path": "export_path",
        "runnable": "runnable",
        "custom_features": "custom_features",
        "export_filter": "export_filter",
        "include_filter": "include_filter",
        "exclude_filter": "exclude_filter",
        "dedicated_server": "dedicated_server",
    }
    for semantic_name in fields:
        if args.has(semantic_name):
            touched.append(section + "/" + fields[semantic_name])
    var options: Array = args.get("options", [])
    for item in options:
        var key := str(item.get("key", ""))
        if key.is_empty() or key.length() > 160:
            return ctx._error("invalid_argument", "invalid export option key")
        var option_section := section + ".options"
        if not cfg.has_section_key(option_section, key):
            return ctx._error("invalid_argument", "unknown export option")
        if typeof(item.get("value")) not in [TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING]:
            return ctx._error("invalid_argument", "export option value must be scalar")
        touched.append(option_section + "/" + key)
    if touched.is_empty():
        return ctx._error("invalid_argument", "no export preset changes supplied")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, touched, "dry-run")
    for semantic_name in fields:
        if args.has(semantic_name):
            cfg.set_value(section, fields[semantic_name], args[semantic_name])
    for item in options:
        cfg.set_value(section + ".options", str(item["key"]), item["value"])
    if cfg.save(PRESETS_PATH) != OK:
        return ctx._error("backend_failed", "failed to save export presets")
    ctx._revision += 1
    return ctx._mutation_result(true, touched, "Configure export preset")

static func _load() -> Dictionary:
    if not FileAccess.file_exists(PRESETS_PATH):
        return {"_error": "export_presets.cfg does not exist", "_code": "not_found"}
    var cfg := ConfigFile.new()
    if cfg.load(PRESETS_PATH) != OK:
        return {"_error": "failed to load export presets", "_code": "backend_failed"}
    return {"cfg": cfg}

static func _find_section(cfg: ConfigFile, preset_name: String) -> String:
    if preset_name.is_empty():
        return ""
    for section in cfg.get_sections():
        var name := str(section)
        if name.begins_with("preset.") and not name.ends_with(".options"):
            if str(cfg.get_value(name, "name", "")) == preset_name:
                return name
    return ""

static func _preset_summary(cfg: ConfigFile, section: String) -> Dictionary:
    return {
        "name": str(cfg.get_value(section, "name", "")),
        "platform": str(cfg.get_value(section, "platform", "")),
        "runnable": bool(cfg.get_value(section, "runnable", false)),
        "dedicated_server": bool(cfg.get_value(section, "dedicated_server", false)),
        "custom_features": str(cfg.get_value(section, "custom_features", "")),
        "export_filter": str(cfg.get_value(section, "export_filter", "all_resources")),
        "include_filter": str(cfg.get_value(section, "include_filter", "")),
        "exclude_filter": str(cfg.get_value(section, "exclude_filter", "")),
        "export_path": str(cfg.get_value(section, "export_path", "")),
    }
