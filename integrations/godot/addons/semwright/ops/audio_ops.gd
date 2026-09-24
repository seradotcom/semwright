@tool
extends RefCounted

const BUS_LAYOUT_PATH := "res://default_bus_layout.tres"
const MAX_BUSES := 64
const MAX_EFFECTS := 32

static func player_inspect(ctx, args: Dictionary) -> Dictionary:
    var player = ctx._resolve_node(str(args.get("target", "")))
    if not _is_player(player): return ctx._error("not_found", "AudioStreamPlayer node not found")
    var stream_path := ""
    if player.stream != null: stream_path = player.stream.resource_path
    var data := {
        "target":str(args.get("target", "")),
        "class":player.get_class(),
        "stream":stream_path,
        "bus":str(player.bus),
        "volume_db":player.volume_db,
        "pitch_scale":player.pitch_scale,
        "autoplay":player.autoplay,
        "playing":player.playing,
        "max_polyphony":player.max_polyphony,
    }
    for key in ["max_distance","unit_size","panning_strength","attenuation_model","doppler_tracking"]:
        if _has_property(player, key): data[key] = player.get(key)
    return {"stamp":ctx._stamp(),"data":data}

static func player_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var player = ctx._resolve_node(str(args.get("target", "")))
    if not _is_player(player): return ctx._error("not_found", "AudioStreamPlayer node not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, [str(args.get("target", ""))], "dry-run")
    if args.has("stream"):
        var path = str(args["stream"])
        if path.is_empty():
            player.stream = null
        elif ctx._safe_res(path) and ResourceLoader.exists(path):
            var stream = ResourceLoader.load(path, "AudioStream", ResourceLoader.CACHE_MODE_REUSE)
            if not (stream is AudioStream): return ctx._error("invalid_argument", "resource is not AudioStream")
            player.stream = stream
        else:
            return ctx._error("not_found", "AudioStream resource not found")
    if args.has("bus"):
        var bus = str(args["bus"])
        if AudioServer.get_bus_index(bus) < 0: return ctx._error("not_found", "audio bus not found")
        player.bus = StringName(bus)
    for key in ["volume_db","pitch_scale"]:
        if args.has(key): player.set(key, float(args[key]))
    if args.has("autoplay"): player.autoplay = bool(args["autoplay"])
    if args.has("max_polyphony"): player.max_polyphony = int(args["max_polyphony"])
    for key in ["max_distance","unit_size","panning_strength"]:
        if args.has(key):
            if not _has_property(player,key): return ctx._error("invalid_argument", "%s is not supported by this audio player" % key)
            player.set(key,float(args[key]))
    for key in ["attenuation_model","doppler_tracking"]:
        if args.has(key):
            if not _has_property(player,key): return ctx._error("invalid_argument", "%s is not supported by this audio player" % key)
            player.set(key,int(args[key]))
    EditorInterface.mark_scene_as_unsaved()
    ctx._revision += 1
    return ctx._mutation_result(true, [str(args.get("target", ""))], "Configure audio player")

static func bus_inspect(ctx, _args: Dictionary) -> Dictionary:
    var buses: Array = []
    var count = min(AudioServer.get_bus_count(), MAX_BUSES)
    for i in count:
        var effects: Array = []
        var effect_count = min(AudioServer.get_bus_effect_count(i), MAX_EFFECTS)
        for j in effect_count:
            var effect = AudioServer.get_bus_effect(i,j)
            effects.append({
                "index":j,
                "class":"" if effect == null else effect.get_class(),
                "resource":"" if effect == null else effect.resource_path,
            })
        buses.append({
            "index":i,
            "name":AudioServer.get_bus_name(i),
            "volume_db":AudioServer.get_bus_volume_db(i),
            "mute":AudioServer.is_bus_mute(i),
            "solo":AudioServer.is_bus_solo(i),
            "send":str(AudioServer.get_bus_send(i)),
            "effects":effects,
        })
    return {"stamp":ctx._stamp(),"data":{"buses":buses,"truncated":AudioServer.get_bus_count() > MAX_BUSES}}

static func bus_create(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var name = str(args.get("name", ""))
    if name.is_empty() or name.length() > 96 or AudioServer.get_bus_index(name) >= 0:
        return ctx._error("conflict", "invalid or duplicate audio bus name")
    if AudioServer.get_bus_count() >= MAX_BUSES: return ctx._error("invalid_argument", "audio bus limit reached")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, ["audio_bus:" + name], "dry-run")
    AudioServer.add_bus(int(args.get("position", -1)))
    var index = AudioServer.get_bus_count() - 1
    if int(args.get("position", -1)) >= 0: index = int(args["position"])
    AudioServer.set_bus_name(index, name)
    if args.has("volume_db"): AudioServer.set_bus_volume_db(index, float(args["volume_db"]))
    if args.has("send"):
        var send = str(args["send"])
        if AudioServer.get_bus_index(send) < 0: return ctx._error("not_found", "audio send bus not found")
        AudioServer.set_bus_send(index, StringName(send))
    var persisted = _persist(ctx)
    if not persisted.is_empty(): return persisted
    ctx._revision += 1
    return ctx._mutation_result(true, ["audio_bus:" + name], "Create audio bus")

static func bus_configure(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var name = str(args.get("name", ""))
    var index = AudioServer.get_bus_index(name)
    if index < 0: return ctx._error("not_found", "audio bus not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, ["audio_bus:" + name], "dry-run")
    if args.has("volume_db"): AudioServer.set_bus_volume_db(index, float(args["volume_db"]))
    if args.has("mute"): AudioServer.set_bus_mute(index, bool(args["mute"]))
    if args.has("solo"): AudioServer.set_bus_solo(index, bool(args["solo"]))
    if args.has("send"):
        var send = str(args["send"])
        if AudioServer.get_bus_index(send) < 0: return ctx._error("not_found", "audio send bus not found")
        AudioServer.set_bus_send(index, StringName(send))
    var persisted = _persist(ctx)
    if not persisted.is_empty(): return persisted
    ctx._revision += 1
    return ctx._mutation_result(true, ["audio_bus:" + name], "Configure audio bus")

static func bus_remove(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var name = str(args.get("name", ""))
    var index = AudioServer.get_bus_index(name)
    if index <= 0: return ctx._error("permission_denied", "Master or missing audio bus cannot be removed")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, ["audio_bus:" + name], "dry-run")
    AudioServer.remove_bus(index)
    var persisted = _persist(ctx)
    if not persisted.is_empty(): return persisted
    ctx._revision += 1
    return ctx._mutation_result(true, ["audio_bus:" + name], "Remove audio bus")

static func effect_add(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var bus = AudioServer.get_bus_index(str(args.get("bus", "")))
    var path = str(args.get("effect", ""))
    if bus < 0 or not ctx._safe_res(path) or not ResourceLoader.exists(path):
        return ctx._error("not_found", "audio bus or effect resource not found")
    var effect = ResourceLoader.load(path, "AudioEffect", ResourceLoader.CACHE_MODE_REUSE)
    if not (effect is AudioEffect): return ctx._error("invalid_argument", "resource is not AudioEffect")
    if AudioServer.get_bus_effect_count(bus) >= MAX_EFFECTS: return ctx._error("invalid_argument", "audio effect limit reached")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, ["audio_bus:" + str(args.get("bus", "")),path], "dry-run")
    AudioServer.add_bus_effect(bus, effect, int(args.get("position", -1)))
    var persisted = _persist(ctx)
    if not persisted.is_empty(): return persisted
    ctx._revision += 1
    return ctx._mutation_result(true, ["audio_bus:" + str(args.get("bus", "")),path], "Add audio bus effect")

static func effect_remove(ctx, args: Dictionary) -> Dictionary:
    var conflict = ctx._check_expect(args)
    if not conflict.is_empty(): return conflict
    var bus = AudioServer.get_bus_index(str(args.get("bus", "")))
    var effect_index = int(args.get("index", -1))
    if bus < 0 or effect_index < 0 or effect_index >= AudioServer.get_bus_effect_count(bus):
        return ctx._error("not_found", "audio bus effect not found")
    if bool(args.get("dry_run", false)):
        return ctx._mutation_result(false, ["audio_bus:" + str(args.get("bus", ""))], "dry-run")
    AudioServer.remove_bus_effect(bus, effect_index)
    var persisted = _persist(ctx)
    if not persisted.is_empty(): return persisted
    ctx._revision += 1
    return ctx._mutation_result(true, ["audio_bus:" + str(args.get("bus", ""))], "Remove audio bus effect")

static func _persist(ctx) -> Dictionary:
    var layout = AudioServer.generate_bus_layout()
    if ResourceSaver.save(layout, BUS_LAYOUT_PATH) != OK:
        return ctx._error("backend_failed", "failed to save AudioBusLayout")
    ProjectSettings.set_setting("audio/buses/default_bus_layout", BUS_LAYOUT_PATH)
    if ProjectSettings.save() != OK:
        return ctx._error("backend_failed", "failed to persist audio bus setting")
    EditorInterface.get_resource_filesystem().update_file(BUS_LAYOUT_PATH)
    return {}

static func _is_player(node) -> bool:
    return node is AudioStreamPlayer or node is AudioStreamPlayer2D or node is AudioStreamPlayer3D

static func _has_property(object: Object, name: String) -> bool:
    for item in object.get_property_list():
        if str(item.get("name", "")) == name:
            return true
    return false
