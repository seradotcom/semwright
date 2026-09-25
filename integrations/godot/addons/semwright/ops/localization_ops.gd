@tool
extends RefCounted

const TRANSLATIONS_SETTING := "internationalization/locale/translations"
const FALLBACK_SETTING := "internationalization/locale/fallback"
const TEST_SETTING := "internationalization/locale/test"
const MAX_MESSAGES := 2048

static func inspect(ctx, _args: Dictionary) -> Dictionary:
    var translations: Array = []
    for raw in ProjectSettings.get_setting(TRANSLATIONS_SETTING, PackedStringArray()):
        translations.append(str(raw))
    return {"stamp": ctx._stamp(), "data": {
        "fallback": str(ProjectSettings.get_setting(FALLBACK_SETTING, "en")),
        "test_locale": str(ProjectSettings.get_setting(TEST_SETTING, "")),
        "translations": translations,
        "loaded_locales": Array(TranslationServer.get_loaded_locales()),
    }}

static func configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var touched: Array[String] = []
    if args.has("fallback"):
        touched.append(FALLBACK_SETTING)
    if args.has("test_locale"):
        touched.append(TEST_SETTING)
    if args.has("translations"):
        for path in args["translations"]:
            var p := str(path)
            if not ctx._safe_res(p) or not ResourceLoader.exists(p):
                return ctx._error("not_found", "translation resource does not exist")
            var resource = ResourceLoader.load(p, "Translation", ResourceLoader.CACHE_MODE_IGNORE)
            if not (resource is Translation):
                return ctx._error("invalid_argument", "translation path is not a Translation resource")
        touched.append(TRANSLATIONS_SETTING)
    if touched.is_empty():
        return ctx._error("invalid_argument", "no localization changes supplied")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, touched, "dry-run")
    if args.has("fallback"):
        ProjectSettings.set_setting(FALLBACK_SETTING, str(args["fallback"]))
    if args.has("test_locale"):
        ProjectSettings.set_setting(TEST_SETTING, str(args["test_locale"]))
    if args.has("translations"):
        var packed := PackedStringArray()
        for path in args["translations"]:
            packed.append(str(path))
        ProjectSettings.set_setting(TRANSLATIONS_SETTING, packed)
    if ProjectSettings.save() != OK:
        return ctx._error("backend_failed", "failed to persist localization settings")
    ctx._revision += 1
    return ctx._mutation_result(true, touched, "Configure localization")

static func translation_inspect(ctx, args: Dictionary) -> Dictionary:
    var loaded = _translation(ctx, str(args.get("path", "")))
    if loaded is Dictionary:
        return loaded
    var translation: Translation = loaded
    var rows: Array = []
    var truncated := false
    for message in translation.get_message_list():
        if rows.size() >= MAX_MESSAGES:
            truncated = true
            break
        var key := str(message)
        rows.append({"source": key, "translation": str(translation.get_message(message))})
    return {"stamp": ctx._stamp(), "data": {
        "path": translation.resource_path,
        "locale": translation.locale,
        "message_count": translation.get_message_count(),
        "messages": rows,
        "truncated": truncated,
    }}
static func translation_create(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var path := str(args.get("path", ""))
    var locale := str(args.get("locale", ""))
    if not ctx._safe_res(path) or not path.ends_with(".translation") or locale.is_empty():
        return ctx._error("invalid_argument", "invalid translation path or locale")
    if ResourceLoader.exists(path):
        return ctx._error("conflict", "translation resource already exists")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [path], "dry-run")
    var translation := Translation.new()
    translation.locale = locale
    if ResourceSaver.save(translation, path) != OK:
        return ctx._error("backend_failed", "failed to save translation resource")
    EditorInterface.get_resource_filesystem().update_file(path)
    if bool(args.get("register", false)):
        var existing := PackedStringArray()
        for raw in ProjectSettings.get_setting(TRANSLATIONS_SETTING, PackedStringArray()):
            existing.append(str(raw))
        if not existing.has(path):
            existing.append(path)
            ProjectSettings.set_setting(TRANSLATIONS_SETTING, existing)
            if ProjectSettings.save() != OK:
                return ctx._error("backend_failed", "failed to register translation resource")
    ctx._revision += 1
    return ctx._mutation_result(true, [path], "Create translation")
static func message_set(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var path := str(args.get("path", ""))
    var loaded = _translation(ctx, path)
    if loaded is Dictionary:
        return loaded
    var translation: Translation = loaded
    var source := str(args.get("source", ""))
    var translated := str(args.get("translation", ""))
    var context := str(args.get("context", ""))
    if source.is_empty():
        return ctx._error("invalid_argument", "translation source is required")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [path + "#" + source], "dry-run")
    translation.add_message(StringName(source), StringName(translated), StringName(context))
    if ResourceSaver.save(translation, path) != OK:
        return ctx._error("backend_failed", "failed to save translation")
    ctx._revision += 1
    return ctx._mutation_result(true, [path + "#" + source], "Set translation message")

static func message_remove(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty():
        return conflict
    var path := str(args.get("path", ""))
    var loaded = _translation(ctx, path)
    if loaded is Dictionary:
        return loaded
    var translation: Translation = loaded
    var source := str(args.get("source", ""))
    var context := str(args.get("context", ""))
    if source.is_empty():
        return ctx._error("invalid_argument", "translation source is required")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [path + "#" + source], "dry-run")
    translation.erase_message(StringName(source), StringName(context))
    if ResourceSaver.save(translation, path) != OK:
        return ctx._error("backend_failed", "failed to save translation")
    ctx._revision += 1
    return ctx._mutation_result(true, [path + "#" + source], "Remove translation message")
static func _translation(ctx, path: String):
    if not ctx._safe_res(path) or not ResourceLoader.exists(path):
        return ctx._error("not_found", "translation resource does not exist")
    var resource = ResourceLoader.load(path, "Translation", ResourceLoader.CACHE_MODE_IGNORE)
    if not (resource is Translation):
        return ctx._error("invalid_argument", "resource is not a Translation")
    return resource
