use semwright_audio_domain::{Error, Result};

pub const ADAPTER_VERSION: u32 = 1;
pub const RESULT_PREFIX: &str = "SEMWRIGHT_AUDIO_JSON:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeMutation {
    StemCreate {
        channels: u16,
        name: String,
    },
    BusCreate {
        channels: u16,
        name: String,
    },
    MasterCreate {
        channels: u16,
    },
    RouteRemove {
        route_id: String,
    },
    RouteRename {
        route_id: String,
        name: String,
    },
    RouteMute {
        route_id: String,
        value: bool,
    },
    RouteSolo {
        route_id: String,
        value: bool,
    },
    RouteGain {
        route_id: String,
        gain_millidb: i32,
    },
    RoutePan {
        route_id: String,
        pan_milli: i16,
    },
    ClipMove {
        region_id: String,
        start: u64,
    },
    ClipTrim {
        region_id: String,
        source_start: u64,
        length: u64,
    },
    ClipRemove {
        region_id: String,
    },
    ClipSplit {
        region_id: String,
        frame: u64,
    },
    SendCreate {
        source_route: String,
        target_route: String,
        pre_fader: bool,
    },
    SendGain {
        source_route: String,
        target_route: String,
        gain_millidb: i32,
    },
    SendRemove {
        source_route: String,
        target_route: String,
    },
    GroupCreate {
        name: String,
        route_id: String,
    },
    GroupAdd {
        group_id: String,
        route_id: String,
    },
    GroupRemove {
        group_id: String,
        route_id: String,
    },
    GroupDelete {
        group_id: String,
    },
    PluginInsert {
        route_id: String,
        plugin_name: String,
        plugin_type: String,
        preset: String,
    },
    PluginRemove {
        route_id: String,
        plugin_id: String,
    },
    PluginParamSet {
        route_id: String,
        plugin_id: String,
        parameter_index: u32,
        value_microunits: i64,
    },
    PluginAutomationPoint {
        route_id: String,
        plugin_id: String,
        parameter_index: u32,
        frame: u64,
        value_microunits: i64,
    },
    SessionRange {
        start: u64,
        end: u64,
    },
    SaveAs {
        state: String,
    },
}

impl NativeMutation {
    pub fn argv(&self) -> Result<Vec<String>> {
        match self {
            Self::StemCreate { channels, name } => {
                validate_name(name)?;
                if !(1..=64).contains(channels) {
                    return Err(Error::invalid("Invalid Ardour track channel count"));
                }
                Ok(vec![
                    "stem_create".into(),
                    channels.to_string(),
                    name.clone(),
                ])
            }
            Self::BusCreate { channels, name } => {
                validate_name(name)?;
                if !(1..=64).contains(channels) {
                    return Err(Error::invalid("Invalid Ardour bus channel count"));
                }
                Ok(vec![
                    "bus_create".into(),
                    channels.to_string(),
                    name.clone(),
                ])
            }
            Self::MasterCreate { channels } => {
                if !(1..=64).contains(channels) {
                    return Err(Error::invalid("Invalid Ardour master channel count"));
                }
                Ok(vec!["master_create".into(), channels.to_string()])
            }
            Self::RouteRemove { route_id } => {
                validate_id(route_id)?;
                Ok(vec!["route_remove".into(), route_id.clone()])
            }
            Self::RouteRename { route_id, name } => {
                validate_id(route_id)?;
                validate_name(name)?;
                Ok(vec!["route_rename".into(), route_id.clone(), name.clone()])
            }
            Self::RouteMute { route_id, value } => bool_command("route_mute", route_id, *value),
            Self::RouteSolo { route_id, value } => bool_command("route_solo", route_id, *value),
            Self::RouteGain {
                route_id,
                gain_millidb,
            } => {
                validate_id(route_id)?;
                if !(-120_000..=24_000).contains(gain_millidb) {
                    return Err(Error::invalid(
                        "Ardour route gain is outside semantic bounds",
                    ));
                }
                Ok(vec![
                    "route_gain".into(),
                    route_id.clone(),
                    gain_millidb.to_string(),
                ])
            }
            Self::RoutePan {
                route_id,
                pan_milli,
            } => {
                validate_id(route_id)?;
                if !(-1000..=1000).contains(pan_milli) {
                    return Err(Error::invalid(
                        "Ardour route pan is outside semantic bounds",
                    ));
                }
                Ok(vec![
                    "route_pan".into(),
                    route_id.clone(),
                    pan_milli.to_string(),
                ])
            }
            Self::ClipMove { region_id, start } => {
                validate_id(region_id)?;
                Ok(vec![
                    "clip_move".into(),
                    region_id.clone(),
                    start.to_string(),
                ])
            }
            Self::ClipTrim {
                region_id,
                source_start,
                length,
            } => {
                validate_id(region_id)?;
                if *length == 0 {
                    return Err(Error::invalid("Ardour clip trim length must be non-zero"));
                }
                source_start
                    .checked_add(*length)
                    .ok_or_else(|| Error::limit("Ardour clip trim overflows sample position"))?;
                Ok(vec![
                    "clip_trim".into(),
                    region_id.clone(),
                    source_start.to_string(),
                    length.to_string(),
                ])
            }
            Self::ClipRemove { region_id } => {
                validate_id(region_id)?;
                Ok(vec!["clip_remove".into(), region_id.clone()])
            }
            Self::ClipSplit { region_id, frame } => {
                validate_id(region_id)?;
                Ok(vec![
                    "clip_split".into(),
                    region_id.clone(),
                    frame.to_string(),
                ])
            }
            Self::SendCreate {
                source_route,
                target_route,
                pre_fader,
            } => {
                validate_id(source_route)?;
                validate_id(target_route)?;
                if source_route == target_route {
                    return Err(Error::invalid("Ardour send source and target must differ"));
                }
                Ok(vec![
                    "send_create".into(),
                    source_route.clone(),
                    target_route.clone(),
                    if *pre_fader { "1" } else { "0" }.into(),
                ])
            }
            Self::SendGain {
                source_route,
                target_route,
                gain_millidb,
            } => {
                validate_id(source_route)?;
                validate_id(target_route)?;
                if !(-120_000..=24_000).contains(gain_millidb) {
                    return Err(Error::invalid("Ardour send gain exceeds semantic bounds"));
                }
                Ok(vec![
                    "send_gain".into(),
                    source_route.clone(),
                    target_route.clone(),
                    gain_millidb.to_string(),
                ])
            }
            Self::SendRemove {
                source_route,
                target_route,
            } => {
                validate_id(source_route)?;
                validate_id(target_route)?;
                Ok(vec![
                    "send_remove".into(),
                    source_route.clone(),
                    target_route.clone(),
                ])
            }
            Self::GroupCreate { name, route_id } => {
                validate_name(name)?;
                validate_id(route_id)?;
                Ok(vec!["group_create".into(), name.clone(), route_id.clone()])
            }
            Self::GroupAdd { group_id, route_id } => {
                validate_id(group_id)?;
                validate_id(route_id)?;
                Ok(vec!["group_add".into(), group_id.clone(), route_id.clone()])
            }
            Self::GroupRemove { group_id, route_id } => {
                validate_id(group_id)?;
                validate_id(route_id)?;
                Ok(vec![
                    "group_remove".into(),
                    group_id.clone(),
                    route_id.clone(),
                ])
            }
            Self::GroupDelete { group_id } => {
                validate_id(group_id)?;
                Ok(vec!["group_delete".into(), group_id.clone()])
            }
            Self::PluginInsert {
                route_id,
                plugin_name,
                plugin_type,
                preset,
            } => {
                validate_id(route_id)?;
                validate_name(plugin_name)?;
                if !matches!(plugin_type.as_str(), "lua" | "lv2") {
                    return Err(Error::invalid("Unsupported Ardour plugin allowlist type"));
                }
                if preset.len() > 1024 || preset.chars().any(char::is_control) {
                    return Err(Error::invalid("Invalid Ardour plugin preset"));
                }
                Ok(vec![
                    "plugin_insert".into(),
                    route_id.clone(),
                    plugin_name.clone(),
                    plugin_type.clone(),
                    preset.clone(),
                ])
            }
            Self::PluginRemove {
                route_id,
                plugin_id,
            } => {
                validate_id(route_id)?;
                validate_id(plugin_id)?;
                Ok(vec![
                    "plugin_remove".into(),
                    route_id.clone(),
                    plugin_id.clone(),
                ])
            }
            Self::PluginParamSet {
                route_id,
                plugin_id,
                parameter_index,
                value_microunits,
            } => {
                validate_id(route_id)?;
                validate_id(plugin_id)?;
                if *parameter_index > 4095 || value_microunits.abs() > 1_000_000_000_000_000 {
                    return Err(Error::invalid("Invalid Ardour plugin parameter mutation"));
                }
                Ok(vec![
                    "plugin_param_set".into(),
                    route_id.clone(),
                    plugin_id.clone(),
                    parameter_index.to_string(),
                    value_microunits.to_string(),
                ])
            }
            Self::PluginAutomationPoint {
                route_id,
                plugin_id,
                parameter_index,
                frame,
                value_microunits,
            } => {
                validate_id(route_id)?;
                validate_id(plugin_id)?;
                if *parameter_index > 4095
                    || *frame > i64::MAX as u64
                    || value_microunits.abs() > 1_000_000_000_000_000
                {
                    return Err(Error::invalid("Invalid Ardour plugin automation point"));
                }
                Ok(vec![
                    "plugin_automation_point".into(),
                    route_id.clone(),
                    plugin_id.clone(),
                    parameter_index.to_string(),
                    frame.to_string(),
                    value_microunits.to_string(),
                ])
            }
            Self::SessionRange { start, end } => {
                if end <= start || *end > i64::MAX as u64 {
                    return Err(Error::invalid("Invalid Ardour session range"));
                }
                Ok(vec![
                    "session_range".into(),
                    start.to_string(),
                    end.to_string(),
                ])
            }
            Self::SaveAs { state } => {
                validate_id(state)?;
                Ok(vec!["save_as".into(), state.clone()])
            }
        }
    }
}

fn bool_command(command: &str, route_id: &str, value: bool) -> Result<Vec<String>> {
    validate_id(route_id)?;
    Ok(vec![
        command.into(),
        route_id.into(),
        if value { "1" } else { "0" }.into(),
    ])
}

fn validate_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(Error::invalid("Invalid Ardour native identity"));
    }
    Ok(())
}

fn validate_name(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(Error::invalid("Invalid Ardour route name"));
    }
    Ok(())
}

/// Fixed Semwright-owned adapter. User/agent values are never interpolated into
/// this source: all runtime values arrive through Lua positional arguments.
pub const LUA_ADAPTER: &str = r#"
local JSON_PREFIX = "SEMWRIGHT_AUDIO_JSON:"

local function esc(s)
  s = tostring(s or "")
  return (s:gsub('[%z\1-\31\\"]', function(c)
    local b = string.byte(c)
    if c == "\\" then return "\\\\" end
    if c == '"' then return '\\"' end
    return string.format("\\u%04x", b)
  end))
end

local function q(s) return '"' .. esc(s) .. '"' end
local function bool(v) return v and "true" or "false" end
local function arr(values) return "[" .. table.concat(values, ",") .. "]" end
local function obj(fields) return "{" .. table.concat(fields, ",") .. "}" end
local function field(k, v) return q(k) .. ":" .. v end

local function samples(v)
  local ok, n = pcall(function() return v:samples() end)
  if ok then return tonumber(n) or 0 end
  return tonumber(v) or 0
end

local function db_milli_from_coeff(v)
  v = tonumber(v) or 0
  if v <= 0 then return -120000 end
  local db = 20.0 * math.log(v) / math.log(10.0)
  local milli = math.floor(db * 1000.0 + (db >= 0 and 0.5 or -0.5))
  if milli < -120000 then milli = -120000 end
  if milli > 24000 then milli = 24000 end
  return milli
end

local function micro(v)
  v = tonumber(v)
  if not v or v ~= v or v > 1000000000.0 or v < -1000000000.0 then
    error("native numeric value is outside bounded range")
  end
  local scaled = v * 1000000.0
  return math.floor(scaled + (scaled >= 0 and 0.5 or -0.5))
end

local function object_id(value)
  local stateful = value:to_stateful()
  if not stateful or stateful:isnil() then error("native object has no stable stateful identity") end
  return stateful:id():to_s()
end

local function route_id(route) return object_id(route) end

local function find_route(id)
  for route in Session:get_routes():iter() do
    if route_id(route) == id then return route end
  end
  return nil
end

local function route_track(route)
  local track = route:to_track()
  if track and not track:isnil() then return track end
  return nil
end

local function find_region(id)
  for route in Session:get_routes():iter() do
    local track = route_track(route)
    if track then
      local playlist = track:playlist()
      if playlist and not playlist:isnil() then
        for region in playlist:region_list():iter() do
          if object_id(region) == id then return region, playlist end
        end
      end
    end
  end
  return nil, nil
end
"#;
const _: () = ();
pub const LUA_ADAPTER_BODY: &str = r#"
local function inspect_region(region, route_channels)
  local source = region:source(0)
  local source_id = object_id(source)
  local source_name = source:name()
  local source_frames = samples(source:length())
  local source_path = "null"
  local ok_fs, fs = pcall(function() return source:to_filesource() end)
  if ok_fs and fs and not fs:isnil() then
    local ok_within, within = pcall(function() return fs:within_session() end)
    if ok_within and within then
      local ok_path, path = pcall(function() return fs:path() end)
      if ok_path and path and #path > 0 then
        local root = Session:path()
        if string.sub(path, 1, #root) == root then
          local rel = string.sub(path, #root + 1)
          rel = string.gsub(rel, "^/+", "")
          if #rel > 0 then source_path = q(rel) end
        end
      end
    end
  end

  local locked = false
  local ok_locked, value_locked = pcall(function() return region:locked() end)
  if ok_locked then locked = value_locked and true or false end

  return obj({
    field("id", q(object_id(region))),
    field("name", q(region:name())),
    field("position", tostring(samples(region:position()))),
    field("source_start", tostring(samples(region:start()))),
    field("length", tostring(samples(region:length()))),
    field("source_id", q(source_id)),
    field("source_name", q(source_name)),
    field("source_path", source_path),
    field("source_frames", tostring(source_frames)),
    field("source_channels", tostring(route_channels)),
    field("locked", bool(locked))
  })
end

local function route_kind(route)
  local ok_master, master = pcall(function() return route:is_master() end)
  if ok_master and master then return "master" end
  if route_track(route) then return "track" end
  return "bus"
end

local function route_channels(route)
  local ok, count = pcall(function() return route:n_inputs():n_audio() end)
  if ok and tonumber(count) and tonumber(count) > 0 then return tonumber(count) end
  local ok_out, out_count = pcall(function() return route:n_outputs():n_audio() end)
  if ok_out and tonumber(out_count) and tonumber(out_count) > 0 then return tonumber(out_count) end
  return 2
end

local function route_pan_milli(route)
  local ok, control = pcall(function() return route:pan_azimuth_control() end)
  if not ok or not control or control:isnil() then return 0 end
  local ok_value, value = pcall(function() return control:get_value() end)
  if not ok_value then return 0 end
  value = tonumber(value) or 0.5
  local p = math.floor(((value * 2.0) - 1.0) * 1000.0 + 0.5)
  if p < -1000 then p = -1000 end
  if p > 1000 then p = 1000 end
  return p
end

local function inspect_plugin_parameters(processor, plugin)
  local parameters = {}
  local count = tonumber(plugin:parameter_count()) or 0
  if count < 0 or count > 4096 then return parameters, false end
  for index = 0, count - 1 do
    local ok_auto, automation_list, control_list, descriptor =
      pcall(function() return ARDOUR.LuaAPI.plugin_automation(processor, index) end)
    if not ok_auto or not automation_list or automation_list:isnil()
      or not control_list or control_list:isnil() or not descriptor then
      return parameters, false
    end
    local ok_value, value, valid_value =
      pcall(function() return ARDOUR.LuaAPI.get_processor_param(processor, index) end)
    if not ok_value or valid_value == false then return parameters, false end
    local ok_nth, control_id, nth_ok =
      pcall(function() return plugin:nth_parameter(index, false) end)
    if not ok_nth or nth_ok == false then return parameters, false end
    local label = plugin:parameter_label(control_id)
    if not label or #tostring(label) == 0 then label = "parameter-" .. tostring(index) end
    table.insert(parameters, obj({
      field("index", tostring(index)),
      field("label", q(label)),
      field("value_microunits", tostring(micro(value))),
      field("lower_microunits", tostring(micro(descriptor.lower))),
      field("upper_microunits", tostring(micro(descriptor.upper))),
      field("normal_microunits", tostring(micro(descriptor.normal))),
      field("automation_points", tostring(tonumber(control_list:size()) or 0))
    }))
  end
  return parameters, true
end

local function inspect_plugins(route)
  local plugins = {}
  for index = 0, 255 do
    local processor = route:nth_plugin(index)
    if not processor or processor:isnil() then return plugins, true end
    local insert = processor:to_plugininsert()
    if not insert or insert:isnil() then return plugins, false end
    local plugin = insert:plugin(0)
    if not plugin or plugin:isnil() then return plugins, false end
    local parameters, parameters_complete = inspect_plugin_parameters(processor, plugin)
    local unique_id = plugin:unique_id()
    local unique_json = "null"
    if unique_id and #tostring(unique_id) > 0 then unique_json = q(unique_id) end
    table.insert(plugins, obj({
      field("id", q(object_id(processor))),
      field("name", q(plugin:name())),
      field("unique_id", unique_json),
      field("enabled", bool(insert:enabled())),
      field("latency_samples", tostring(tonumber(insert:signal_latency()) or 0)),
      field("parameters", arr(parameters)),
      field("parameters_complete", bool(parameters_complete))
    }))
  end
  return plugins, false
end

local function processor_index(route, wanted)
  for index = 0, 511 do
    local processor = route:nth_processor(index)
    if not processor or processor:isnil() then return nil, true end
    if object_id(processor) == wanted then return index, true end
  end
  return nil, false
end

local function inspect_sends(route)
  local sends = {}
  local amp = route:amp()
  if not amp or amp:isnil() then return sends, false end
  local amp_index, bounded = processor_index(route, object_id(amp))
  if not bounded or amp_index == nil then return sends, false end

  for index = 0, 255 do
    local processor = route:nth_send(index)
    if not processor or processor:isnil() then return sends, true end
    local internal = processor:to_internalsend()
    if not internal or internal:isnil() then return sends, false end
    local target = internal:target_route()
    if not target or target:isnil() then return sends, false end
    local send_index, complete = processor_index(route, object_id(processor))
    if not complete or send_index == nil then return sends, false end
    local level = route:send_level_controllable(index)
    if not level or level:isnil() then return sends, false end
    table.insert(sends, obj({
      field("target_route", q(route_id(target))),
      field("gain_millidb", tostring(db_milli_from_coeff(level:get_value()))),
      field("enabled", bool(internal:active())),
      field("pre_fader", bool(send_index < amp_index))
    }))
  end
  return sends, false
end

local function inspect_route(route)
  local channels = route_channels(route)
  local regions = {}
  local plugins, plugins_complete = inspect_plugins(route)
  local sends, sends_complete = inspect_sends(route)
  local track = route_track(route)
  if track then
    local playlist = track:playlist()
    if playlist and not playlist:isnil() then
      for region in playlist:region_list():iter() do
        table.insert(regions, inspect_region(region, channels))
      end
    end
  end

  return obj({
    field("id", q(route_id(route))),
    field("name", q(route:name())),
    field("kind", q(route_kind(route))),
    field("channels", tostring(channels)),
    field("muted", bool(route:mute_control():get_value() >= 0.5)),
    field("soloed", bool(route:solo_control():get_value() >= 0.5)),
    field("gain_millidb", tostring(db_milli_from_coeff(route:gain_control():get_value()))),
    field("pan_milli", tostring(route_pan_milli(route))),
    field("regions", arr(regions)),
    field("sends", arr(sends)),
    field("plugins", arr(plugins)),
    field("routing_complete", "false"),
    field("sends_complete", bool(sends_complete)),
    field("plugins_complete", bool(plugins_complete))
  })
end

local function inspect_groups()
  local groups = {}
  local count = 0
  for group in Session:route_groups():iter() do
    count = count + 1
    if count > 1024 then error("route group budget exceeded") end
    local routes = {}
    for route in group:route_list():iter() do
      table.insert(routes, q(route_id(route)))
    end
    table.insert(groups, obj({
      field("id", q(object_id(group))),
      field("name", q(group:name())),
      field("route_ids", arr(routes)),
      field("active", bool(group:is_active())),
      field("relative", bool(group:is_relative())),
      field("gain", bool(group:is_gain())),
      field("mute", bool(group:is_mute())),
      field("solo", bool(group:is_solo()))
    }))
  end
  return groups
end

local function snapshot(version)
  local routes = {}
  for route in Session:get_routes():iter() do
    table.insert(routes, inspect_route(route))
  end
  return obj({
    field("snapshot_version", "1"),
    field("ardour_version", q(version)),
    field("session_name", q(Session:name())),
    field("sample_rate", tostring(Session:nominal_sample_rate())),
    field("session_start", tostring(Session:current_start_sample())),
    field("session_end", tostring(Session:current_end_sample())),
    field("routes", arr(routes)),
    field("groups", arr(inspect_groups())),
    field("warnings", "[]")
  })
end
"#;
pub const LUA_ADAPTER_MUTATION_BODY: &str = r#"
local function no_group()
  return PBD.GroupControlDisposition.NoGroup
end

local function require_route(id)
  local route = find_route(id)
  if not route then error("route not found") end
  return route
end

local function require_region(id)
  local region, playlist = find_region(id)
  if not region then error("region not found") end
  return region, playlist
end

local function require_group(id)
  for group in Session:route_groups():iter() do
    if object_id(group) == id then return group end
  end
  error("route group not found")
end

local function require_plugin(route, id)
  for index = 0, 255 do
    local processor = route:nth_plugin(index)
    if not processor or processor:isnil() then break end
    if object_id(processor) == id then
      local insert = processor:to_plugininsert()
      if not insert or insert:isnil() then error("plugin insert cast failed") end
      return processor, insert
    end
  end
  error("plugin not found")
end

local function require_send(source, target_id)
  local found = nil
  local found_index = nil
  for index = 0, 255 do
    local processor = source:nth_send(index)
    if not processor or processor:isnil() then break end
    local internal = processor:to_internalsend()
    if internal and not internal:isnil() then
      local target = internal:target_route()
      if target and not target:isnil() and route_id(target) == target_id then
        if found then error("multiple sends to target are ambiguous") end
        found = processor
        found_index = index
      end
    end
  end
  if not found or found_index == nil then error("send not found") end
  return found, found:to_internalsend(), found_index
end

local function mutate(command)
  if command == "inspect" then return end

  if command == "stem_create" then
    local channels = tonumber(arg[5])
    local name = arg[6]
    if not channels or channels < 1 or channels > 64 then error("invalid channels") end
    local created = Session:new_audio_track(
      channels,
      channels,
      nil,
      1,
      "",
      ARDOUR.PresentationInfo.max_order,
      ARDOUR.TrackMode.Normal,
      true,
      true
    )
    local renamed = false
    for route in created:iter() do
      route:set_name(name)
      renamed = true
    end
    if not renamed then error("track create returned no route") end
  elseif command == "bus_create" then
    local channels = tonumber(arg[5])
    local name = arg[6]
    if not channels or channels < 1 or channels > 64 then error("invalid channels") end
    local created = Session:new_audio_route(
      channels,
      channels,
      nil,
      1,
      "",
      ARDOUR.PresentationInfo.Flag.AudioBus,
      ARDOUR.PresentationInfo.max_order
    )
    local renamed = false
    for route in created:iter() do
      route:set_name(name)
      renamed = true
    end
    if not renamed then error("bus create returned no route") end
  elseif command == "master_create" then
    local channels = tonumber(arg[5])
    if not channels or channels < 1 or channels > 64 then error("invalid master channels") end
    local count = ARDOUR.ChanCount(ARDOUR.DataType("audio"), channels)
    local status = Session:add_master_bus(count)
    if status ~= 0 then error("master create failed") end
  elseif command == "route_remove" then
    local route = require_route(arg[5])
    local ok_master, master = pcall(function() return route:is_master() end)
    if ok_master and master then error("master route cannot be removed") end
    Session:remove_route(route)
  elseif command == "route_rename" then
    require_route(arg[5]):set_name(arg[6])
  elseif command == "route_mute" then
    Session:set_control(
      require_route(arg[5]):mute_control(),
      arg[6] == "1" and 1 or 0,
      no_group()
    )
  elseif command == "route_solo" then
    Session:set_control(
      require_route(arg[5]):solo_control(),
      arg[6] == "1" and 1 or 0,
      no_group()
    )
  elseif command == "route_gain" then
    local db_milli = tonumber(arg[6])
    if not db_milli then error("invalid gain") end
    local coeff = 10.0 ^ (db_milli / 20000.0)
    require_route(arg[5]):gain_control():set_value(coeff, no_group())
  elseif command == "route_pan" then
    local pan_milli = tonumber(arg[6])
    if not pan_milli then error("invalid pan") end
    local raw = (pan_milli + 1000.0) / 2000.0
    require_route(arg[5]):pan_azimuth_control():set_value(raw, no_group())
  elseif command == "clip_move" then
    local region = require_region(arg[5])
    region:set_position(Temporal.timepos_t(tonumber(arg[6])))
  elseif command == "clip_trim" then
    local region = require_region(arg[5])
    local source_start = tonumber(arg[6])
    local length = tonumber(arg[7])
    region:set_start(Temporal.timepos_t(source_start))
    region:set_length(Temporal.timecnt_t(length))
  elseif command == "clip_remove" then
    local region, playlist = require_region(arg[5])
    playlist:remove_region(region)
  elseif command == "clip_split" then
    local region, playlist = require_region(arg[5])
    local frame = tonumber(arg[6])
    if not frame then error("invalid split frame") end
    playlist:split_region(region, Temporal.timepos_t(frame))
  elseif command == "send_create" then
    local source = require_route(arg[5])
    local target = require_route(arg[6])
    if route_id(source) == route_id(target) then error("self send is not supported") end
    local tracks = ARDOUR.RouteListPtr()
    tracks:push_back(source)
    local placement = arg[7] == "1" and ARDOUR.Placement.PreFader or ARDOUR.Placement.PostFader
    Session:add_internal_sends(target, placement, tracks)
  elseif command == "send_gain" then
    local source = require_route(arg[5])
    local _, _, send_index = require_send(source, arg[6])
    local db_milli = tonumber(arg[7])
    if not db_milli then error("invalid send gain") end
    local level = source:send_level_controllable(send_index)
    if not level or level:isnil() then error("send level control is unavailable") end
    if not level:writable() then
      level:set_automation_state(ARDOUR.AutoState.Off)
    end
    if not level:writable() then error("send level control is not writable") end
    local coeff = 10.0 ^ (db_milli / 20000.0)
    local lower = tonumber(level:lower())
    local upper = tonumber(level:upper())
    if not lower or not upper or coeff < lower or coeff > upper then
      error("send gain is outside native control bounds")
    end
    level:set_value(coeff, no_group())
    local observed = tonumber(level:get_value())
    if not observed or math.abs(observed - coeff) > 0.000001 then
      error("send gain native readback mismatch")
    end
  elseif command == "send_remove" then
    local source = require_route(arg[5])
    local processor = require_send(source, arg[6])
    local status = source:remove_processor(processor, nil, true)
    if status ~= 0 then error("send removal failed") end
  elseif command == "group_create" then
    local group = Session:new_route_group(arg[5])
    if not group then error("route group create failed") end
    local status = group:add(require_route(arg[6]))
    if status ~= 0 then error("route group member add failed") end
  elseif command == "group_add" then
    local status = require_group(arg[5]):add(require_route(arg[6]))
    if status ~= 0 then error("route group member add failed") end
  elseif command == "group_remove" then
    local status = require_group(arg[5]):remove(require_route(arg[6]))
    if status ~= 0 then error("route group member remove failed") end
  elseif command == "group_delete" then
    Session:remove_route_group(require_group(arg[5]))
  elseif command == "plugin_insert" then
    local route = require_route(arg[5])
    local plugin_type
    if arg[7] == "lua" then plugin_type = ARDOUR.PluginType.Lua
    elseif arg[7] == "lv2" then plugin_type = ARDOUR.PluginType.LV2
    else error("plugin type outside fixed adapter allowlist") end
    local processor = ARDOUR.LuaAPI.new_plugin(Session, arg[6], plugin_type, arg[8] or "")
    if not processor or processor:isnil() then error("allowed plugin is unavailable") end
    local status = route:add_processor_by_index(processor, 0, nil, true)
    if status ~= 0 then error("plugin insertion failed") end
  elseif command == "plugin_remove" then
    local route = require_route(arg[5])
    local processor = require_plugin(route, arg[6])
    local status = route:remove_processor(processor, nil, true)
    if status ~= 0 then error("plugin removal failed") end
  elseif command == "plugin_param_set" then
    local route = require_route(arg[5])
    local _, insert = require_plugin(route, arg[6])
    local index = tonumber(arg[7])
    local value = tonumber(arg[8])
    if not index or not value then error("invalid plugin parameter") end
    value = value / 1000000.0
    if not ARDOUR.LuaAPI.set_plugin_insert_param(insert, index, value) then
      error("plugin parameter rejected by native bounds")
    end
  elseif command == "plugin_automation_point" then
    local route = require_route(arg[5])
    local processor = require_plugin(route, arg[6])
    local index = tonumber(arg[7])
    local frame = tonumber(arg[8])
    local value = tonumber(arg[9])
    if not index or not frame or not value then error("invalid plugin automation point") end
    value = value / 1000000.0
    local automation_list, control_list, descriptor = ARDOUR.LuaAPI.plugin_automation(processor, index)
    if not automation_list or automation_list:isnil() or not control_list or control_list:isnil() then
      error("plugin automation is unavailable")
    end
    if value < descriptor.lower or value > descriptor.upper then
      error("plugin automation value outside native bounds")
    end
    Session:begin_reversible_command("Semwright plugin automation")
    local before = automation_list:get_state()
    control_list:add(Temporal.timepos_t(frame), value, false, true)
    local after = automation_list:get_state()
    Session:add_command(automation_list:memento_command(before, after))
    Session:commit_reversible_command(nil)
  elseif command == "session_range" then
    local start_sample = tonumber(arg[5])
    local end_sample = tonumber(arg[6])
    if not start_sample or not end_sample or end_sample <= start_sample then
      error("invalid session range")
    end
    local locations = Session:locations()
    local session_range = locations:session_range_location()
    if not session_range then error("session range is unavailable") end
    local range_status = session_range:set(
      Temporal.timepos_t(start_sample),
      Temporal.timepos_t(end_sample)
    )
    if range_status ~= 0 then error("Ardour session range update failed") end
  elseif command == "save_as" then
    local state = arg[5]
    local status = Session:save_state(state)
    if status ~= 0 then error("Ardour save-as failed") end
    return
  else
    error("unsupported Semwright Ardour command")
  end

  local status = Session:save_state("")
  if status ~= 0 then error("Ardour save_state failed") end
end

local session_dir = arg[1]
local session_state = arg[2]
local ardour_version = arg[3]
local command = arg[4] or "inspect"

load_session(session_dir, session_state)
if not Session then error("Ardour session failed to load") end
mutate(command)
print(JSON_PREFIX .. snapshot(ardour_version))
close_session()
"#;

pub fn source() -> String {
    [LUA_ADAPTER, LUA_ADAPTER_BODY, LUA_ADAPTER_MUTATION_BODY].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_argv_not_lua_source() {
        let hostile = "x'); os.execute('touch /tmp/pwned'); --";
        let argv = NativeMutation::RouteRename {
            route_id: "route1".into(),
            name: hostile.into(),
        }
        .argv()
        .unwrap();
        assert_eq!(argv[2], hostile);
        assert!(!source().contains(hostile));
        assert!(!source().contains("os.execute"));
    }

    #[test]
    fn route_group_mutations_use_native_defaults_and_check_status() {
        assert!(!source().contains("group:set_active("));
        assert!(source().contains("local status = group:add(require_route(arg[6]))"));
        assert!(
            source().contains("if status ~= 0 then error(\"route group member add failed\") end")
        );
        assert!(
            source()
                .contains("if status ~= 0 then error(\"route group member remove failed\") end")
        );
    }

    #[test]
    fn master_create_is_fixed_and_bounded() {
        assert_eq!(
            NativeMutation::MasterCreate { channels: 2 }.argv().unwrap(),
            vec!["master_create".to_string(), "2".to_string()]
        );
        assert!(NativeMutation::MasterCreate { channels: 0 }.argv().is_err());
        assert!(
            NativeMutation::MasterCreate { channels: 65 }
                .argv()
                .is_err()
        );
    }

    #[test]
    fn mutation_arguments_are_bounded() {
        assert!(
            NativeMutation::RouteRemove {
                route_id: "../bad".into()
            }
            .argv()
            .is_err()
        );
        assert!(
            NativeMutation::RoutePan {
                route_id: "r1".into(),
                pan_milli: 1001
            }
            .argv()
            .is_err()
        );
        assert!(
            NativeMutation::ClipTrim {
                region_id: "c1".into(),
                source_start: u64::MAX,
                length: 2
            }
            .argv()
            .is_err()
        );
    }

    #[test]
    fn fixed_adapter_contains_no_dynamic_code_loader() {
        let script = source();
        for forbidden in [
            "loadstring(",
            "dofile(",
            "io.popen(",
            "os.execute(",
            "package.loadlib(",
        ] {
            assert!(!script.contains(forbidden), "{forbidden}");
        }
        assert!(script.contains("load_session(session_dir, session_state)"));
        assert!(script.contains("Session:save_state"));
    }
}
