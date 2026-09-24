use jsonschema::Validator;
use semwright_driver_sdk::{Capability, descriptor_digest};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Local,
    Plugin,
    Runner,
}

pub struct Entry {
    pub capability: Capability,
    pub digest: String,
    pub route: Route,
    pub mutation: bool,
    input: Validator,
    output: Validator,
}

pub struct Catalog {
    entries: BTreeMap<String, Entry>,
}

impl Catalog {
    pub fn load() -> Result<Self> {
        let mut entries = BTreeMap::new();
        for spec in specs() {
            let descriptor = CommandDescriptor {
                name: format!("driver.godot.{}", spec.name),
                version: "1".into(),
                description: spec.description.into(),
                input_schema: (spec.input)(),
                output_schema: (spec.output)(),
                requires: vec!["driver:godot".into()],
                risk: spec.risk,
                idempotency: spec.idempotency,
                timeout_ms: spec.timeout_ms,
                dry_run: spec.dry_run,
                interactive_consent: spec.risk.sensitive(),
                backends: vec!["driver:godot".into()],
            };
            let capability = Capability {
                descriptor,
                aliases: vec![],
                tags: vec!["godot".into(), spec.tag.into()],
                object_types: vec![spec.object_type.into()],
            };
            let digest = descriptor_digest(&capability.descriptor)?;
            let input = jsonschema::validator_for(&capability.descriptor.input_schema)
                .map_err(|_| Error::new(ErrorCode::Internal, "Invalid Godot input schema"))?;
            let output = jsonschema::validator_for(&capability.descriptor.output_schema)
                .map_err(|_| Error::new(ErrorCode::Internal, "Invalid Godot output schema"))?;
            let name = capability.descriptor.name.clone();
            let entry = Entry {
                capability,
                digest,
                route: spec.route,
                mutation: spec.mutation,
                input,
                output,
            };
            if entries.insert(name, entry).is_some() {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "Duplicate Godot capability",
                ));
            }
        }
        Ok(Self { entries })
    }

    pub fn capabilities(&self) -> Vec<Capability> {
        self.capabilities_for(true)
    }

    pub fn capabilities_for(&self, include_runner: bool) -> Vec<Capability> {
        self.entries
            .values()
            .filter(|entry| include_runner || entry.route != Route::Runner)
            .map(|entry| entry.capability.clone())
            .collect()
    }

    pub fn names_for(&self, route: Route) -> Vec<String> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.route == route)
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub fn get(&self, name: &str) -> Result<&Entry> {
        self.entries
            .get(name)
            .ok_or_else(|| Error::new(ErrorCode::Unsupported, "Unsupported Godot capability"))
    }
}

impl Entry {
    pub fn validate_input(&self, value: &Value) -> Result<()> {
        if let Some(error) = self.input.iter_errors(value).next() {
            return Err(Error::invalid(format!(
                "Godot capability input violates schema at {}: {:?}",
                error.instance_path, error.kind
            )));
        }
        Ok(())
    }
    pub fn validate_output(&self, value: &Value) -> Result<()> {
        if let Some(error) = self.output.iter_errors(value).next() {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                format!(
                    "Godot plugin output violates schema at {}: {:?}",
                    error.instance_path, error.kind
                ),
            ));
        }
        Ok(())
    }
}

struct Spec {
    name: &'static str,
    description: &'static str,
    tag: &'static str,
    object_type: &'static str,
    route: Route,
    risk: Risk,
    idempotency: Idempotency,
    timeout_ms: u64,
    dry_run: bool,
    mutation: bool,
    input: fn() -> Value,
    output: fn() -> Value,
}

fn specs() -> Vec<Spec> {
    use Idempotency::{NonIdempotent, ReadOnly};
    use Risk::{CodeExecution, Destructive, Mutating, MutatingReversible, ReadOnly as R};
    vec![
        spec(
            "doctor",
            "Inspect Godot driver health",
            "diagnostics",
            "driver",
            Route::Local,
            R,
            ReadOnly,
            5_000,
            false,
            false,
            empty,
            doctor_out,
        ),
        spec(
            "session.list",
            "List authenticated Godot editor sessions",
            "session",
            "session",
            Route::Local,
            R,
            ReadOnly,
            5_000,
            false,
            false,
            empty,
            sessions_out,
        ),
        spec(
            "project.inspect",
            "Inspect the current Godot project and editor state",
            "project",
            "project",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            session_in,
            project_out,
        ),
        spec(
            "project.files",
            "List bounded project resource files",
            "project",
            "file",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            session_in,
            files_out,
        ),
        spec(
            "scene.inspect",
            "Inspect the current scene graph",
            "scene",
            "scene",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            session_in,
            scene_out,
        ),
        spec(
            "scene.create",
            "Create and save a new scene",
            "scene",
            "scene",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            scene_create_in,
            mutation_out,
        ),
        spec(
            "scene.open",
            "Open an existing scene",
            "scene",
            "scene",
            Route::Plugin,
            CodeExecution,
            NonIdempotent,
            20_000,
            true,
            true,
            path_mutation_in,
            mutation_out,
        ),
        spec(
            "scene.save",
            "Save the current scene",
            "scene",
            "scene",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            mutation_in,
            mutation_out,
        ),
        spec(
            "node.inspect",
            "Inspect a node by semantic path",
            "node",
            "node",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            node_inspect_in,
            node_out,
        ),
        spec(
            "node.create",
            "Create a typed node under a parent path",
            "node",
            "node",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            node_create_in,
            mutation_out,
        ),
        spec(
            "node.patch",
            "Patch allowlisted node properties",
            "node",
            "node",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            node_patch_in,
            mutation_out,
        ),
        spec(
            "node.remove",
            "Remove a node from the scene",
            "node",
            "node",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            node_target_mutation_in,
            mutation_out,
        ),
        spec(
            "input.list",
            "Inspect InputMap actions",
            "input",
            "input_action",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            session_in,
            input_out,
        ),
        spec(
            "input.set",
            "Create or replace an InputMap action",
            "input",
            "input_action",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            input_set_in,
            mutation_out,
        ),
        spec(
            "scene.reload",
            "Reload the current saved scene",
            "scene",
            "scene",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            mutation_in,
            mutation_out,
        ),
        spec(
            "scene.instantiate",
            "Instantiate a PackedScene under a node",
            "scene",
            "scene_instance",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            scene_instance_in,
            mutation_out,
        ),
        spec(
            "node.rename",
            "Rename a scene node",
            "node",
            "node",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            10_000,
            true,
            true,
            node_rename_in,
            mutation_out,
        ),
        spec(
            "node.reparent",
            "Reparent a scene node",
            "node",
            "node",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            node_reparent_in,
            mutation_out,
        ),
        spec(
            "group.set",
            "Set persistent node group membership",
            "node",
            "group",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            10_000,
            true,
            true,
            group_set_in,
            mutation_out,
        ),
        spec(
            "input.remove",
            "Remove a non-built-in InputMap action",
            "input",
            "input_action",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            input_remove_in,
            mutation_out,
        ),
        spec(
            "assets.status",
            "Inspect Godot resource filesystem scan/import state",
            "assets",
            "asset_index",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            session_in,
            assets_status_out,
        ),
        spec(
            "assets.rescan",
            "Request a resource filesystem rescan",
            "assets",
            "asset_index",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            30_000,
            true,
            true,
            mutation_in,
            mutation_out,
        ),
        spec(
            "project.main_scene",
            "Set the project main scene",
            "project",
            "project",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            path_mutation_in,
            mutation_out,
        ),
        spec(
            "resource.inspect",
            "Inspect a saved Godot Resource",
            "resource",
            "resource",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            path_read_in,
            resource_out,
        ),
        spec(
            "resource.create",
            "Create a typed Godot Resource",
            "resource",
            "resource",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            resource_create_in,
            mutation_out,
        ),
        spec(
            "resource.patch",
            "Patch allowlisted Resource properties",
            "resource",
            "resource",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            resource_patch_in,
            mutation_out,
        ),
        spec(
            "resource.duplicate",
            "Deep-duplicate a saved Resource",
            "resource",
            "resource",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            resource_duplicate_in,
            mutation_out,
        ),
        spec(
            "script.inspect",
            "Inspect a managed GDScript source file",
            "script",
            "script",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            path_read_in,
            script_out,
        ),
        spec(
            "script.write",
            "Create or replace a managed non-@tool GDScript",
            "script",
            "script",
            Route::Plugin,
            CodeExecution,
            NonIdempotent,
            20_000,
            true,
            true,
            script_write_in,
            mutation_out,
        ),
        spec(
            "script.attach",
            "Attach a managed GDScript to a node",
            "script",
            "script",
            Route::Plugin,
            CodeExecution,
            NonIdempotent,
            15_000,
            true,
            true,
            script_attach_in,
            mutation_out,
        ),
        spec(
            "script.detach",
            "Detach a script from a node",
            "script",
            "script",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            node_target_mutation_in,
            mutation_out,
        ),
        spec(
            "signal.list",
            "Inspect node signals and connections",
            "signal",
            "signal",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            node_target_read_in,
            signal_out,
        ),
        spec(
            "signal.connect",
            "Connect a signal between explicit node refs",
            "signal",
            "signal",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            signal_connect_in,
            mutation_out,
        ),
        spec(
            "signal.disconnect",
            "Disconnect an explicit signal connection",
            "signal",
            "signal",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            signal_connect_in,
            mutation_out,
        ),
        spec(
            "animation.inspect",
            "Inspect AnimationPlayer libraries, tracks, and keys",
            "animation",
            "animation",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            animation_player_in,
            animation_out,
        ),
        spec(
            "animation.create",
            "Create an Animation in an AnimationPlayer library",
            "animation",
            "animation",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_create_in,
            mutation_out,
        ),
        spec(
            "animation.remove",
            "Remove an Animation",
            "animation",
            "animation",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_target_in,
            mutation_out,
        ),
        spec(
            "animation.track.add",
            "Add a typed Animation track",
            "animation",
            "animation_track",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_track_add_in,
            mutation_out,
        ),
        spec(
            "animation.track.remove",
            "Remove an Animation track",
            "animation",
            "animation_track",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_track_target_in,
            mutation_out,
        ),
        spec(
            "animation.keyframe.set",
            "Insert or replace an Animation keyframe",
            "animation",
            "keyframe",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_key_set_in,
            mutation_out,
        ),
        spec(
            "animation.keyframe.remove",
            "Remove an Animation keyframe by time",
            "animation",
            "keyframe",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_key_remove_in,
            mutation_out,
        ),
        spec(
            "animation_tree.configure",
            "Configure bounded AnimationTree state",
            "animation",
            "animation_tree",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_tree_in,
            mutation_out,
        ),
        spec(
            "physics.layers",
            "Set collision layer and mask",
            "physics",
            "physics_body",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            10_000,
            true,
            true,
            physics_layers_in,
            mutation_out,
        ),
        spec(
            "ui.layout",
            "Patch bounded Control layout properties",
            "ui",
            "control",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            10_000,
            true,
            true,
            ui_layout_in,
            mutation_out,
        ),
        spec(
            "shader.write",
            "Create or replace a bounded Godot shader resource",
            "shader",
            "shader",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            20_000,
            true,
            true,
            shader_write_in,
            mutation_out,
        ),
        spec(
            "shader.attach",
            "Attach a ShaderMaterial to a supported node property",
            "shader",
            "shader_material",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            shader_attach_in,
            mutation_out,
        ),
        spec(
            "project.validate",
            "Validate a configured project with pinned Godot",
            "runtime",
            "project",
            Route::Runner,
            CodeExecution,
            NonIdempotent,
            30_000,
            false,
            true,
            runner_project_in,
            runner_out,
        ),
        spec(
            "script.validate",
            "Parse-check a GDScript with pinned Godot",
            "runtime",
            "script",
            Route::Runner,
            CodeExecution,
            NonIdempotent,
            20_000,
            false,
            true,
            runner_script_in,
            runner_out,
        ),
        spec(
            "project.run_test",
            "Run a bounded headless project or scene test",
            "runtime",
            "runtime",
            Route::Runner,
            CodeExecution,
            NonIdempotent,
            60_000,
            false,
            true,
            runner_test_in,
            runner_out,
        ),
        spec(
            "export.pack",
            "Export a bounded PCK/ZIP artifact",
            "export",
            "artifact",
            Route::Runner,
            CodeExecution,
            NonIdempotent,
            120_000,
            false,
            true,
            runner_export_in,
            runner_out,
        ),
        spec(
            "export.build",
            "Export a debug or release build using an existing preset",
            "export",
            "artifact",
            Route::Runner,
            CodeExecution,
            NonIdempotent,
            180_000,
            false,
            true,
            runner_build_in,
            runner_out,
        ),
        spec(
            "movie.capture",
            "Capture a bounded deterministic Godot movie",
            "runtime",
            "artifact",
            Route::Runner,
            CodeExecution,
            NonIdempotent,
            180_000,
            false,
            true,
            runner_movie_in,
            runner_out,
        ),
        spec(
            "snapshot.diff",
            "Compute a bounded semantic JSON diff",
            "snapshot",
            "snapshot",
            Route::Local,
            R,
            ReadOnly,
            5_000,
            false,
            false,
            diff_in,
            diff_out,
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn spec(
    name: &'static str,
    description: &'static str,
    tag: &'static str,
    object_type: &'static str,
    route: Route,
    risk: Risk,
    idempotency: Idempotency,
    timeout_ms: u64,
    dry_run: bool,
    mutation: bool,
    input: fn() -> Value,
    output: fn() -> Value,
) -> Spec {
    Spec {
        name,
        description,
        tag,
        object_type,
        route,
        risk,
        idempotency,
        timeout_ms,
        dry_run,
        mutation,
        input,
        output,
    }
}

fn object(properties: Map<String, Value>, required: &[&str]) -> Value {
    json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})
}
fn string(max: u64) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn hex_string(len: u64) -> Value {
    json!({"type":"string","pattern":format!("^[0-9a-f]{{{len}}}$")})
}
fn boolean() -> Value {
    json!({"type":"boolean"})
}
fn integer() -> Value {
    json!({"type":"integer","minimum":0})
}
fn stamp() -> Value {
    object(
        Map::from_iter([
            ("revision".into(), integer()),
            ("fingerprint".into(), hex_string(64)),
        ]),
        &["revision", "fingerprint"],
    )
}
fn session_prop() -> (String, Value) {
    ("session".into(), hex_string(32))
}
fn expect_prop() -> (String, Value) {
    ("expect".into(), stamp())
}
fn dry_prop() -> (String, Value) {
    ("dry_run".into(), boolean())
}
fn empty() -> Value {
    object(Map::new(), &[])
}
fn session_in() -> Value {
    object(Map::from_iter([session_prop()]), &["session"])
}
fn mutation_in() -> Value {
    object(
        Map::from_iter([session_prop(), expect_prop(), dry_prop()]),
        &["session", "expect", "dry_run"],
    )
}
fn path_mutation_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("path".into(), string(240)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "path", "expect", "dry_run"],
    )
}
fn scene_create_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("path".into(), string(240)),
            ("name".into(), string(96)),
            ("class".into(), string(96)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "path", "name", "class", "expect", "dry_run"],
    )
}
fn node_target_read_in() -> Value {
    object(
        Map::from_iter([session_prop(), ("target".into(), string(240))]),
        &["session", "target"],
    )
}
fn node_inspect_in() -> Value {
    object(
        Map::from_iter([session_prop(), ("path".into(), string(240))]),
        &["session", "path"],
    )
}
fn node_create_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("parent".into(), string(240)),
            ("name".into(), string(96)),
            ("class".into(), string(96)),
            ("properties".into(), property_patch_schema()),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session",
            "parent",
            "name",
            "class",
            "properties",
            "expect",
            "dry_run",
        ],
    )
}
fn node_patch_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            ("properties".into(), property_patch_schema()),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "target", "properties", "expect", "dry_run"],
    )
}
fn node_target_mutation_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "target", "expect", "dry_run"],
    )
}
fn input_set_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("name".into(), string(96)),
            (
                "deadzone".into(),
                json!({"type":"number","minimum":0.0,"maximum":1.0}),
            ),
            (
                "events".into(),
                json!({"type":"array","maxItems":32,"items":{"type":"object","additionalProperties":false,"required":["type","code"],"properties":{"type":{"enum":["key","mouse_button"]},"code":{"type":"integer","minimum":0,"maximum":1000000}}}}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "name", "deadzone", "events", "expect", "dry_run"],
    )
}
fn godot_value_schema() -> Value {
    json!({
        "oneOf": [
            {"type": "null"},
            {"type": "boolean"},
            {"type": "number"},
            {"type": "string", "maxLength": 4096},
            {
                "type": "object",
                "additionalProperties": false,
                "required": ["$type", "value"],
                "properties": {
                    "$type": {"const": "Vector2"},
                    "value": {
                        "type": "array",
                        "minItems": 2,
                        "maxItems": 2,
                        "items": {"type": "number"}
                    }
                }
            },
            {
                "type": "object",
                "additionalProperties": false,
                "required": ["$type", "value"],
                "properties": {
                    "$type": {"const": "Vector3"},
                    "value": {
                        "type": "array",
                        "minItems": 3,
                        "maxItems": 3,
                        "items": {"type": "number"}
                    }
                }
            },
            {
                "type": "object",
                "additionalProperties": false,
                "required": ["$type", "value"],
                "properties": {
                    "$type": {"const": "Color"},
                    "value": {
                        "type": "array",
                        "minItems": 3,
                        "maxItems": 4,
                        "items": {"type": "number", "minimum": 0.0, "maximum": 1.0}
                    }
                }
            },
            {
                "type": "object",
                "additionalProperties": false,
                "required": ["$type", "path"],
                "properties": {
                    "$type": {"const": "Resource"},
                    "path": {
                        "type": "string",
                        "minLength": 7,
                        "maxLength": 240,
                        "pattern": "^res://"
                    }
                }
            }
        ]
    })
}

fn property_patch_schema() -> Value {
    json!({
        "type": "array",
        "maxItems": 64,
        "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["name", "value"],
            "properties": {
                "name": {"type": "string", "minLength": 1, "maxLength": 96},
                "value": godot_value_schema()
            }
        }
    })
}
fn diff_in() -> Value {
    object(
        Map::from_iter([("before".into(), json!({})), ("after".into(), json!({}))]),
        &["before", "after"],
    )
}

fn doctor_out() -> Value {
    object(
        Map::from_iter([
            ("driver".into(), string(64)),
            ("bridge".into(), integer()),
            ("connected_sessions".into(), integer()),
            ("certification".into(), string(64)),
        ]),
        &["driver", "bridge", "connected_sessions", "certification"],
    )
}
fn sessions_out() -> Value {
    object(
        Map::from_iter([(
            "sessions".into(),
            json!({"type":"array","maxItems":8,"items":{"type":"object","additionalProperties":false,"required":["session","project","generation","plugin_version","engine_version"],"properties":{"session":{"type":"string","pattern":"^[0-9a-f]{32}$"},"project":{"type":"string","pattern":"^[0-9a-f]{64}$"},"generation":{"type":"string","pattern":"^[0-9a-f]{32}$"},"plugin_version":{"type":"string"},"engine_version":{"type":"string"}}}}),
        )]),
        &["sessions"],
    )
}
fn project_out() -> Value {
    read_out(
        json!({"type":"object","additionalProperties":false,"required":["name","engine","scene","unsaved"],"properties":{"name":{"type":"string"},"engine":{"type":"string"},"scene":{"type":"string"},"unsaved":{"type":"boolean"}}}),
    )
}
fn files_out() -> Value {
    read_out(json!({"type":"array","maxItems":4096,"items":{"type":"string","maxLength":240}}))
}
fn scene_out() -> Value {
    read_out(
        json!({"type":"object","additionalProperties":false,"required":["scene","nodes"],"properties":{"scene":{"type":"string"},"nodes":{"type":"array","maxItems":4000,"items":{"type":"object"}}}}),
    )
}
fn node_out() -> Value {
    read_out(
        json!({"type":"object","additionalProperties":false,"required":["path","name","class","properties"],"properties":{"path":{"type":"string"},"name":{"type":"string"},"class":{"type":"string"},"properties":{"type":"object"}}}),
    )
}
fn input_out() -> Value {
    read_out(json!({"type":"array","maxItems":512,"items":{"type":"object"}}))
}
fn read_out(data: Value) -> Value {
    object(
        Map::from_iter([("stamp".into(), stamp()), ("data".into(), data)]),
        &["stamp", "data"],
    )
}
fn mutation_out() -> Value {
    object(
        Map::from_iter([
            ("applied".into(), boolean()),
            ("stamp".into(), stamp()),
            (
                "affected".into(),
                json!({"type":"array","maxItems":256,"items":{"type":"string","maxLength":240}}),
            ),
            ("undo".into(), string(128)),
        ]),
        &["applied", "stamp", "affected", "undo"],
    )
}
fn diff_out() -> Value {
    object(
        Map::from_iter([
            (
                "changes".into(),
                json!({"type":"array","maxItems":512,"items":{"type":"object","additionalProperties":false,"required":["path","before","after"],"properties":{"path":{"type":"string"},"before":{},"after":{}}}}),
            ),
            ("truncated".into(), boolean()),
        ]),
        &["changes", "truncated"],
    )
}

fn path_read_in() -> Value {
    object(
        Map::from_iter([session_prop(), ("path".into(), string(240))]),
        &["session", "path"],
    )
}
fn scene_instance_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("scene".into(), string(240)),
            ("parent".into(), string(240)),
            ("name".into(), string(96)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "scene", "parent", "expect", "dry_run"],
    )
}
fn node_rename_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            ("name".into(), string(96)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "target", "name", "expect", "dry_run"],
    )
}
fn node_reparent_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            ("parent".into(), string(240)),
            ("keep_global_transform".into(), boolean()),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "target", "parent", "expect", "dry_run"],
    )
}
fn group_set_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            ("group".into(), string(96)),
            ("enabled".into(), boolean()),
            ("persistent".into(), boolean()),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session",
            "target",
            "group",
            "enabled",
            "persistent",
            "expect",
            "dry_run",
        ],
    )
}
fn input_remove_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("name".into(), string(96)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "name", "expect", "dry_run"],
    )
}
fn resource_create_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("path".into(), string(240)),
            ("class".into(), string(96)),
            ("properties".into(), property_patch_schema()),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session",
            "path",
            "class",
            "properties",
            "expect",
            "dry_run",
        ],
    )
}
fn resource_patch_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("path".into(), string(240)),
            ("properties".into(), property_patch_schema()),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "path", "properties", "expect", "dry_run"],
    )
}
fn resource_duplicate_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("source".into(), string(240)),
            ("destination".into(), string(240)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "source", "destination", "expect", "dry_run"],
    )
}
fn script_write_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("path".into(), string(240)),
            ("source".into(), json!({"type":"string","maxLength":262144})),
            (
                "expected_sha256".into(),
                json!({"type":"string","pattern":"^$|^[0-9a-f]{64}$"}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "path", "source", "expect", "dry_run"],
    )
}
fn script_attach_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            ("path".into(), string(240)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "target", "path", "expect", "dry_run"],
    )
}
fn signal_connect_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("source".into(), string(240)),
            ("target".into(), string(240)),
            ("signal".into(), string(96)),
            ("method".into(), string(96)),
            (
                "flags".into(),
                json!({"type":"integer","minimum":0,"maximum":255}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session", "source", "target", "signal", "method", "expect", "dry_run",
        ],
    )
}
fn animation_player_in() -> Value {
    object(
        Map::from_iter([session_prop(), ("player".into(), string(240))]),
        &["session", "player"],
    )
}
fn animation_create_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("player".into(), string(240)),
            ("library".into(), json!({"type":"string","maxLength":96})),
            ("animation".into(), string(96)),
            (
                "length".into(),
                json!({"type":"number","exclusiveMinimum":0.0,"maximum":3600.0}),
            ),
            (
                "loop_mode".into(),
                json!({"type":"integer","minimum":0,"maximum":2}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session",
            "player",
            "library",
            "animation",
            "length",
            "expect",
            "dry_run",
        ],
    )
}
fn animation_target_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("player".into(), string(240)),
            ("library".into(), json!({"type":"string","maxLength":96})),
            ("animation".into(), string(96)),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session",
            "player",
            "library",
            "animation",
            "expect",
            "dry_run",
        ],
    )
}
fn animation_track_add_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("player".into(), string(240)),
            ("library".into(), json!({"type":"string","maxLength":96})),
            ("animation".into(), string(96)),
            (
                "type".into(),
                json!({"enum":["value","position_3d","rotation_3d","scale_3d","blend_shape","bezier","method","audio","animation"]}),
            ),
            ("path".into(), string(240)),
            ("enabled".into(), boolean()),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session",
            "player",
            "library",
            "animation",
            "type",
            "path",
            "expect",
            "dry_run",
        ],
    )
}
fn animation_track_target_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("player".into(), string(240)),
            ("library".into(), json!({"type":"string","maxLength":96})),
            ("animation".into(), string(96)),
            (
                "track".into(),
                json!({"type":"integer","minimum":0,"maximum":4095}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session",
            "player",
            "library",
            "animation",
            "track",
            "expect",
            "dry_run",
        ],
    )
}
fn animation_key_set_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("player".into(), string(240)),
            ("library".into(), json!({"type":"string","maxLength":96})),
            ("animation".into(), string(96)),
            (
                "track".into(),
                json!({"type":"integer","minimum":0,"maximum":4095}),
            ),
            (
                "time".into(),
                json!({"type":"number","minimum":0.0,"maximum":3600.0}),
            ),
            ("value".into(), godot_value_schema()),
            (
                "transition".into(),
                json!({"type":"number","minimum":0.0,"maximum":16.0}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session",
            "player",
            "library",
            "animation",
            "track",
            "time",
            "value",
            "expect",
            "dry_run",
        ],
    )
}
fn animation_key_remove_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("player".into(), string(240)),
            ("library".into(), json!({"type":"string","maxLength":96})),
            ("animation".into(), string(96)),
            (
                "track".into(),
                json!({"type":"integer","minimum":0,"maximum":4095}),
            ),
            (
                "time".into(),
                json!({"type":"number","minimum":0.0,"maximum":3600.0}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session",
            "player",
            "library",
            "animation",
            "track",
            "time",
            "expect",
            "dry_run",
        ],
    )
}
fn animation_tree_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("tree".into(), string(240)),
            ("active".into(), boolean()),
            ("root_motion_track".into(), string(240)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "tree", "expect", "dry_run"],
    )
}
fn physics_layers_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            (
                "layer".into(),
                json!({"type":"integer","minimum":0,"maximum":4294967295u64}),
            ),
            (
                "mask".into(),
                json!({"type":"integer","minimum":0,"maximum":4294967295u64}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "target", "expect", "dry_run"],
    )
}
fn ui_layout_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            (
                "anchor_left".into(),
                json!({"type":"number","minimum":-16.0,"maximum":16.0}),
            ),
            (
                "anchor_top".into(),
                json!({"type":"number","minimum":-16.0,"maximum":16.0}),
            ),
            (
                "anchor_right".into(),
                json!({"type":"number","minimum":-16.0,"maximum":16.0}),
            ),
            (
                "anchor_bottom".into(),
                json!({"type":"number","minimum":-16.0,"maximum":16.0}),
            ),
            ("offset_left".into(), json!({"type":"number"})),
            ("offset_top".into(), json!({"type":"number"})),
            ("offset_right".into(), json!({"type":"number"})),
            ("offset_bottom".into(), json!({"type":"number"})),
            (
                "grow_horizontal".into(),
                json!({"type":"integer","minimum":0,"maximum":2}),
            ),
            (
                "grow_vertical".into(),
                json!({"type":"integer","minimum":0,"maximum":2}),
            ),
            (
                "mouse_filter".into(),
                json!({"type":"integer","minimum":0,"maximum":2}),
            ),
            ("minimum_size".into(), json!({})),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "target", "expect", "dry_run"],
    )
}
fn shader_write_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("path".into(), string(240)),
            ("code".into(), json!({"type":"string","maxLength":262144})),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "path", "code", "expect", "dry_run"],
    )
}
fn shader_attach_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            ("path".into(), string(240)),
            (
                "property".into(),
                json!({"enum":["material_override","material"]}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "target", "path", "expect", "dry_run"],
    )
}

fn assets_status_out() -> Value {
    read_out(
        json!({"type":"object","additionalProperties":false,"required":["scanning","importing","progress"],"properties":{"scanning":{"type":"boolean"},"importing":{"type":"boolean"},"progress":{"type":"number","minimum":0.0,"maximum":1.0}}}),
    )
}
fn resource_out() -> Value {
    read_out(
        json!({"type":"object","additionalProperties":false,"required":["path","class","properties"],"properties":{"path":{"type":"string"},"class":{"type":"string"},"properties":{"type":"object"}}}),
    )
}
fn script_out() -> Value {
    read_out(
        json!({"type":"object","additionalProperties":false,"required":["path","sha256","source"],"properties":{"path":{"type":"string"},"sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},"source":{"type":"string","maxLength":262144}}}),
    )
}
fn signal_out() -> Value {
    read_out(
        json!({"type":"array","maxItems":512,"items":{"type":"object","additionalProperties":false,"required":["name","connections"],"properties":{"name":{"type":"string"},"connections":{"type":"array","maxItems":512,"items":{"type":"object"}}}}}),
    )
}
fn animation_out() -> Value {
    read_out(
        json!({"type":"object","additionalProperties":false,"required":["libraries"],"properties":{"libraries":{"type":"array","maxItems":128,"items":{"type":"object"}}}}),
    )
}

fn runner_project_in() -> Value {
    object(
        Map::from_iter([("project".into(), hex_string(64))]),
        &["project"],
    )
}
fn runner_script_in() -> Value {
    object(
        Map::from_iter([
            ("project".into(), hex_string(64)),
            ("path".into(), string(240)),
        ]),
        &["project", "path"],
    )
}
fn runner_test_in() -> Value {
    object(
        Map::from_iter([
            ("project".into(), hex_string(64)),
            ("scene".into(), string(240)),
            (
                "frames".into(),
                json!({"type":"integer","minimum":1,"maximum":3600}),
            ),
        ]),
        &["project"],
    )
}
fn runner_export_in() -> Value {
    object(
        Map::from_iter([
            ("project".into(), hex_string(64)),
            ("preset".into(), string(128)),
            ("output".into(), string(200)),
        ]),
        &["project", "preset", "output"],
    )
}
fn runner_build_in() -> Value {
    object(
        Map::from_iter([
            ("project".into(), hex_string(64)),
            ("preset".into(), string(128)),
            ("output".into(), string(200)),
            ("debug".into(), boolean()),
        ]),
        &["project", "preset", "output", "debug"],
    )
}
fn runner_movie_in() -> Value {
    object(
        Map::from_iter([
            ("project".into(), hex_string(64)),
            ("scene".into(), string(240)),
            ("output".into(), string(200)),
            (
                "frames".into(),
                json!({"type":"integer","minimum":1,"maximum":18000}),
            ),
            (
                "fps".into(),
                json!({"type":"integer","minimum":1,"maximum":240}),
            ),
        ]),
        &["project", "output", "frames", "fps"],
    )
}
fn runner_out() -> Value {
    object(
        Map::from_iter([
            ("success".into(), boolean()),
            ("exit_code".into(), json!({"type":"integer"})),
            ("stdout".into(), json!({"type":"string","maxLength":65536})),
            ("stderr".into(), json!({"type":"string","maxLength":65536})),
            ("artifact".into(), json!({"type":"string","maxLength":200})),
        ]),
        &["success", "exit_code", "stdout", "stderr", "artifact"],
    )
}
