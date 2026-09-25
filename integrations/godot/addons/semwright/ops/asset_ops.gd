@tool
extends RefCounted

const MAX_DEPENDENCIES := 512
const MAX_IMPORT_PARAMS := 128

static func inspect(ctx, args: Dictionary) -> Dictionary:
    var path := str(args.get("path", ""))
    if not ctx._safe_res(path) or not FileAccess.file_exists(path):
        return ctx._error("not_found", "asset source does not exist")
    var fs := EditorInterface.get_resource_filesystem()
    var file := FileAccess.open(path, FileAccess.READ)
    var bytes := 0
    if file != null:
        bytes = file.get_length()
        file.close()
    var uid := ResourceLoader.get_resource_uid(path)
    return {"stamp": ctx._stamp(), "data": {
        "path": path,
        "type": fs.get_file_type(path),
        "bytes": bytes,
        "resource_exists": ResourceLoader.exists(path),
        "resource_uid": uid,
        "import_sidecar": FileAccess.file_exists(path + ".import"),
        "is_importing": fs.is_importing(),
    }}

static func dependencies(ctx, args: Dictionary) -> Dictionary:
    var path := str(args.get("path", ""))
    if not ctx._safe_res(path) or not ResourceLoader.exists(path):
        return ctx._error("not_found", "resource does not exist")
    var rows: Array = []
    var truncated := false
    for raw in ResourceLoader.get_dependencies(path):
        if rows.size() >= MAX_DEPENDENCIES:
            truncated = true
            break
        var text := str(raw)
        if text.contains("::"):
            rows.append({"uid": text.get_slice("::", 0), "path": text.get_slice("::", 2)})
        else:
            rows.append({"uid": "", "path": text})
    return {"stamp": ctx._stamp(), "data": {"dependencies": rows, "truncated": truncated}}
static func reimport(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var path := str(args.get("path", ""))
    if not ctx._safe_res(path) or not FileAccess.file_exists(path):
        return ctx._error("not_found", "asset source does not exist")
    var fs := EditorInterface.get_resource_filesystem()
    if fs.is_importing() or fs.is_scanning():
        return ctx._error("unavailable", "resource filesystem is busy")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [path], "dry-run")
    fs.reimport_files(PackedStringArray([path]))
    ctx._revision += 1
    return ctx._mutation_result(true, [path], "Reimport asset")

static func import_inspect(ctx, args: Dictionary) -> Dictionary:
    var path := str(args.get("path", ""))
    if not ctx._safe_res(path) or not FileAccess.file_exists(path):
        return ctx._error("not_found", "asset source does not exist")
    var sidecar := path + ".import"
    if not FileAccess.file_exists(sidecar):
        return ctx._error("not_found", "asset has no import sidecar")
    var cfg := ConfigFile.new()
    var err := cfg.load(sidecar)
    if err != OK:
        return ctx._error("backend_failed", "failed to load import sidecar")
    var params := {}
    var count := 0
    for key in cfg.get_section_keys("params"):
        if count >= MAX_IMPORT_PARAMS:
            break
        var value = cfg.get_value("params", key)
        params[str(key)] = value if ctx._json_safe(value) else str(value)
        count += 1
    return {"stamp": ctx._stamp(), "data": {
        "path": path,
        "importer": str(cfg.get_value("remap", "importer", "")),
        "type": str(cfg.get_value("remap", "type", "")),
        "params": params,
        "truncated": cfg.get_section_keys("params").size() > MAX_IMPORT_PARAMS,
    }}
static func import_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var path := str(args.get("path", ""))
    if not ctx._safe_res(path) or not FileAccess.file_exists(path):
        return ctx._error("not_found", "asset source does not exist")
    var sidecar := path + ".import"
    if not FileAccess.file_exists(sidecar):
        return ctx._error("not_found", "asset has no import sidecar")
    var cfg := ConfigFile.new()
    if cfg.load(sidecar) != OK:
        return ctx._error("backend_failed", "failed to load import sidecar")
    var updates: Array = args.get("params", [])
    if updates.is_empty():
        return ctx._error("invalid_argument", "no import parameters supplied")
    for item in updates:
        var key := str(item.get("key", ""))
        if key.is_empty() or not cfg.has_section_key("params", key):
            return ctx._error("invalid_argument", "unknown import parameter")
        var value = item.get("value")
        if typeof(value) not in [TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING]:
            return ctx._error("invalid_argument", "import parameter value must be scalar")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [path], "dry-run")
    for item in updates:
        cfg.set_value("params", str(item["key"]), item["value"])
    if cfg.save(sidecar) != OK:
        return ctx._error("backend_failed", "failed to save import sidecar")
    var fs := EditorInterface.get_resource_filesystem()
    if fs.is_importing() or fs.is_scanning():
        return ctx._error("unavailable", "resource filesystem became busy")
    fs.reimport_files(PackedStringArray([path]))
    ctx._revision += 1
    return ctx._mutation_result(true, [path], "Configure and reimport asset")
