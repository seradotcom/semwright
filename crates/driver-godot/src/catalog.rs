use jsonschema::Validator;
use semwright_driver_sdk::{
    Capability, artifact_input_tag, artifact_output_tag, descriptor_digest,
};
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
            let mut tags = vec!["godot".into(), spec.tag.into()];
            match spec.name {
                "assets.rescan" => {
                    tags.push(artifact_input_tag("model/3d")?);
                    tags.push(artifact_input_tag("image/raster")?);
                    tags.push(artifact_input_tag("image/vector")?);
                    tags.push(artifact_input_tag("audio/sample")?);
                }
                "export.pack" => tags.push(artifact_output_tag("game/package")?),
                "export.build" => tags.push(artifact_output_tag("application/binary")?),
                "movie.capture" => tags.push(artifact_output_tag("video/clip")?),
                _ => {}
            }
            let capability = Capability {
                descriptor,
                aliases: vec![],
                tags,
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
            "tilemap.inspect",
            "Inspect TileMapLayer cells and bounds",
            "tilemap",
            "tilemap",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "tilemap.cell.set",
            "Set a TileMapLayer cell by semantic tile identity",
            "tilemap",
            "tile_cell",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            tilemap_cell_set_in,
            mutation_out,
        ),
        spec(
            "tilemap.cell.erase",
            "Erase a TileMapLayer cell",
            "tilemap",
            "tile_cell",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            tilemap_cell_target_in,
            mutation_out,
        ),
        spec(
            "tilemap.clear",
            "Clear all cells in a TileMapLayer",
            "tilemap",
            "tilemap",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            target_mutation_in,
            mutation_out,
        ),
        spec(
            "tileset.inspect",
            "Inspect TileSet geometry, layers and sources",
            "tilemap",
            "tileset",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            path_read_in,
            semantic_read_out,
        ),
        spec(
            "tileset.configure",
            "Configure bounded TileSet layout properties",
            "tilemap",
            "tileset",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            tileset_configure_in,
            mutation_out,
        ),
        spec(
            "tileset.atlas.create",
            "Create a TileSet atlas source from a project texture",
            "tilemap",
            "tileset_source",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            20_000,
            true,
            true,
            tileset_atlas_create_in,
            mutation_out,
        ),
        spec(
            "tileset.tile.create",
            "Create a tile inside an atlas source",
            "tilemap",
            "tile",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            tileset_tile_create_in,
            mutation_out,
        ),
        spec(
            "gridmap.inspect",
            "Inspect GridMap cells and MeshLibrary binding",
            "gridmap",
            "gridmap",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "gridmap.configure",
            "Configure GridMap cell geometry, collision and MeshLibrary",
            "gridmap",
            "gridmap",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            gridmap_configure_in,
            mutation_out,
        ),
        spec(
            "gridmap.cell.set",
            "Set a GridMap cell item and orthogonal orientation",
            "gridmap",
            "grid_cell",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            10_000,
            true,
            true,
            gridmap_cell_set_in,
            mutation_out,
        ),
        spec(
            "gridmap.cell.erase",
            "Erase a GridMap cell",
            "gridmap",
            "grid_cell",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            10_000,
            true,
            true,
            gridmap_cell_target_in,
            mutation_out,
        ),
        spec(
            "gridmap.clear",
            "Clear every populated GridMap cell",
            "gridmap",
            "gridmap",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            target_mutation_in,
            mutation_out,
        ),
        spec(
            "meshlibrary.inspect",
            "Inspect bounded MeshLibrary items used by GridMap",
            "gridmap",
            "mesh_library",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            path_read_in,
            semantic_read_out,
        ),
        spec(
            "meshlibrary.item.create",
            "Create a MeshLibrary item with mesh and navigation semantics",
            "gridmap",
            "mesh_library_item",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            meshlibrary_item_create_in,
            mutation_out,
        ),
        spec(
            "meshlibrary.item.configure",
            "Configure a MeshLibrary item",
            "gridmap",
            "mesh_library_item",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            meshlibrary_item_configure_in,
            mutation_out,
        ),
        spec(
            "meshlibrary.item.remove",
            "Remove a MeshLibrary item",
            "gridmap",
            "mesh_library_item",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            meshlibrary_item_target_in,
            mutation_out,
        ),
        spec(
            "path.inspect",
            "Inspect Path2D/Path3D curve points and baked length",
            "path",
            "path",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "path.configure",
            "Configure a Path2D/Path3D curve",
            "path",
            "path",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            10_000,
            true,
            true,
            path_configure_in,
            mutation_out,
        ),
        spec(
            "path.point.add",
            "Add a typed point to a Path2D/Path3D curve",
            "path",
            "path_point",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            10_000,
            true,
            true,
            path_point_add_in,
            mutation_out,
        ),
        spec(
            "path.point.configure",
            "Configure a Path2D/Path3D curve point",
            "path",
            "path_point",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            10_000,
            true,
            true,
            path_point_configure_in,
            mutation_out,
        ),
        spec(
            "path.point.remove",
            "Remove a point from a Path2D/Path3D curve",
            "path",
            "path_point",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            10_000,
            true,
            true,
            path_point_target_in,
            mutation_out,
        ),
        spec(
            "path.clear",
            "Clear all points from a Path2D/Path3D curve",
            "path",
            "path",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            10_000,
            true,
            true,
            target_mutation_in,
            mutation_out,
        ),
        spec(
            "path.follow.inspect",
            "Inspect PathFollow2D/PathFollow3D traversal state",
            "path",
            "path_follow",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "path.follow.configure",
            "Configure PathFollow2D/PathFollow3D traversal state",
            "path",
            "path_follow",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            10_000,
            true,
            true,
            path_follow_configure_in,
            mutation_out,
        ),
        spec(
            "navigation.region.inspect",
            "Inspect a NavigationRegion2D or NavigationRegion3D",
            "navigation",
            "navigation_region",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "navigation.region.configure",
            "Configure navigation region costs, layers and mesh",
            "navigation",
            "navigation_region",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            navigation_region_configure_in,
            mutation_out,
        ),
        spec(
            "navigation.region.bake",
            "Bake a NavigationRegion2D polygon or NavigationRegion3D mesh synchronously",
            "navigation",
            "navigation_region",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            60_000,
            true,
            true,
            target_mutation_in,
            mutation_out,
        ),
        spec(
            "navigation.agent.inspect",
            "Inspect NavigationAgent2D/3D pathfinding and avoidance state",
            "navigation",
            "navigation_agent",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "navigation.agent.configure",
            "Configure NavigationAgent2D/3D pathfinding and avoidance",
            "navigation",
            "navigation_agent",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            navigation_agent_configure_in,
            mutation_out,
        ),
        spec(
            "navigation.link.configure",
            "Configure a NavigationLink2D or NavigationLink3D",
            "navigation",
            "navigation_link",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            navigation_link_configure_in,
            mutation_out,
        ),
        spec(
            "physics.body.inspect",
            "Inspect a PhysicsBody2D/3D using body-specific semantics",
            "physics",
            "physics_body",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "physics.body.configure",
            "Configure bounded RigidBody/CharacterBody/StaticBody state in 2D or 3D",
            "physics",
            "physics_body",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            physics_body_configure_in,
            mutation_out,
        ),
        spec(
            "physics.area.inspect",
            "Inspect an Area2D or Area3D",
            "physics",
            "area",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "physics.area.configure",
            "Configure Area2D/3D monitoring and physics overrides",
            "physics",
            "area",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            physics_area_configure_in,
            mutation_out,
        ),
        spec(
            "physics.joint.configure",
            "Configure a Joint2D/3D connection",
            "physics",
            "joint",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            physics_joint_configure_in,
            mutation_out,
        ),
        spec(
            "collision.shape.configure",
            "Configure a CollisionShape2D/3D",
            "physics",
            "collision_shape",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            collision_shape_configure_in,
            mutation_out,
        ),
        spec(
            "audio.player.inspect",
            "Inspect an AudioStreamPlayer, 2D or 3D",
            "audio",
            "audio_player",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "audio.player.configure",
            "Configure an AudioStreamPlayer, 2D or 3D",
            "audio",
            "audio_player",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            audio_player_configure_in,
            mutation_out,
        ),
        spec(
            "audio.bus.inspect",
            "Inspect bounded AudioServer bus state",
            "audio",
            "audio_bus",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            session_in,
            semantic_read_out,
        ),
        spec(
            "audio.bus.create",
            "Create and persist an audio bus",
            "audio",
            "audio_bus",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            audio_bus_create_in,
            mutation_out,
        ),
        spec(
            "audio.bus.configure",
            "Configure and persist an audio bus",
            "audio",
            "audio_bus",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            audio_bus_configure_in,
            mutation_out,
        ),
        spec(
            "audio.bus.remove",
            "Remove and persist a non-Master audio bus",
            "audio",
            "audio_bus",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            audio_bus_remove_in,
            mutation_out,
        ),
        spec(
            "audio.effect.add",
            "Add a typed AudioEffect resource to a bus",
            "audio",
            "audio_effect",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            audio_effect_add_in,
            mutation_out,
        ),
        spec(
            "audio.effect.remove",
            "Remove an audio bus effect by index",
            "audio",
            "audio_effect",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            audio_effect_remove_in,
            mutation_out,
        ),
        spec(
            "particles.inspect",
            "Inspect GPU or CPU particles in 2D or 3D",
            "particles",
            "particle_emitter",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "particles.configure",
            "Configure bounded particle-emitter lifecycle state",
            "particles",
            "particle_emitter",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            particles_configure_in,
            mutation_out,
        ),
        spec(
            "particles.restart",
            "Restart a particle emitter",
            "particles",
            "particle_emitter",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            10_000,
            true,
            true,
            target_mutation_in,
            mutation_out,
        ),
        spec(
            "particles.material.configure",
            "Configure a ParticleProcessMaterial",
            "particles",
            "particle_material",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            particle_material_configure_in,
            mutation_out,
        ),
        spec(
            "camera.inspect",
            "Inspect Camera2D or Camera3D semantic state",
            "rendering",
            "camera",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "camera.configure",
            "Configure Camera2D or Camera3D semantic state",
            "rendering",
            "camera",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            camera_configure_in,
            mutation_out,
        ),
        spec(
            "light.inspect",
            "Inspect Light2D or Light3D semantic state",
            "rendering",
            "light",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "light.configure",
            "Configure Light2D or Light3D semantic state",
            "rendering",
            "light",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            light_configure_in,
            mutation_out,
        ),
        spec(
            "environment.inspect",
            "Inspect an Environment resource",
            "rendering",
            "environment",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            path_read_in,
            semantic_read_out,
        ),
        spec(
            "environment.configure",
            "Configure bounded environment, fog, glow and tonemap state",
            "rendering",
            "environment",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            environment_configure_in,
            mutation_out,
        ),
        spec(
            "material.standard.inspect",
            "Inspect a StandardMaterial3D",
            "rendering",
            "material",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            path_read_in,
            semantic_read_out,
        ),
        spec(
            "material.standard.configure",
            "Configure bounded StandardMaterial3D PBR state",
            "rendering",
            "material",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            material_standard_configure_in,
            mutation_out,
        ),
        spec(
            "ui.control.inspect",
            "Inspect a Control node semantic UI state",
            "ui",
            "control",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "ui.control.configure",
            "Configure bounded Control focus, sizing and theme state",
            "ui",
            "control",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            ui_control_configure_in,
            mutation_out,
        ),
        spec(
            "ui.text.configure",
            "Configure text-bearing UI controls",
            "ui",
            "text_control",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            ui_text_configure_in,
            mutation_out,
        ),
        spec(
            "theme.inspect",
            "Inspect a Theme resource and bounded item inventory",
            "ui",
            "theme",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            path_read_in,
            semantic_read_out,
        ),
        spec(
            "theme.configure",
            "Configure a typed Theme item",
            "ui",
            "theme",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            theme_configure_in,
            mutation_out,
        ),
        spec(
            "theme.apply",
            "Apply a Theme resource to a Control",
            "ui",
            "theme",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            theme_apply_in,
            mutation_out,
        ),
        spec(
            "skeleton.inspect",
            "Inspect Skeleton3D bone hierarchy and pose",
            "skeleton",
            "skeleton",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "skeleton.bone.add",
            "Add a bounded Skeleton3D bone",
            "skeleton",
            "bone",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            skeleton_bone_add_in,
            mutation_out,
        ),
        spec(
            "skeleton.bone.configure",
            "Configure a Skeleton3D bone pose and hierarchy",
            "skeleton",
            "bone",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            skeleton_bone_configure_in,
            mutation_out,
        ),
        spec(
            "skeleton.attachment.configure",
            "Configure a BoneAttachment3D",
            "skeleton",
            "bone_attachment",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            skeleton_attachment_configure_in,
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
            "project.window.inspect",
            "Inspect curated project window settings",
            "project",
            "project_settings",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            session_in,
            semantic_read_out,
        ),
        spec(
            "project.window.configure",
            "Configure curated project window settings",
            "project",
            "project_settings",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            project_window_configure_in,
            mutation_out,
        ),
        spec(
            "project.rendering.inspect",
            "Inspect curated project rendering settings",
            "project",
            "project_settings",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            session_in,
            semantic_read_out,
        ),
        spec(
            "project.rendering.configure",
            "Configure curated project rendering settings",
            "project",
            "project_settings",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            project_rendering_configure_in,
            mutation_out,
        ),
        spec(
            "project.physics.inspect",
            "Inspect curated project physics settings",
            "project",
            "project_settings",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            session_in,
            semantic_read_out,
        ),
        spec(
            "project.physics.configure",
            "Configure curated project physics settings",
            "project",
            "project_settings",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            project_physics_configure_in,
            mutation_out,
        ),
        spec(
            "project.layers.inspect",
            "Inspect named project layers",
            "project",
            "project_layers",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            session_in,
            semantic_read_out,
        ),
        spec(
            "project.layers.set",
            "Set a named project layer",
            "project",
            "project_layer",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            project_layer_set_in,
            mutation_out,
        ),
        spec(
            "autoload.list",
            "List project autoload singletons",
            "project",
            "autoload",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            session_in,
            semantic_read_out,
        ),
        spec(
            "autoload.add",
            "Add a project autoload singleton",
            "project",
            "autoload",
            Route::Plugin,
            CodeExecution,
            NonIdempotent,
            15_000,
            true,
            true,
            autoload_add_in,
            mutation_out,
        ),
        spec(
            "autoload.remove",
            "Remove a project autoload singleton",
            "project",
            "autoload",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            autoload_remove_in,
            mutation_out,
        ),
        spec(
            "asset.inspect",
            "Inspect a project asset and import state",
            "asset",
            "asset",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            path_read_in,
            semantic_read_out,
        ),
        spec(
            "asset.dependencies",
            "Inspect bounded resource dependencies",
            "asset",
            "asset",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            path_read_in,
            semantic_read_out,
        ),
        spec(
            "asset.reimport",
            "Reimport a project asset through EditorFileSystem",
            "asset",
            "asset",
            Route::Plugin,
            CodeExecution,
            NonIdempotent,
            60_000,
            true,
            true,
            path_mutation_in,
            mutation_out,
        ),
        spec(
            "asset.import.inspect",
            "Inspect existing import parameters",
            "asset",
            "asset_import",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            path_read_in,
            semantic_read_out,
        ),
        spec(
            "asset.import.configure",
            "Configure existing import parameters and reimport",
            "asset",
            "asset_import",
            Route::Plugin,
            CodeExecution,
            NonIdempotent,
            60_000,
            true,
            true,
            asset_import_configure_in,
            mutation_out,
        ),
        spec(
            "export.preset.list",
            "List non-secret export presets",
            "export",
            "export_preset",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            session_in,
            semantic_read_out,
        ),
        spec(
            "export.preset.inspect",
            "Inspect a non-secret export preset",
            "export",
            "export_preset",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            export_preset_inspect_in,
            semantic_read_out,
        ),
        spec(
            "export.preset.configure",
            "Configure an existing non-secret export preset",
            "export",
            "export_preset",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            export_preset_configure_in,
            mutation_out,
        ),
        spec(
            "localization.inspect",
            "Inspect project localization configuration",
            "localization",
            "localization",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            session_in,
            semantic_read_out,
        ),
        spec(
            "localization.configure",
            "Configure project localization resources and fallback",
            "localization",
            "localization",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            localization_configure_in,
            mutation_out,
        ),
        spec(
            "translation.inspect",
            "Inspect a Translation resource",
            "localization",
            "translation",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            path_read_in,
            semantic_read_out,
        ),
        spec(
            "translation.create",
            "Create a Translation resource",
            "localization",
            "translation",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            15_000,
            true,
            true,
            translation_create_in,
            mutation_out,
        ),
        spec(
            "translation.message.set",
            "Set a translation message",
            "localization",
            "translation_message",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            translation_message_set_in,
            mutation_out,
        ),
        spec(
            "translation.message.remove",
            "Remove a translation message",
            "localization",
            "translation_message",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            translation_message_remove_in,
            mutation_out,
        ),
        spec(
            "animation_tree.inspect",
            "Inspect an AnimationTree state machine graph",
            "animation",
            "animation_tree",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            animation_tree_read_in,
            semantic_read_out,
        ),
        spec(
            "animation_tree.state.add",
            "Add a typed state to an AnimationTree state machine",
            "animation",
            "animation_state",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_tree_state_add_in,
            mutation_out,
        ),
        spec(
            "animation_tree.state.remove",
            "Remove a state from an AnimationTree state machine",
            "animation",
            "animation_state",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_tree_state_target_in,
            mutation_out,
        ),
        spec(
            "animation_tree.transition.add",
            "Add a state-machine transition",
            "animation",
            "animation_transition",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_tree_transition_in,
            mutation_out,
        ),
        spec(
            "animation_tree.transition.configure",
            "Configure a state-machine transition",
            "animation",
            "animation_transition",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_tree_transition_in,
            mutation_out,
        ),
        spec(
            "animation_tree.transition.remove",
            "Remove a state-machine transition",
            "animation",
            "animation_transition",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_tree_transition_target_in,
            mutation_out,
        ),
        spec(
            "animation_tree.parameter.set",
            "Set an existing AnimationTree parameter",
            "animation",
            "animation_parameter",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            10_000,
            true,
            true,
            animation_tree_parameter_set_in,
            mutation_out,
        ),
        spec(
            "animation_tree.node.inspect",
            "Inspect a recursively-addressed AnimationTree graph node",
            "animation",
            "animation_graph_node",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            animation_graph_read_in,
            semantic_read_out,
        ),
        spec(
            "animation_tree.node.configure",
            "Configure a typed AnimationTree graph node",
            "animation",
            "animation_graph_node",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_graph_node_configure_in,
            mutation_out,
        ),
        spec(
            "animation_tree.blend_tree.inspect",
            "Inspect nodes and connections in a recursively-addressed BlendTree",
            "animation",
            "animation_blend_tree",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            animation_graph_read_in,
            semantic_read_out,
        ),
        spec(
            "animation_tree.blend_tree.node.add",
            "Add a typed node to a BlendTree",
            "animation",
            "animation_blend_tree_node",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_blend_tree_node_add_in,
            mutation_out,
        ),
        spec(
            "animation_tree.blend_tree.node.configure",
            "Configure or reposition a BlendTree node",
            "animation",
            "animation_blend_tree_node",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_blend_tree_node_configure_in,
            mutation_out,
        ),
        spec(
            "animation_tree.blend_tree.node.remove",
            "Remove a BlendTree node",
            "animation",
            "animation_blend_tree_node",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_blend_tree_node_target_in,
            mutation_out,
        ),
        spec(
            "animation_tree.blend_tree.connection.set",
            "Connect or disconnect a typed BlendTree input",
            "animation",
            "animation_blend_tree_connection",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_blend_tree_connection_in,
            mutation_out,
        ),
        spec(
            "animation_tree.blend_space.inspect",
            "Inspect a recursively-addressed BlendSpace1D or BlendSpace2D",
            "animation",
            "animation_blend_space",
            Route::Plugin,
            R,
            ReadOnly,
            15_000,
            false,
            false,
            animation_graph_read_in,
            semantic_read_out,
        ),
        spec(
            "animation_tree.blend_space.configure",
            "Configure BlendSpace extents, labels, interpolation and synchronization",
            "animation",
            "animation_blend_space",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_blend_space_configure_in,
            mutation_out,
        ),
        spec(
            "animation_tree.blend_space.point.add",
            "Add a named typed root node to a BlendSpace",
            "animation",
            "animation_blend_point",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_blend_space_point_add_in,
            mutation_out,
        ),
        spec(
            "animation_tree.blend_space.point.configure",
            "Configure a BlendSpace point",
            "animation",
            "animation_blend_point",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_blend_space_point_configure_in,
            mutation_out,
        ),
        spec(
            "animation_tree.blend_space.point.remove",
            "Remove a BlendSpace point",
            "animation",
            "animation_blend_point",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_blend_space_point_target_in,
            mutation_out,
        ),
        spec(
            "animation_tree.blend_space.triangle.add",
            "Add a manual BlendSpace2D triangle",
            "animation",
            "animation_blend_triangle",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_blend_space_triangle_add_in,
            mutation_out,
        ),
        spec(
            "animation_tree.blend_space.triangle.remove",
            "Remove a manual BlendSpace2D triangle",
            "animation",
            "animation_blend_triangle",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            animation_blend_space_triangle_target_in,
            mutation_out,
        ),
        spec(
            "multiplayer.spawner.inspect",
            "Inspect MultiplayerSpawner authoring state",
            "multiplayer",
            "multiplayer_spawner",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "multiplayer.spawner.configure",
            "Configure MultiplayerSpawner authoring state",
            "multiplayer",
            "multiplayer_spawner",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            multiplayer_spawner_configure_in,
            mutation_out,
        ),
        spec(
            "multiplayer.spawner.scene.add",
            "Add a spawnable PackedScene",
            "multiplayer",
            "spawnable_scene",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            multiplayer_spawner_scene_in,
            mutation_out,
        ),
        spec(
            "multiplayer.spawner.scene.remove",
            "Remove a spawnable PackedScene",
            "multiplayer",
            "spawnable_scene",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            multiplayer_spawner_scene_in,
            mutation_out,
        ),
        spec(
            "multiplayer.synchronizer.inspect",
            "Inspect MultiplayerSynchronizer replication state",
            "multiplayer",
            "multiplayer_synchronizer",
            Route::Plugin,
            R,
            ReadOnly,
            10_000,
            false,
            false,
            target_read_in,
            semantic_read_out,
        ),
        spec(
            "multiplayer.synchronizer.configure",
            "Configure MultiplayerSynchronizer authoring state",
            "multiplayer",
            "multiplayer_synchronizer",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            multiplayer_synchronizer_configure_in,
            mutation_out,
        ),
        spec(
            "multiplayer.replication.property.add",
            "Add a replicated property",
            "multiplayer",
            "replication_property",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            multiplayer_replication_property_in,
            mutation_out,
        ),
        spec(
            "multiplayer.replication.property.configure",
            "Configure a replicated property",
            "multiplayer",
            "replication_property",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            15_000,
            true,
            true,
            multiplayer_replication_property_in,
            mutation_out,
        ),
        spec(
            "multiplayer.replication.property.remove",
            "Remove a replicated property",
            "multiplayer",
            "replication_property",
            Route::Plugin,
            Destructive,
            NonIdempotent,
            15_000,
            true,
            true,
            multiplayer_replication_property_target_in,
            mutation_out,
        ),
        spec(
            "editor.state",
            "Inspect semantic Godot editor state",
            "editor",
            "editor_state",
            Route::Plugin,
            R,
            ReadOnly,
            5_000,
            false,
            false,
            session_in,
            semantic_read_out,
        ),
        spec(
            "editor.selection.get",
            "Inspect SceneTree editor selection",
            "editor",
            "editor_selection",
            Route::Plugin,
            R,
            ReadOnly,
            5_000,
            false,
            false,
            session_in,
            semantic_read_out,
        ),
        spec(
            "editor.selection.set",
            "Set SceneTree editor selection",
            "editor",
            "editor_selection",
            Route::Plugin,
            MutatingReversible,
            NonIdempotent,
            5_000,
            true,
            true,
            editor_selection_set_in,
            mutation_out,
        ),
        spec(
            "editor.run.start",
            "Start a bounded scene run from the editor",
            "editor",
            "editor_runtime",
            Route::Plugin,
            CodeExecution,
            NonIdempotent,
            15_000,
            true,
            true,
            editor_run_start_in,
            mutation_out,
        ),
        spec(
            "editor.run.stop",
            "Stop the editor scene run",
            "editor",
            "editor_runtime",
            Route::Plugin,
            Mutating,
            NonIdempotent,
            10_000,
            true,
            true,
            mutation_in,
            mutation_out,
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
    let device = json!({"type":"integer","minimum":-1,"maximum":32});
    let coded = |kind: &str, max_code: i64| {
        json!({
            "type":"object",
            "additionalProperties":false,
            "required":["type","code"],
            "properties":{
                "type":{"const":kind},
                "code":{"type":"integer","minimum":0,"maximum":max_code},
                "device":device.clone()
            }
        })
    };
    let axis = json!({
        "type":"object",
        "additionalProperties":false,
        "required":["type","axis","value"],
        "properties":{
            "type":{"const":"joy_axis"},
            "axis":{"type":"integer","minimum":0,"maximum":15},
            "value":{"type":"number","minimum":-1.0,"maximum":1.0},
            "device":device.clone()
        }
    });
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
                json!({
                    "type":"array",
                    "maxItems":32,
                    "items":{"oneOf":[
                        coded("key", 1_000_000),
                        coded("mouse_button", 128),
                        coded("joy_button", 128),
                        axis
                    ]}
                }),
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

fn target_read_in() -> Value {
    node_target_read_in()
}
fn target_mutation_in() -> Value {
    node_target_mutation_in()
}
fn semantic_read_out() -> Value {
    read_out(json!({"type":"object","maxProperties":128}))
}
fn vec2_schema() -> Value {
    json!({"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}})
}
fn vec2i_schema() -> Value {
    json!({"type":"array","minItems":2,"maxItems":2,"items":{"type":"integer","minimum":-32768,"maximum":32767}})
}
fn vec2i_positive_schema() -> Value {
    json!({"type":"array","minItems":2,"maxItems":2,"items":{"type":"integer","minimum":1,"maximum":4096}})
}
fn vec3i_schema() -> Value {
    json!({"type":"array","minItems":3,"maxItems":3,"items":{"type":"integer","minimum":-32768,"maximum":32767}})
}
fn vec3_schema() -> Value {
    json!({"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","minimum":-1000000000.0,"maximum":1000000000.0}})
}
fn vec2_or_vec3_schema() -> Value {
    json!({"oneOf":[vec2_schema(),vec3_schema()]})
}
fn number_or_vec3_schema() -> Value {
    json!({"oneOf":[
        {"type":"number","minimum":-1000000000.0,"maximum":1000000000.0},
        vec3_schema()
    ]})
}
fn bool_or_ccd_mode_schema() -> Value {
    json!({"oneOf":[{"type":"boolean"},{"type":"integer","minimum":0,"maximum":2}]})
}
fn quat_schema() -> Value {
    json!({"type":"array","minItems":4,"maxItems":4,"items":{"type":"number","minimum":-1.0,"maximum":1.0}})
}
fn color_array_schema() -> Value {
    json!({"type":"array","minItems":3,"maxItems":4,"items":{"type":"number","minimum":0.0,"maximum":1.0}})
}
fn u32_schema() -> Value {
    json!({"type":"integer","minimum":0,"maximum":4294967295u64})
}
fn optional_path_schema() -> Value {
    json!({"type":"string","maxLength":240})
}
fn bounded_number(min: f64, max: f64) -> Value {
    json!({"type":"number","minimum":min,"maximum":max})
}
fn bounded_int(min: i64, max: i64) -> Value {
    json!({"type":"integer","minimum":min,"maximum":max})
}
fn target_mutation_schema(mut extra: Map<String, Value>, required_extra: &[&str]) -> Value {
    extra.insert("session".into(), hex_string(32));
    extra.insert("target".into(), string(240));
    extra.insert("expect".into(), stamp());
    extra.insert("dry_run".into(), boolean());
    let mut required = vec!["session", "target"];
    required.extend_from_slice(required_extra);
    required.extend_from_slice(&["expect", "dry_run"]);
    object(extra, &required)
}
fn path_mutation_schema(mut extra: Map<String, Value>, required_extra: &[&str]) -> Value {
    extra.insert("session".into(), hex_string(32));
    extra.insert("path".into(), string(240));
    extra.insert("expect".into(), stamp());
    extra.insert("dry_run".into(), boolean());
    let mut required = vec!["session", "path"];
    required.extend_from_slice(required_extra);
    required.extend_from_slice(&["expect", "dry_run"]);
    object(extra, &required)
}
fn session_mutation_schema(mut extra: Map<String, Value>, required_extra: &[&str]) -> Value {
    extra.insert("session".into(), hex_string(32));
    extra.insert("expect".into(), stamp());
    extra.insert("dry_run".into(), boolean());
    let mut required = vec!["session"];
    required.extend_from_slice(required_extra);
    required.extend_from_slice(&["expect", "dry_run"]);
    object(extra, &required)
}

fn tilemap_cell_set_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("coords".into(), vec2i_schema()),
            ("source_id".into(), bounded_int(-1, i32::MAX as i64)),
            ("atlas_coords".into(), vec2i_schema()),
            ("alternative".into(), bounded_int(0, i32::MAX as i64)),
        ]),
        &["coords", "source_id", "atlas_coords"],
    )
}
fn tilemap_cell_target_in() -> Value {
    target_mutation_schema(
        Map::from_iter([("coords".into(), vec2i_schema())]),
        &["coords"],
    )
}
fn tileset_configure_in() -> Value {
    path_mutation_schema(
        Map::from_iter([
            ("tile_size".into(), vec2i_positive_schema()),
            ("tile_shape".into(), bounded_int(0, 3)),
            ("tile_layout".into(), bounded_int(0, 5)),
            ("tile_offset_axis".into(), bounded_int(0, 1)),
            ("uv_clipping".into(), boolean()),
        ]),
        &[],
    )
}
fn tileset_atlas_create_in() -> Value {
    path_mutation_schema(
        Map::from_iter([
            ("texture".into(), string(240)),
            ("region_size".into(), vec2i_positive_schema()),
            ("margins".into(), vec2i_schema()),
            ("separation".into(), vec2i_schema()),
            ("source_id".into(), bounded_int(-1, i32::MAX as i64)),
        ]),
        &["texture", "region_size"],
    )
}
fn tileset_tile_create_in() -> Value {
    path_mutation_schema(
        Map::from_iter([
            ("source_id".into(), bounded_int(0, i32::MAX as i64)),
            ("atlas_coords".into(), vec2i_schema()),
            ("size".into(), vec2i_positive_schema()),
        ]),
        &["source_id", "atlas_coords"],
    )
}
fn gridmap_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("mesh_library".into(), optional_path_schema()),
            ("cell_size".into(), vec3_schema()),
            ("cell_octant_size".into(), bounded_int(1, 1024)),
            ("cell_scale".into(), bounded_number(0.001, 1000.0)),
            ("cell_center_x".into(), boolean()),
            ("cell_center_y".into(), boolean()),
            ("cell_center_z".into(), boolean()),
            ("bake_navigation".into(), boolean()),
            ("collision_layer".into(), u32_schema()),
            ("collision_mask".into(), u32_schema()),
        ]),
        &[],
    )
}
fn gridmap_cell_set_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("position".into(), vec3i_schema()),
            ("item".into(), bounded_int(0, i32::MAX as i64)),
            ("orientation".into(), bounded_int(0, 23)),
        ]),
        &["position", "item"],
    )
}
fn gridmap_cell_target_in() -> Value {
    target_mutation_schema(
        Map::from_iter([("position".into(), vec3i_schema())]),
        &["position"],
    )
}
fn meshlibrary_item_create_in() -> Value {
    path_mutation_schema(
        Map::from_iter([
            ("id".into(), bounded_int(-1, i32::MAX as i64)),
            ("name".into(), string(96)),
            ("mesh".into(), optional_path_schema()),
            ("navigation_mesh".into(), optional_path_schema()),
            ("navigation_layers".into(), u32_schema()),
        ]),
        &[],
    )
}
fn meshlibrary_item_configure_in() -> Value {
    path_mutation_schema(
        Map::from_iter([
            ("id".into(), bounded_int(0, i32::MAX as i64)),
            ("name".into(), string(96)),
            ("mesh".into(), optional_path_schema()),
            ("navigation_mesh".into(), optional_path_schema()),
            ("navigation_layers".into(), u32_schema()),
        ]),
        &["id"],
    )
}
fn meshlibrary_item_target_in() -> Value {
    path_mutation_schema(
        Map::from_iter([("id".into(), bounded_int(0, i32::MAX as i64))]),
        &["id"],
    )
}
fn path_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("bake_interval".into(), bounded_number(0.001, 10000.0)),
            ("closed".into(), boolean()),
            ("up_vector_enabled".into(), boolean()),
        ]),
        &[],
    )
}
fn path_point_add_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("position".into(), vec2_or_vec3_schema()),
            ("in".into(), vec2_or_vec3_schema()),
            ("out".into(), vec2_or_vec3_schema()),
            ("index".into(), bounded_int(-1, 2048)),
            (
                "tilt".into(),
                bounded_number(-std::f64::consts::TAU * 64.0, std::f64::consts::TAU * 64.0),
            ),
        ]),
        &["position"],
    )
}
fn path_point_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("index".into(), bounded_int(0, 2047)),
            ("position".into(), vec2_or_vec3_schema()),
            ("in".into(), vec2_or_vec3_schema()),
            ("out".into(), vec2_or_vec3_schema()),
            (
                "tilt".into(),
                bounded_number(-std::f64::consts::TAU * 64.0, std::f64::consts::TAU * 64.0),
            ),
        ]),
        &["index"],
    )
}
fn path_point_target_in() -> Value {
    target_mutation_schema(
        Map::from_iter([("index".into(), bounded_int(0, 2047))]),
        &["index"],
    )
}
fn path_follow_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("progress".into(), bounded_number(0.0, 1_000_000_000.0)),
            ("progress_ratio".into(), bounded_number(0.0, 1.0)),
            ("loop".into(), boolean()),
            ("cubic_interp".into(), boolean()),
            ("h_offset".into(), bounded_number(-1_000_000.0, 1_000_000.0)),
            ("v_offset".into(), bounded_number(-1_000_000.0, 1_000_000.0)),
            ("rotates".into(), boolean()),
            ("rotation_mode".into(), bounded_int(0, 4)),
            ("tilt_enabled".into(), boolean()),
            ("use_model_front".into(), boolean()),
        ]),
        &[],
    )
}
fn navigation_region_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("enabled".into(), boolean()),
            ("navigation_layers".into(), u32_schema()),
            ("enter_cost".into(), bounded_number(0.0, 1_000_000.0)),
            ("travel_cost".into(), bounded_number(0.0, 1_000_000.0)),
            ("use_edge_connections".into(), boolean()),
            ("navigation_mesh".into(), optional_path_schema()),
            ("navigation_polygon".into(), optional_path_schema()),
        ]),
        &[],
    )
}
fn navigation_agent_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("navigation_layers".into(), u32_schema()),
            ("target_position".into(), vec2_or_vec3_schema()),
            (
                "path_desired_distance".into(),
                bounded_number(0.0, 1_000_000.0),
            ),
            (
                "target_desired_distance".into(),
                bounded_number(0.0, 1_000_000.0),
            ),
            ("path_max_distance".into(), bounded_number(0.0, 1_000_000.0)),
            ("radius".into(), bounded_number(0.0, 1_000_000.0)),
            ("height".into(), bounded_number(0.0, 1_000_000.0)),
            ("max_speed".into(), bounded_number(0.0, 1_000_000.0)),
            ("avoidance_enabled".into(), boolean()),
            ("avoidance_layers".into(), u32_schema()),
            ("avoidance_mask".into(), u32_schema()),
            ("avoidance_priority".into(), bounded_number(0.0, 1.0)),
            ("neighbor_distance".into(), bounded_number(0.0, 1_000_000.0)),
            ("max_neighbors".into(), bounded_int(0, 4096)),
            ("use_3d_avoidance".into(), boolean()),
            ("time_horizon_agents".into(), bounded_number(0.0, 3600.0)),
            ("time_horizon_obstacles".into(), bounded_number(0.0, 3600.0)),
        ]),
        &[],
    )
}
fn navigation_link_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("enabled".into(), boolean()),
            ("bidirectional".into(), boolean()),
            ("navigation_layers".into(), u32_schema()),
            ("enter_cost".into(), bounded_number(0.0, 1_000_000.0)),
            ("travel_cost".into(), bounded_number(0.0, 1_000_000.0)),
            ("start_position".into(), vec2_or_vec3_schema()),
            ("end_position".into(), vec2_or_vec3_schema()),
        ]),
        &[],
    )
}
fn physics_body_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("collision_layer".into(), u32_schema()),
            ("collision_mask".into(), u32_schema()),
            ("mass".into(), bounded_number(0.0001, 1_000_000.0)),
            ("gravity_scale".into(), bounded_number(-1000.0, 1000.0)),
            ("linear_damp".into(), bounded_number(-1.0, 1000.0)),
            ("angular_damp".into(), bounded_number(-1.0, 1000.0)),
            ("lock_rotation".into(), boolean()),
            ("freeze".into(), boolean()),
            ("continuous_cd".into(), bool_or_ccd_mode_schema()),
            ("freeze_mode".into(), bounded_int(0, 1)),
            ("linear_velocity".into(), vec2_or_vec3_schema()),
            ("angular_velocity".into(), number_or_vec3_schema()),
            ("motion_mode".into(), bounded_int(0, 1)),
            ("max_slides".into(), bounded_int(1, 64)),
            ("floor_stop_on_slope".into(), boolean()),
            (
                "floor_max_angle".into(),
                bounded_number(0.0, std::f64::consts::PI),
            ),
            ("floor_snap_length".into(), bounded_number(0.0, 1_000_000.0)),
            (
                "wall_min_slide_angle".into(),
                bounded_number(0.0, std::f64::consts::PI),
            ),
            ("up_direction".into(), vec2_or_vec3_schema()),
            ("velocity".into(), vec2_or_vec3_schema()),
            ("constant_linear_velocity".into(), vec2_or_vec3_schema()),
            ("constant_angular_velocity".into(), number_or_vec3_schema()),
        ]),
        &[],
    )
}
fn physics_area_configure_in() -> Value {
    let mut schema = target_mutation_schema(
        Map::from_iter([
            ("monitoring".into(), boolean()),
            ("monitorable".into(), boolean()),
            ("priority".into(), bounded_number(-1_000_000.0, 1_000_000.0)),
            ("gravity_point".into(), boolean()),
            ("gravity".into(), bounded_number(-1_000_000.0, 1_000_000.0)),
            ("gravity_space_override".into(), bounded_int(0, 4)),
            ("linear_damp_space_override".into(), bounded_int(0, 4)),
            ("linear_damp".into(), bounded_number(-1.0, 1000.0)),
            ("angular_damp_space_override".into(), bounded_int(0, 4)),
            ("angular_damp".into(), bounded_number(-1.0, 1000.0)),
            ("audio_bus_override".into(), boolean()),
            ("audio_bus_name".into(), string(96)),
            ("gravity_direction".into(), vec2_or_vec3_schema()),
            ("gravity_point_center".into(), vec2_or_vec3_schema()),
            (
                "gravity_point_unit_distance".into(),
                bounded_number(0.0, 1_000_000.0),
            ),
            ("collision_layer".into(), u32_schema()),
            ("collision_mask".into(), u32_schema()),
        ]),
        &[],
    );
    schema["not"] = json!({"required":["gravity_direction","gravity_point_center"]});
    schema
}
fn physics_joint_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("node_a".into(), optional_path_schema()),
            ("node_b".into(), optional_path_schema()),
            ("solver_priority".into(), bounded_int(1, 64)),
            ("bias".into(), bounded_number(0.0, 1.0)),
            ("exclude_nodes_from_collision".into(), boolean()),
        ]),
        &[],
    )
}
fn collision_shape_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("shape".into(), optional_path_schema()),
            ("disabled".into(), boolean()),
        ]),
        &[],
    )
}
fn audio_player_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("stream".into(), optional_path_schema()),
            ("bus".into(), string(96)),
            ("volume_db".into(), bounded_number(-120.0, 48.0)),
            ("pitch_scale".into(), bounded_number(0.01, 16.0)),
            ("autoplay".into(), boolean()),
            ("max_polyphony".into(), bounded_int(1, 128)),
            ("max_distance".into(), bounded_number(0.0, 1_000_000.0)),
            ("unit_size".into(), bounded_number(0.001, 1_000_000.0)),
            ("panning_strength".into(), bounded_number(0.0, 16.0)),
            ("attenuation_model".into(), bounded_int(0, 4)),
            ("doppler_tracking".into(), bounded_int(0, 2)),
        ]),
        &[],
    )
}
fn audio_bus_create_in() -> Value {
    session_mutation_schema(
        Map::from_iter([
            ("name".into(), string(96)),
            ("position".into(), bounded_int(-1, 63)),
            ("volume_db".into(), bounded_number(-120.0, 48.0)),
            ("send".into(), string(96)),
        ]),
        &["name"],
    )
}
fn audio_bus_configure_in() -> Value {
    session_mutation_schema(
        Map::from_iter([
            ("name".into(), string(96)),
            ("volume_db".into(), bounded_number(-120.0, 48.0)),
            ("mute".into(), boolean()),
            ("solo".into(), boolean()),
            ("send".into(), string(96)),
        ]),
        &["name"],
    )
}
fn audio_bus_remove_in() -> Value {
    session_mutation_schema(Map::from_iter([("name".into(), string(96))]), &["name"])
}
fn audio_effect_add_in() -> Value {
    session_mutation_schema(
        Map::from_iter([
            ("bus".into(), string(96)),
            ("effect".into(), string(240)),
            ("position".into(), bounded_int(-1, 31)),
        ]),
        &["bus", "effect"],
    )
}
fn audio_effect_remove_in() -> Value {
    session_mutation_schema(
        Map::from_iter([
            ("bus".into(), string(96)),
            ("index".into(), bounded_int(0, 31)),
        ]),
        &["bus", "index"],
    )
}
fn particles_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("amount".into(), bounded_int(1, 1_000_000)),
            ("lifetime".into(), bounded_number(0.001, 3600.0)),
            ("emitting".into(), boolean()),
            ("one_shot".into(), boolean()),
            ("preprocess".into(), bounded_number(0.0, 3600.0)),
            ("randomness".into(), bounded_number(0.0, 1.0)),
            ("speed_scale".into(), bounded_number(0.0, 1000.0)),
            ("amount_ratio".into(), bounded_number(0.0, 1.0)),
            ("explosiveness".into(), bounded_number(0.0, 1.0)),
            ("local_coords".into(), boolean()),
            ("fixed_fps".into(), bounded_int(0, 1000)),
            ("use_fixed_seed".into(), boolean()),
            ("seed".into(), bounded_int(0, i32::MAX as i64)),
            ("trail_enabled".into(), boolean()),
            ("trail_lifetime".into(), bounded_number(0.0, 3600.0)),
            ("process_material".into(), optional_path_schema()),
        ]),
        &[],
    )
}
fn particle_material_configure_in() -> Value {
    path_mutation_schema(
        Map::from_iter([
            ("direction".into(), vec3_schema()),
            ("gravity".into(), vec3_schema()),
            ("color".into(), color_array_schema()),
            ("emission_box_extents".into(), vec3_schema()),
            ("emission_shape".into(), bounded_int(0, 6)),
            ("spread".into(), bounded_number(0.0, 180.0)),
            (
                "initial_velocity_min".into(),
                bounded_number(-1_000_000.0, 1_000_000.0),
            ),
            (
                "initial_velocity_max".into(),
                bounded_number(-1_000_000.0, 1_000_000.0),
            ),
            ("scale_min".into(), bounded_number(0.0, 1_000_000.0)),
            ("scale_max".into(), bounded_number(0.0, 1_000_000.0)),
            ("lifetime_randomness".into(), bounded_number(0.0, 1.0)),
        ]),
        &[],
    )
}
fn camera_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("projection".into(), bounded_int(0, 2)),
            ("fov".into(), bounded_number(1.0, 179.0)),
            ("size".into(), bounded_number(0.001, 1_000_000.0)),
            ("near".into(), bounded_number(0.001, 1_000_000.0)),
            ("far".into(), bounded_number(0.002, 10_000_000.0)),
            ("keep_aspect".into(), bounded_int(0, 1)),
            ("current".into(), boolean()),
            ("cull_mask".into(), u32_schema()),
            ("environment".into(), optional_path_schema()),
            ("enabled".into(), boolean()),
            ("ignore_rotation".into(), boolean()),
            ("position_smoothing_enabled".into(), boolean()),
            (
                "position_smoothing_speed".into(),
                bounded_number(0.0, 1_000_000.0),
            ),
            ("rotation_smoothing_enabled".into(), boolean()),
            (
                "rotation_smoothing_speed".into(),
                bounded_number(0.0, 1_000_000.0),
            ),
            ("limit_enabled".into(), boolean()),
            ("zoom".into(), vec2_schema()),
            ("offset".into(), vec2_schema()),
            (
                "limits".into(),
                json!({"type":"array","minItems":4,"maxItems":4,"items":{"type":"integer","minimum":-1000000000,"maximum":1000000000}}),
            ),
        ]),
        &[],
    )
}
fn light_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("color".into(), color_array_schema()),
            ("energy".into(), bounded_number(0.0, 1_000_000.0)),
            ("indirect_energy".into(), bounded_number(0.0, 1_000_000.0)),
            ("specular".into(), bounded_number(0.0, 1_000_000.0)),
            (
                "volumetric_fog_energy".into(),
                bounded_number(0.0, 1_000_000.0),
            ),
            ("shadow_enabled".into(), boolean()),
            ("cull_mask".into(), u32_schema()),
            ("enabled".into(), boolean()),
            ("blend_mode".into(), bounded_int(0, 3)),
            ("shadow_cull_mask".into(), u32_schema()),
        ]),
        &[],
    )
}
fn environment_configure_in() -> Value {
    path_mutation_schema(
        Map::from_iter([
            ("background_mode".into(), bounded_int(0, 6)),
            ("background_color".into(), color_array_schema()),
            (
                "background_energy_multiplier".into(),
                bounded_number(0.0, 1000.0),
            ),
            ("ambient_light_source".into(), bounded_int(0, 3)),
            ("ambient_light_color".into(), color_array_schema()),
            ("ambient_light_energy".into(), bounded_number(0.0, 1000.0)),
            ("fog_enabled".into(), boolean()),
            ("fog_density".into(), bounded_number(0.0, 1.0)),
            ("fog_light_color".into(), color_array_schema()),
            ("fog_light_energy".into(), bounded_number(0.0, 1000.0)),
            ("glow_enabled".into(), boolean()),
            ("glow_intensity".into(), bounded_number(0.0, 1000.0)),
            ("tonemap_mode".into(), bounded_int(0, 4)),
        ]),
        &[],
    )
}
fn material_standard_configure_in() -> Value {
    path_mutation_schema(
        Map::from_iter([
            ("albedo_color".into(), color_array_schema()),
            ("metallic".into(), bounded_number(0.0, 1.0)),
            ("roughness".into(), bounded_number(0.0, 1.0)),
            ("emission_enabled".into(), boolean()),
            ("emission".into(), color_array_schema()),
            ("transparency".into(), bounded_int(0, 6)),
            ("shading_mode".into(), bounded_int(0, 2)),
        ]),
        &[],
    )
}
fn ui_control_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("visible".into(), boolean()),
            ("focus_mode".into(), bounded_int(0, 2)),
            ("mouse_filter".into(), bounded_int(0, 2)),
            ("layout_direction".into(), bounded_int(0, 3)),
            ("size_flags_horizontal".into(), bounded_int(0, 31)),
            ("size_flags_vertical".into(), bounded_int(0, 31)),
            (
                "size_flags_stretch_ratio".into(),
                bounded_number(0.0, 1000.0),
            ),
            (
                "tooltip_text".into(),
                json!({"type":"string","maxLength":4096}),
            ),
            (
                "theme_type_variation".into(),
                json!({"type":"string","maxLength":96}),
            ),
            ("minimum_size".into(), vec2_schema()),
        ]),
        &[],
    )
}
fn ui_text_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("text".into(), json!({"type":"string","maxLength":262144})),
            ("horizontal_alignment".into(), bounded_int(0, 3)),
            ("vertical_alignment".into(), bounded_int(0, 3)),
            ("autowrap_mode".into(), bounded_int(0, 3)),
            ("text_overrun_behavior".into(), bounded_int(0, 4)),
            ("uppercase".into(), boolean()),
            ("editable".into(), boolean()),
            ("secret".into(), boolean()),
        ]),
        &[],
    )
}
fn theme_configure_in() -> Value {
    path_mutation_schema(
        Map::from_iter([
            (
                "kind".into(),
                json!({"enum":["color","constant","font_size","font","icon","stylebox","type_variation"]}),
            ),
            ("theme_type".into(), json!({"type":"string","maxLength":96})),
            ("name".into(), json!({"type":"string","maxLength":96})),
            ("color".into(), color_array_schema()),
            ("integer".into(), bounded_int(-1_000_000, 1_000_000)),
            ("resource".into(), optional_path_schema()),
            ("base_type".into(), json!({"type":"string","maxLength":96})),
        ]),
        &["kind", "theme_type", "name"],
    )
}
fn theme_apply_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("theme".into(), optional_path_schema()),
            (
                "type_variation".into(),
                json!({"type":"string","maxLength":96}),
            ),
        ]),
        &["theme"],
    )
}
fn skeleton_bone_add_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("name".into(), string(96)),
            ("parent".into(), bounded_int(-1, 511)),
            ("position".into(), vec3_schema()),
            ("rotation".into(), quat_schema()),
            ("scale".into(), vec3_schema()),
        ]),
        &["name"],
    )
}
fn skeleton_bone_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("index".into(), bounded_int(0, 511)),
            ("name".into(), string(96)),
            ("parent".into(), bounded_int(-1, 511)),
            ("enabled".into(), boolean()),
            ("position".into(), vec3_schema()),
            ("rotation".into(), quat_schema()),
            ("scale".into(), vec3_schema()),
            ("reset_pose".into(), boolean()),
        ]),
        &["index"],
    )
}
fn skeleton_attachment_configure_in() -> Value {
    target_mutation_schema(
        Map::from_iter([
            ("bone_name".into(), json!({"type":"string","maxLength":96})),
            ("bone_idx".into(), bounded_int(-1, 511)),
            ("override_pose".into(), boolean()),
            ("use_external_skeleton".into(), boolean()),
            (
                "external_skeleton".into(),
                json!({"type":"string","maxLength":240}),
            ),
        ]),
        &[],
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

fn scalar_schema() -> Value {
    json!({"anyOf":[
        {"type":"boolean"},
        {"type":"integer"},
        {"type":"number"},
        {"type":"string","maxLength":4096}
    ]})
}
fn res_path_schema() -> Value {
    json!({"type":"string","minLength":7,"maxLength":240,"pattern":"^res://"})
}
fn translation_path_schema() -> Value {
    json!({"type":"string","minLength":19,"maxLength":240,"pattern":"^res://.*\\.translation$"})
}
fn project_window_configure_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("viewport_width".into(), bounded_int(1, 16384)),
            ("viewport_height".into(), bounded_int(1, 16384)),
            ("window_width_override".into(), bounded_int(0, 16384)),
            ("window_height_override".into(), bounded_int(0, 16384)),
            ("mode".into(), bounded_int(0, 4)),
            ("resizable".into(), boolean()),
            ("borderless".into(), boolean()),
            ("always_on_top".into(), boolean()),
            (
                "stretch_mode".into(),
                json!({"type":"string","maxLength":64}),
            ),
            (
                "stretch_aspect".into(),
                json!({"type":"string","maxLength":64}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "expect", "dry_run"],
    )
}
fn project_rendering_configure_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            (
                "rendering_method".into(),
                json!({"type":"string","maxLength":64}),
            ),
            (
                "rendering_method_mobile".into(),
                json!({"type":"string","maxLength":64}),
            ),
            ("msaa_2d".into(), bounded_int(0, 4)),
            ("msaa_3d".into(), bounded_int(0, 4)),
            ("taa".into(), boolean()),
            ("use_debanding".into(), boolean()),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "expect", "dry_run"],
    )
}
fn project_physics_configure_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("ticks_per_second".into(), bounded_int(1, 1000)),
            ("max_steps_per_frame".into(), bounded_int(1, 64)),
            ("jitter_fix".into(), bounded_number(0.0, 1.0)),
            ("gravity_2d".into(), bounded_number(0.0, 100000.0)),
            ("gravity_3d".into(), bounded_number(0.0, 100000.0)),
            ("gravity_vector_3d".into(), vec3_schema()),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "expect", "dry_run"],
    )
}
fn project_layer_set_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            (
                "kind".into(),
                json!({"enum":["2d_render","2d_physics","3d_render","3d_physics","navigation"]}),
            ),
            ("index".into(), bounded_int(1, 32)),
            ("name".into(), json!({"type":"string","maxLength":96})),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "kind", "index", "name", "expect", "dry_run"],
    )
}
fn autoload_add_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            (
                "name".into(),
                json!({"type":"string","minLength":1,"maxLength":96,"pattern":"^[A-Za-z_][A-Za-z0-9_]*$"}),
            ),
            ("path".into(), res_path_schema()),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "name", "path", "expect", "dry_run"],
    )
}
fn autoload_remove_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            (
                "name".into(),
                json!({"type":"string","minLength":1,"maxLength":96,"pattern":"^[A-Za-z_][A-Za-z0-9_]*$"}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "name", "expect", "dry_run"],
    )
}
fn asset_import_configure_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("path".into(), res_path_schema()),
            (
                "params".into(),
                json!({"type":"array","minItems":1,"maxItems":64,"items":{
                    "type":"object","additionalProperties":false,"required":["key","value"],
                    "properties":{"key":{"type":"string","minLength":1,"maxLength":160},"value":scalar_schema()}
                }}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "path", "params", "expect", "dry_run"],
    )
}
fn export_preset_inspect_in() -> Value {
    object(
        Map::from_iter([session_prop(), ("name".into(), string(128))]),
        &["session", "name"],
    )
}
fn export_preset_configure_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("name".into(), string(128)),
            (
                "export_path".into(),
                json!({"type":"string","maxLength":240}),
            ),
            ("runnable".into(), boolean()),
            (
                "custom_features".into(),
                json!({"type":"string","maxLength":1024}),
            ),
            (
                "export_filter".into(),
                json!({"enum":["all_resources","scenes","resources","exclude","customized"]}),
            ),
            (
                "include_filter".into(),
                json!({"type":"string","maxLength":2048}),
            ),
            (
                "exclude_filter".into(),
                json!({"type":"string","maxLength":2048}),
            ),
            ("dedicated_server".into(), boolean()),
            (
                "options".into(),
                json!({"type":"array","maxItems":64,"items":{
                    "type":"object","additionalProperties":false,"required":["key","value"],
                    "properties":{"key":{"type":"string","minLength":1,"maxLength":160},"value":scalar_schema()}
                }}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "name", "expect", "dry_run"],
    )
}
fn localization_configure_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("fallback".into(), json!({"type":"string","maxLength":32})),
            (
                "test_locale".into(),
                json!({"type":"string","maxLength":32}),
            ),
            (
                "translations".into(),
                json!({"type":"array","maxItems":256,"uniqueItems":true,"items":res_path_schema()}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "expect", "dry_run"],
    )
}
fn translation_create_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("path".into(), translation_path_schema()),
            (
                "locale".into(),
                json!({"type":"string","minLength":1,"maxLength":32}),
            ),
            ("register".into(), boolean()),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "path", "locale", "register", "expect", "dry_run"],
    )
}
fn translation_message_set_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("path".into(), res_path_schema()),
            ("source".into(), string(4096)),
            (
                "translation".into(),
                json!({"type":"string","maxLength":16384}),
            ),
            ("context".into(), json!({"type":"string","maxLength":512})),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session",
            "path",
            "source",
            "translation",
            "expect",
            "dry_run",
        ],
    )
}
fn translation_message_remove_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("path".into(), res_path_schema()),
            ("source".into(), string(4096)),
            ("context".into(), json!({"type":"string","maxLength":512})),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "path", "source", "expect", "dry_run"],
    )
}
fn animation_tree_read_in() -> Value {
    object(
        Map::from_iter([session_prop(), ("tree".into(), string(240))]),
        &["session", "tree"],
    )
}
fn animation_tree_state_add_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("tree".into(), string(240)),
            ("name".into(), string(96)),
            (
                "kind".into(),
                json!({"enum":["animation","state_machine","blend_tree","blend_space_1d","blend_space_2d","one_shot","transition","time_scale","sync"]}),
            ),
            ("position".into(), vec2_schema()),
            ("animation".into(), json!({"type":"string","maxLength":96})),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session", "tree", "name", "kind", "position", "expect", "dry_run",
        ],
    )
}
fn animation_tree_state_target_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("tree".into(), string(240)),
            ("name".into(), string(96)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "tree", "name", "expect", "dry_run"],
    )
}
fn animation_tree_transition_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("tree".into(), string(240)),
            ("from".into(), string(96)),
            ("to".into(), string(96)),
            (
                "advance_condition".into(),
                json!({"type":"string","maxLength":96}),
            ),
            (
                "advance_expression".into(),
                json!({"type":"string","maxLength":1024}),
            ),
            ("advance_mode".into(), bounded_int(0, 2)),
            ("priority".into(), bounded_int(0, 64)),
            ("reset".into(), boolean()),
            ("switch_mode".into(), bounded_int(0, 2)),
            ("xfade_time".into(), bounded_number(0.0, 60.0)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "tree", "from", "to", "expect", "dry_run"],
    )
}
fn animation_tree_transition_target_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("tree".into(), string(240)),
            ("from".into(), string(96)),
            ("to".into(), string(96)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "tree", "from", "to", "expect", "dry_run"],
    )
}
fn animation_tree_parameter_set_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("tree".into(), string(240)),
            ("parameter".into(), string(240)),
            ("value".into(), godot_value_schema()),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "tree", "parameter", "value", "expect", "dry_run"],
    )
}

fn animation_graph_read_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("tree".into(), string(240)),
            ("graph".into(), json!({"type":"string","maxLength":1024})),
        ]),
        &["session", "tree", "graph"],
    )
}

fn animation_graph_mutation_schema(
    mut extra: Map<String, Value>,
    required_extra: &[&str],
) -> Value {
    extra.insert("session".into(), hex_string(32));
    extra.insert("tree".into(), string(240));
    extra.insert("graph".into(), json!({"type":"string","maxLength":1024}));
    extra.insert("expect".into(), stamp());
    extra.insert("dry_run".into(), boolean());
    let mut required = vec!["session", "tree", "graph"];
    required.extend_from_slice(required_extra);
    required.extend_from_slice(&["expect", "dry_run"]);
    object(extra, &required)
}

fn animation_graph_node_kind_schema() -> Value {
    json!({"enum":[
        "animation","state_machine","blend_tree","blend_space_1d","blend_space_2d",
        "one_shot","transition","time_scale","time_seek","blend2","blend3",
        "add2","add3","sub2"
    ]})
}

fn animation_root_node_kind_schema() -> Value {
    json!({"enum":[
        "animation","state_machine","blend_tree","blend_space_1d","blend_space_2d"
    ]})
}

fn blend_position_schema() -> Value {
    json!({"oneOf":[
        {"type":"number","minimum":-1000000.0,"maximum":1000000.0},
        vec2_schema()
    ]})
}

fn animation_graph_node_properties() -> Map<String, Value> {
    Map::from_iter([
        ("animation".into(), json!({"type":"string","maxLength":96})),
        ("sync".into(), boolean()),
        ("fadein_time".into(), bounded_number(0.0, 60.0)),
        ("fadeout_time".into(), bounded_number(0.0, 60.0)),
        ("mix_mode".into(), bounded_int(0, 1)),
        ("autorestart".into(), boolean()),
        ("autorestart_delay".into(), bounded_number(0.0, 3600.0)),
        (
            "autorestart_random_delay".into(),
            bounded_number(0.0, 3600.0),
        ),
        ("abort_on_reset".into(), boolean()),
        ("break_loop_at_end".into(), boolean()),
        ("input_count".into(), bounded_int(0, 64)),
        ("xfade_time".into(), bounded_number(0.0, 60.0)),
        ("allow_transition_to_self".into(), boolean()),
        (
            "transition_input".into(),
            object(
                Map::from_iter([
                    ("index".into(), bounded_int(0, 63)),
                    ("auto_advance".into(), boolean()),
                    ("reset".into(), boolean()),
                    ("break_loop_at_end".into(), boolean()),
                ]),
                &["index"],
            ),
        ),
        ("graph_offset".into(), vec2_schema()),
        ("blend_mode".into(), bounded_int(0, 2)),
        ("sync_mode".into(), bounded_int(0, 2)),
        ("cyclic_length".into(), bounded_number(0.0, 1000000.0)),
        ("min_space".into(), blend_position_schema()),
        ("max_space".into(), blend_position_schema()),
        ("snap".into(), blend_position_schema()),
        (
            "value_label".into(),
            json!({"type":"string","maxLength":96}),
        ),
        ("x_label".into(), json!({"type":"string","maxLength":96})),
        ("y_label".into(), json!({"type":"string","maxLength":96})),
        ("auto_triangles".into(), boolean()),
    ])
}

fn animation_graph_node_configure_in() -> Value {
    let mut props = animation_graph_node_properties();
    props.insert("graph_position".into(), vec2_schema());
    animation_graph_mutation_schema(props, &[])
}

fn animation_blend_tree_node_add_in() -> Value {
    let mut props = animation_graph_node_properties();
    props.insert("name".into(), string(96));
    props.insert("kind".into(), animation_graph_node_kind_schema());
    props.insert("graph_position".into(), vec2_schema());
    animation_graph_mutation_schema(props, &["name", "kind", "graph_position"])
}

fn animation_blend_tree_node_configure_in() -> Value {
    let mut props = animation_graph_node_properties();
    props.insert("name".into(), string(96));
    props.insert("new_name".into(), json!({"type":"string","maxLength":96}));
    props.insert("graph_position".into(), vec2_schema());
    animation_graph_mutation_schema(props, &["name"])
}

fn animation_blend_tree_node_target_in() -> Value {
    animation_graph_mutation_schema(Map::from_iter([("name".into(), string(96))]), &["name"])
}

fn animation_blend_tree_connection_in() -> Value {
    animation_graph_mutation_schema(
        Map::from_iter([
            ("input_node".into(), string(96)),
            ("input_index".into(), bounded_int(0, 63)),
            ("output_node".into(), string(96)),
            ("connected".into(), boolean()),
        ]),
        &["input_node", "input_index", "output_node", "connected"],
    )
}

fn animation_blend_space_configure_in() -> Value {
    animation_graph_mutation_schema(animation_graph_node_properties(), &[])
}

fn animation_blend_space_point_add_in() -> Value {
    let mut props = animation_graph_node_properties();
    props.insert("name".into(), string(96));
    props.insert("kind".into(), animation_root_node_kind_schema());
    props.insert("index".into(), bounded_int(-1, 255));
    props.insert("position".into(), blend_position_schema());
    animation_graph_mutation_schema(props, &["name", "kind", "position"])
}

fn animation_blend_space_point_configure_in() -> Value {
    let mut props = animation_graph_node_properties();
    props.insert("index".into(), bounded_int(0, 255));
    props.insert("name".into(), json!({"type":"string","maxLength":96}));
    props.insert("position".into(), blend_position_schema());
    animation_graph_mutation_schema(props, &["index"])
}

fn animation_blend_space_point_target_in() -> Value {
    animation_graph_mutation_schema(
        Map::from_iter([("index".into(), bounded_int(0, 255))]),
        &["index"],
    )
}

fn animation_blend_space_triangle_add_in() -> Value {
    animation_graph_mutation_schema(
        Map::from_iter([
            ("a".into(), bounded_int(0, 255)),
            ("b".into(), bounded_int(0, 255)),
            ("c".into(), bounded_int(0, 255)),
            ("index".into(), bounded_int(-1, 1023)),
        ]),
        &["a", "b", "c"],
    )
}

fn animation_blend_space_triangle_target_in() -> Value {
    animation_graph_mutation_schema(
        Map::from_iter([("index".into(), bounded_int(0, 1023))]),
        &["index"],
    )
}

fn multiplayer_spawner_configure_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            (
                "spawn_path".into(),
                json!({"type":"string","maxLength":240}),
            ),
            ("spawn_limit".into(), bounded_int(0, 100000)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "target", "expect", "dry_run"],
    )
}
fn multiplayer_spawner_scene_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            ("scene".into(), res_path_schema()),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "target", "scene", "expect", "dry_run"],
    )
}
fn multiplayer_synchronizer_configure_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            ("root_path".into(), json!({"type":"string","maxLength":240})),
            ("replication_interval".into(), bounded_number(0.0, 3600.0)),
            ("delta_interval".into(), bounded_number(0.0, 3600.0)),
            ("public_visibility".into(), boolean()),
            ("visibility_update_mode".into(), bounded_int(0, 2)),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "target", "expect", "dry_run"],
    )
}
fn multiplayer_replication_property_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("target".into(), string(240)),
            ("path".into(), string(240)),
            ("spawn".into(), boolean()),
            ("mode".into(), bounded_int(0, 2)),
            expect_prop(),
            dry_prop(),
        ]),
        &[
            "session", "target", "path", "spawn", "mode", "expect", "dry_run",
        ],
    )
}
fn multiplayer_replication_property_target_in() -> Value {
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
fn editor_selection_set_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            (
                "nodes".into(),
                json!({"type":"array","maxItems":256,"uniqueItems":true,"items":{"type":"string","maxLength":240}}),
            ),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "nodes", "expect", "dry_run"],
    )
}
fn editor_run_start_in() -> Value {
    object(
        Map::from_iter([
            session_prop(),
            ("mode".into(), json!({"enum":["main","current","custom"]})),
            ("scene".into(), json!({"type":"string","maxLength":240})),
            expect_prop(),
            dry_prop(),
        ]),
        &["session", "mode", "expect", "dry_run"],
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
