@tool
extends RefCounted

const MAX_THEME_ITEMS := 512

static func control_inspect(ctx, args: Dictionary) -> Dictionary:
    var control = ctx._resolve_node(str(args.get("target", "")))
    if not (control is Control): return ctx._error("not_found", "Control node not found")
    var theme_path := ""
    if control.theme != null: theme_path = control.theme.resource_path
    return {"stamp":ctx._stamp(),"data":{
        "target":str(args.get("target", "")),"class":control.get_class(),
        "visible":control.visible,"focus_mode":control.focus_mode,"mouse_filter":control.mouse_filter,
        "tooltip_text":control.tooltip_text,"layout_direction":control.layout_direction,
        "custom_minimum_size":[control.custom_minimum_size.x,control.custom_minimum_size.y],
        "size_flags_horizontal":control.size_flags_horizontal,
        "size_flags_vertical":control.size_flags_vertical,
        "size_flags_stretch_ratio":control.size_flags_stretch_ratio,
        "theme":theme_path,"theme_type_variation":str(control.theme_type_variation),
    }}

static func control_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var control = ctx._resolve_node(str(args.get("target", "")))
    if not (control is Control): return ctx._error("not_found", "Control node not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    for key in ["visible"]:
        if args.has(key): control.set(key,bool(args[key]))
    for key in ["focus_mode","mouse_filter","layout_direction","size_flags_horizontal","size_flags_vertical"]:
        if args.has(key): control.set(key,int(args[key]))
    if args.has("size_flags_stretch_ratio"): control.size_flags_stretch_ratio = float(args["size_flags_stretch_ratio"])
    if args.has("tooltip_text"): control.tooltip_text = str(args["tooltip_text"])
    if args.has("theme_type_variation"): control.theme_type_variation = StringName(str(args["theme_type_variation"]))
    if args.has("minimum_size"):
        var size = _array_v2(args["minimum_size"])
        if size == null: return ctx._error("invalid_argument", "minimum_size must be a 2-number array")
        control.custom_minimum_size = size
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure UI control")

static func text_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var node = ctx._resolve_node(str(args.get("target", "")))
    if node == null or not _has_property(node,"text"):
        return ctx._error("invalid_argument", "target does not expose text")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("text"): node.set("text",str(args["text"]))
    for key in ["horizontal_alignment","vertical_alignment","autowrap_mode","text_overrun_behavior"]:
        if args.has(key):
            if not _has_property(node,key): return ctx._error("invalid_argument", "%s unsupported by target" % key)
            node.set(key,int(args[key]))
    for key in ["uppercase","editable","secret"]:
        if args.has(key):
            if not _has_property(node,key): return ctx._error("invalid_argument", "%s unsupported by target" % key)
            node.set(key,bool(args[key]))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure UI text")

static func theme_inspect(ctx, args: Dictionary) -> Dictionary:
    var theme = _theme(ctx,str(args.get("path", "")))
    if theme == null: return ctx._error("not_found", "Theme resource not found")
    var items: Array = []
    for theme_type in _theme_types(theme):
        for name in theme.get_color_list(theme_type):
            if items.size() >= MAX_THEME_ITEMS: break
            items.append({"kind":"color","type":theme_type,"name":str(name),"value":_color(theme.get_color(name,theme_type))})
        for name in theme.get_constant_list(theme_type):
            if items.size() >= MAX_THEME_ITEMS: break
            items.append({"kind":"constant","type":theme_type,"name":str(name),"value":theme.get_constant(name,theme_type)})
        for name in theme.get_font_size_list(theme_type):
            if items.size() >= MAX_THEME_ITEMS: break
            items.append({"kind":"font_size","type":theme_type,"name":str(name),"value":theme.get_font_size(name,theme_type)})
        if items.size() >= MAX_THEME_ITEMS: break
    return {"stamp":ctx._stamp(),"data":{
        "path":theme.resource_path,"default_font_size":theme.default_font_size,
        "types":_theme_types(theme),"items":items,"truncated":items.size() >= MAX_THEME_ITEMS,
    }}

static func theme_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var theme = _theme(ctx,str(args.get("path", "")))
    if theme == null: return ctx._error("not_found", "Theme resource not found")
    var kind = str(args.get("kind", ""))
    var theme_type = StringName(str(args.get("theme_type", "")))
    var name = StringName(str(args.get("name", "")))
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [theme.resource_path], "dry-run")
    match kind:
        "color":
            var color = _array_color(args.get("color"))
            if color == null: return ctx._error("invalid_argument", "theme color must have 3 or 4 components")
            theme.set_color(name,theme_type,color)
        "constant":
            theme.set_constant(name,theme_type,int(args.get("integer",0)))
        "font_size":
            theme.set_font_size(name,theme_type,int(args.get("integer",16)))
        "font":
            var font = _load_resource(ctx,str(args.get("resource","")),"Font")
            if not (font is Font): return ctx._error("not_found", "Font resource not found")
            theme.set_font(name,theme_type,font)
        "icon":
            var icon = _load_resource(ctx,str(args.get("resource","")),"Texture2D")
            if not (icon is Texture2D): return ctx._error("not_found", "Texture2D icon not found")
            theme.set_icon(name,theme_type,icon)
        "stylebox":
            var box = _load_resource(ctx,str(args.get("resource","")),"StyleBox")
            if not (box is StyleBox): return ctx._error("not_found", "StyleBox resource not found")
            theme.set_stylebox(name,theme_type,box)
        "type_variation":
            theme.set_type_variation(theme_type,StringName(str(args.get("base_type",""))))
        _:
            return ctx._error("invalid_argument", "unsupported theme item kind")
    if ResourceSaver.save(theme,theme.resource_path) != OK:
        return ctx._error("backend_failed", "failed to save Theme")
    EditorInterface.get_resource_filesystem().update_file(theme.resource_path)
    ctx._revision += 1
    return ctx._mutation_result(true, [theme.resource_path], "Configure theme item")

static func theme_apply(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var control = ctx._resolve_node(str(args.get("target", "")))
    if not (control is Control): return ctx._error("not_found", "Control node not found")
    var path = str(args.get("theme", ""))
    var theme: Theme = null
    if not path.is_empty():
        theme = _theme(ctx,path)
        if theme == null: return ctx._error("not_found", "Theme resource not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", "")),path], "dry-run")
    control.theme = theme
    if args.has("type_variation"): control.theme_type_variation = StringName(str(args["type_variation"]))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", "")),path], "Apply UI theme")

static func _theme(ctx, path: String) -> Theme:
    if not ctx._safe_res(path) or not ResourceLoader.exists(path): return null
    return ResourceLoader.load(path,"Theme",ResourceLoader.CACHE_MODE_REPLACE) as Theme

static func _theme_types(theme: Theme) -> Array:
    var seen := {}
    for list in [theme.get_color_type_list(),theme.get_constant_type_list(),theme.get_font_size_type_list(),theme.get_font_type_list(),theme.get_icon_type_list(),theme.get_stylebox_type_list()]:
        for item in list: seen[str(item)] = true
    var result: Array = seen.keys()
    result.sort()
    return result

static func _load_resource(ctx, path: String, type_hint: String):
    if not ctx._safe_res(path) or not ResourceLoader.exists(path): return null
    return ResourceLoader.load(path,type_hint,ResourceLoader.CACHE_MODE_REUSE)

static func _has_property(object: Object, name: String) -> bool:
    for item in object.get_property_list():
        if str(item.get("name", "")) == name: return true
    return false

static func _array_v2(value):
    if not (value is Array) or value.size() != 2: return null
    return Vector2(float(value[0]),float(value[1]))

static func _array_color(value):
    if not (value is Array) or value.size() not in [3,4]: return null
    return Color(float(value[0]),float(value[1]),float(value[2]),1.0 if value.size()==3 else float(value[3]))

static func _color(value: Color) -> Array:
    return [value.r,value.g,value.b,value.a]
