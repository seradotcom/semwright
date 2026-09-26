//! Transactional semantic edits operate on a private model clone.
use crate::{
    Error, Result,
    model::*,
    refs::{Kind, Reference},
    security, validate,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    SettingsPatch {
        #[schemars(with = "SettingsPatchSchema")]
        patch: Value,
    },
    ThemePatch {
        #[schemars(with = "ThemePatchSchema")]
        patch: Value,
    },
    VariableSet {
        name: String,
        value: SemanticValue,
    },
    VariableRemove {
        name: String,
    },
    SceneCreate {
        scene: Scene,
    },
    ScenePatch {
        scene_ref: String,
        name: Option<String>,
        duration_ms: Option<u64>,
        transition: Option<Transition>,
        #[serde(default)]
        clear_transition: bool,
    },
    SceneDuplicate {
        scene_ref: String,
        new_id: String,
        name: String,
    },
    SceneRemove {
        scene_ref: String,
    },
    SceneReorder {
        order: Vec<String>,
    },
    NodeCreate {
        scene_ref: String,
        node: Node,
    },
    NodePatch {
        node_ref: String,
        #[schemars(with = "Properties")]
        patch: Value,
        name: Option<String>,
    },
    NodeRemove {
        node_ref: String,
        #[serde(default)]
        cascade: bool,
    },
    NodeReparent {
        node_ref: String,
        parent_ref: Option<String>,
    },
    NodeReorder {
        scene_ref: String,
        parent_ref: Option<String>,
        order: Vec<String>,
    },
    AnimationAdd {
        scene_ref: String,
        animation: Animation,
    },
    AnimationPatch {
        animation_ref: String,
        animation: Animation,
    },
    AnimationRemove {
        animation_ref: String,
    },
    AnimationGroup {
        scene_ref: String,
        animations: Vec<Animation>,
        mode: GroupMode,
        #[serde(default)]
        at: TimeAnchor,
        #[serde(default)]
        stagger_ms: u64,
    },
    CueUpsert {
        scene_ref: String,
        cue: Cue,
    },
    CueRemove {
        cue_ref: String,
    },
    AudioSet {
        tracks: Vec<AudioTrack>,
    },
    AssetRemove {
        asset_ref: String,
    },
    CodeHighlight {
        node_ref: String,
        selection: Option<CodeSelection>,
    },
    CameraFocus {
        camera_ref: String,
        target_ref: String,
        id: String,
        #[serde(default)]
        at: TimeAnchor,
        duration_ms: u64,
    },
    DiagramEdgeCreate {
        scene_ref: String,
        id: String,
        from_ref: String,
        to_ref: String,
        #[serde(default)]
        properties: Properties,
    },
    ComponentCreate {
        scene_ref: String,
        id: String,
        component: Component,
        text: String,
        parent_ref: Option<String>,
        asset: Option<String>,
        #[serde(default)]
        properties: Properties,
    },
    AnimationPreset {
        node_ref: String,
        id: String,
        preset: AnimationPreset,
        #[serde(default)]
        at: TimeAnchor,
        duration_ms: u64,
        #[serde(default)]
        from_number: f64,
        #[serde(default)]
        to_number: f64,
    },
}
impl Operation {
    pub fn tag(&self) -> &'static str {
        match self {
            Self::SettingsPatch { .. } => "settings_patch",
            Self::ThemePatch { .. } => "theme_patch",
            Self::VariableSet { .. } => "variable_set",
            Self::VariableRemove { .. } => "variable_remove",
            Self::SceneCreate { .. } => "scene_create",
            Self::ScenePatch { .. } => "scene_patch",
            Self::SceneDuplicate { .. } => "scene_duplicate",
            Self::SceneRemove { .. } => "scene_remove",
            Self::SceneReorder { .. } => "scene_reorder",
            Self::NodeCreate { .. } => "node_create",
            Self::NodePatch { .. } => "node_patch",
            Self::NodeRemove { .. } => "node_remove",
            Self::NodeReparent { .. } => "node_reparent",
            Self::NodeReorder { .. } => "node_reorder",
            Self::AnimationAdd { .. } => "animation_add",
            Self::AnimationPatch { .. } => "animation_patch",
            Self::AnimationRemove { .. } => "animation_remove",
            Self::AnimationGroup { .. } => "animation_group",
            Self::CueUpsert { .. } => "cue_upsert",
            Self::CueRemove { .. } => "cue_remove",
            Self::AudioSet { .. } => "audio_set",
            Self::AssetRemove { .. } => "asset_remove",
            Self::CodeHighlight { .. } => "code_highlight",
            Self::CameraFocus { .. } => "camera_focus",
            Self::DiagramEdgeCreate { .. } => "diagram_edge_create",
            Self::ComponentCreate { .. } => "component_create",
            Self::AnimationPreset { .. } => "animation_preset",
        }
    }
}
fn invalid(message: &str) -> Error {
    Error::invalid(message)
}
fn index(project: &Project, fingerprint: &str, value: &str, kind: Kind) -> Result<String> {
    let reference = Reference::decode(value)?;
    reference.check(project, fingerprint, kind)?;
    Ok(reference.id)
}

/// Partial root schemas: the existing full Settings/Theme annotations would make
/// their required fields mandatory even for a one-field patch. Runtime validation
/// still applies the complete Settings/Theme invariants after replacement.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SettingsPatchSchema {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub background: Option<String>,
    pub color_space: Option<ColorSpace>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ThemePatchSchema {
    pub font_family: Option<String>,
    pub mono_family: Option<String>,
    pub font_size: Option<f64>,
    pub font_weight: Option<u16>,
    pub spacing: Option<f64>,
    pub line_width: Option<f64>,
    pub radius: Option<f64>,
    pub colors: Option<BTreeMap<String, String>>,
}

/// Transaction requests deliberately have a tighter operation budget than projects.
/// The caller must also bound the incoming protocol frame before deserializing it.
pub const MAX_TRANSACTION_OPERATIONS: usize = 128;
const MAX_PRESET_STEPS: usize = 128;

/// An in-memory transaction result. This is NOT an on-disk commit or proof that
/// generated TypeScript has been typechecked. The store/driver must do both.
#[derive(Debug, Clone)]
pub struct PreparedTransaction {
    pub project: Project,
    pub diff: crate::diff::SemanticDiff,
    pub generated: crate::compiler::Generated,
    pub render_invalidated: bool,
}

/// Prepare a transaction against one already-loaded, validated snapshot.
///
/// `fingerprint` is the store's hash of the exact source bytes it loaded. The
/// store must compare it again under its write lock immediately before commit.
/// This function never opens files and cannot by itself detect a disk race.
///
/// Every object reference is resolved against the ORIGINAL snapshot. New objects
/// may be supplied as a complete scene/tree, but cannot be addressed by a made-up
/// reference later in the same transaction. Removed identities cannot be reused
/// in this transaction. A semantic no-op does not advance the revision.
///
/// Errors leave the input unchanged. Use this same preparation path for dry-run;
/// only the store decides whether to commit the returned model and derived tree.
pub fn prepare(
    original: &Project,
    fingerprint: &str,
    operations: &[Operation],
) -> Result<PreparedTransaction> {
    let project = apply(original, fingerprint, operations)?;
    let diff = crate::diff::between(original, &project)?;
    let generated = crate::compiler::compile(&project)?;
    let render_invalidated = !diff.is_empty();
    Ok(PreparedTransaction {
        project,
        diff,
        generated,
        render_invalidated,
    })
}

/// Apply and validate semantic operations on a private clone, without codegen or
/// persistence. `prepare` is the normal pre-commit/dry-run entry point.
pub fn apply(original: &Project, fingerprint: &str, operations: &[Operation]) -> Result<Project> {
    validate::project_valid(original)?;
    if !security::digest(fingerprint)
        || operations.len() > MAX_TRANSACTION_OPERATIONS
        || serde_json::to_vec(operations)?.len() > MAX_PROJECT_BYTES
    {
        return Err(invalid(
            "Transaction fingerprint or operation budget is invalid",
        ));
    }
    let mut project = original.clone();
    let mut identities = Identities::from_project(original);
    for operation in operations {
        apply_one(
            &mut project,
            original,
            fingerprint,
            operation,
            &mut identities,
        )?;
        // Intermediate states must be valid too. This prevents an invalid/deep
        // graph from reaching any later operation's traversal or clone logic.
        validate::project_valid(&project)?;
    }
    if !crate::diff::between(original, &project)?.is_empty() {
        project.revision = original
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("Project revision is exhausted"))?;
    }
    validate::project_valid(&project)?;
    Ok(project)
}

#[derive(Default)]
struct Identities {
    scenes: BTreeSet<String>,
    nodes: BTreeSet<String>,
    animations: BTreeSet<String>,
    cues: BTreeSet<String>,
}
impl Identities {
    fn from_project(project: &Project) -> Self {
        Self {
            scenes: project.scenes.iter().map(|s| s.id.clone()).collect(),
            nodes: project
                .scenes
                .iter()
                .flat_map(|s| s.nodes.iter().map(|n| n.id.clone()))
                .collect(),
            animations: project
                .scenes
                .iter()
                .flat_map(|s| s.animations.iter().map(|a| a.id.clone()))
                .collect(),
            cues: project
                .scenes
                .iter()
                .flat_map(|s| s.cues.iter().map(|c| c.id.clone()))
                .collect(),
        }
    }
    fn reserve(seen: &mut BTreeSet<String>, id: &str) -> Result<()> {
        if !security::identifier(id) || !seen.insert(id.to_owned()) {
            return Err(invalid(
                "Invalid, duplicate, or reused transaction identity",
            ));
        }
        Ok(())
    }
    fn scene(&mut self, scene: &Scene) -> Result<()> {
        Self::reserve(&mut self.scenes, &scene.id)?;
        for node in &scene.nodes {
            Self::reserve(&mut self.nodes, &node.id)?;
        }
        for animation in &scene.animations {
            Self::reserve(&mut self.animations, &animation.id)?;
        }
        for cue in &scene.cues {
            Self::reserve(&mut self.cues, &cue.id)?;
        }
        Ok(())
    }
}

fn missing() -> Error {
    Error::new(
        crate::ErrorCode::StaleReference,
        "Referenced object was removed earlier in this transaction",
    )
}
fn scene_index(project: &Project, id: &str) -> Result<usize> {
    project
        .scenes
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(missing)
}
fn node_index(project: &Project, id: &str) -> Result<(usize, usize)> {
    project
        .scenes
        .iter()
        .enumerate()
        .find_map(|(si, scene)| {
            scene
                .nodes
                .iter()
                .position(|n| n.id == id)
                .map(|ni| (si, ni))
        })
        .ok_or_else(missing)
}
fn animation_index(project: &Project, id: &str) -> Result<(usize, usize)> {
    project
        .scenes
        .iter()
        .enumerate()
        .find_map(|(si, scene)| {
            scene
                .animations
                .iter()
                .position(|a| a.id == id)
                .map(|ai| (si, ai))
        })
        .ok_or_else(missing)
}
fn cue_index(project: &Project, id: &str) -> Result<(usize, usize)> {
    project
        .scenes
        .iter()
        .enumerate()
        .find_map(|(si, scene)| {
            scene
                .cues
                .iter()
                .position(|c| c.id == id)
                .map(|ci| (si, ci))
        })
        .ok_or_else(missing)
}

/// Shallow typed field replacement. Nested layouts/color maps are replaced as
/// complete values, not recursively merged. Null resets optional fields. Keeping
/// unknown null-valued keys until deserialization ensures they are rejected too.
fn patched<T: Serialize + DeserializeOwned>(original: &T, patch: &Value) -> Result<T> {
    let fields = patch
        .as_object()
        .ok_or_else(|| invalid("Patch must be an object"))?;
    if fields.len() > 64 || serde_json::to_vec(patch)?.len() > MAX_PROJECT_BYTES {
        return Err(invalid("Patch exceeds bounds"));
    }
    let mut value = serde_json::to_value(original)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| invalid("Patch target must be an object"))?;
    for (key, field) in fields {
        object.insert(key.clone(), field.clone());
    }
    Ok(serde_json::from_value(value)?)
}

fn exact_order(actual: impl Iterator<Item = String>, requested: &[String]) -> Result<()> {
    let actual = actual.collect::<BTreeSet<_>>();
    let desired = requested.iter().cloned().collect::<BTreeSet<_>>();
    if desired.len() != requested.len() || actual != desired {
        return Err(invalid(
            "Reorder must contain every current sibling exactly once",
        ));
    }
    Ok(())
}
fn child_id(scope: &str, kind: &str, old: &str) -> String {
    // JSON array serialization makes the tuple encoding unambiguous.
    let encoded = serde_json::to_vec(&(scope, kind, old))
        .expect("serializing a tuple of strings is infallible");
    format!("{kind}_{}", &security::sha256(&encoded)[..32])
}
fn mapped(map: &BTreeMap<String, String>, id: &str) -> Result<String> {
    map.get(id)
        .cloned()
        .ok_or_else(|| invalid("Duplicate contains an unresolved internal identity"))
}
fn duplicate_scene(source: &Scene, id: &str, name: &str) -> Result<Scene> {
    if !security::identifier(id) {
        return Err(invalid("Invalid duplicate scene identity"));
    }
    let nodes = source
        .nodes
        .iter()
        .map(|n| (n.id.clone(), child_id(id, "node", &n.id)))
        .collect::<BTreeMap<_, _>>();
    let cues = source
        .cues
        .iter()
        .map(|c| (c.id.clone(), child_id(id, "cue", &c.id)))
        .collect::<BTreeMap<_, _>>();
    let mut scene = source.clone();
    scene.id = id.into();
    scene.name = name.into();
    for node in &mut scene.nodes {
        node.id = mapped(&nodes, &node.id)?;
        node.parent = node
            .parent
            .as_deref()
            .map(|v| mapped(&nodes, v))
            .transpose()?;
        if let Some(edge) = &mut node.properties.edge {
            edge.from = mapped(&nodes, &edge.from)?;
            edge.to = mapped(&nodes, &edge.to)?;
        }
    }
    for cue in &mut scene.cues {
        cue.id = mapped(&cues, &cue.id)?;
    }
    for animation in &mut scene.animations {
        animation.id = child_id(id, "animation", &animation.id);
        animation.target = mapped(&nodes, &animation.target)?;
        animation.at.cue = animation
            .at
            .cue
            .as_deref()
            .map(|v| mapped(&cues, v))
            .transpose()?;
        animation.duration_cue = animation
            .duration_cue
            .as_deref()
            .map(|v| mapped(&cues, v))
            .transpose()?;
        if animation.property == AnimatedProperty::CameraFocus {
            let AnimatedValue::Text(target) = &animation.to else {
                return Err(invalid("Invalid camera focus"));
            };
            animation.to = AnimatedValue::Text(mapped(&nodes, target)?);
        }
    }
    Ok(scene)
}
fn time_anchor(scene: &Scene, at: &TimeAnchor) -> Result<u64> {
    let cue = at
        .cue
        .as_ref()
        .map(|id| {
            scene
                .cues
                .iter()
                .find(|c| &c.id == id)
                .map(|c| c.time_ms)
                .ok_or_else(|| invalid("Unknown group cue"))
        })
        .transpose()?
        .unwrap_or(0);
    let start = i128::from(cue) + i128::from(at.offset_ms);
    if !(0..=i128::from(scene.duration_ms)).contains(&start) {
        return Err(invalid("Group anchor exceeds scene duration"));
    }
    Ok(start as u64)
}
fn remove_node(scene: &mut Scene, id: &str, cascade: bool) -> Result<()> {
    let mut removed = BTreeSet::from([id.to_string()]);
    if cascade {
        loop {
            let mut added = Vec::new();
            for node in &scene.nodes {
                let parent_removed = node.parent.as_ref().is_some_and(|p| removed.contains(p));
                let endpoint_removed = node
                    .properties
                    .edge
                    .as_ref()
                    .is_some_and(|e| removed.contains(&e.from) || removed.contains(&e.to));
                if !removed.contains(&node.id) && (parent_removed || endpoint_removed) {
                    added.push(node.id.clone());
                }
            }
            if added.is_empty() {
                break;
            }
            removed.extend(added);
            if removed.len() > MAX_NODES {
                return Err(invalid("Cascade exceeds node budget"));
            }
        }
    }
    let affected_animation = |a: &Animation| {
        removed.contains(&a.target)
            || (a.property == AnimatedProperty::CameraFocus
                && matches!(&a.to, AnimatedValue::Text(id) if removed.contains(id)))
    };
    if !cascade
        && (scene.nodes.iter().any(|n| {
            n.parent.as_ref().is_some_and(|p| removed.contains(p))
                || n.properties
                    .edge
                    .as_ref()
                    .is_some_and(|e| removed.contains(&e.from) || removed.contains(&e.to))
        }) || scene.animations.iter().any(affected_animation))
    {
        return Err(invalid(
            "Node has dependent children, edges or animations; explicit cascade is required",
        ));
    }
    scene.animations.retain(|a| !affected_animation(a));
    scene.nodes.retain(|n| !removed.contains(&n.id));
    Ok(())
}

fn apply_one(
    project: &mut Project,
    original: &Project,
    fingerprint: &str,
    operation: &Operation,
    identities: &mut Identities,
) -> Result<()> {
    let resolve = |value: &str, kind| index(original, fingerprint, value, kind);
    match operation {
        Operation::SettingsPatch { patch } => project.settings = patched(&project.settings, patch)?,
        Operation::ThemePatch { patch } => project.theme = patched(&project.theme, patch)?,
        Operation::VariableSet { name, value } => {
            if !security::identifier(name) {
                return Err(invalid("Project variable name is not canonical"));
            }
            project.variables.insert(name.clone(), value.clone());
        }
        Operation::VariableRemove { name } => {
            if project.variables.remove(name).is_none() {
                return Err(invalid("Project variable does not exist"));
            }
        }
        Operation::SceneCreate { scene } => {
            identities.scene(scene)?;
            project.scenes.push(scene.clone());
        }
        Operation::ScenePatch {
            scene_ref,
            name,
            duration_ms,
            transition,
            clear_transition,
        } => {
            let id = resolve(scene_ref, Kind::Scene)?;
            let si = scene_index(project, &id)?;
            if *clear_transition && transition.is_some() {
                return Err(invalid("Cannot set and clear a transition together"));
            }
            let scene = &mut project.scenes[si];
            if let Some(v) = name {
                scene.name = v.clone();
            }
            if let Some(v) = duration_ms {
                scene.duration_ms = *v;
            }
            if let Some(v) = transition {
                scene.transition = Some(v.clone());
            }
            if *clear_transition {
                scene.transition = None;
            }
        }
        Operation::SceneDuplicate {
            scene_ref,
            new_id,
            name,
        } => {
            let id = resolve(scene_ref, Kind::Scene)?;
            let si = scene_index(project, &id)?;
            let duplicate = duplicate_scene(&project.scenes[si], new_id, name)?;
            identities.scene(&duplicate)?;
            project.scenes.insert(si + 1, duplicate);
        }
        Operation::SceneRemove { scene_ref } => {
            let id = resolve(scene_ref, Kind::Scene)?;
            let si = scene_index(project, &id)?;
            project.scenes.remove(si);
        }
        Operation::SceneReorder { order } => {
            exact_order(project.scenes.iter().map(|s| s.id.clone()), order)?;
            let ranks = order
                .iter()
                .enumerate()
                .map(|(rank, id)| (id.clone(), rank))
                .collect::<BTreeMap<_, _>>();
            project.scenes.sort_by_key(|s| ranks[&s.id]);
        }
        Operation::NodeCreate { scene_ref, node } => {
            let id = resolve(scene_ref, Kind::Scene)?;
            let si = scene_index(project, &id)?;
            Identities::reserve(&mut identities.nodes, &node.id)?;
            project.scenes[si].nodes.push(node.clone());
        }
        Operation::NodePatch {
            node_ref,
            patch,
            name,
        } => {
            let id = resolve(node_ref, Kind::Node)?;
            let (si, ni) = node_index(project, &id)?;
            let node = &mut project.scenes[si].nodes[ni];
            node.properties = patched(&node.properties, patch)?;
            if let Some(v) = name {
                node.name = v.clone();
            }
        }
        Operation::NodeRemove { node_ref, cascade } => {
            let id = resolve(node_ref, Kind::Node)?;
            let (si, _) = node_index(project, &id)?;
            remove_node(&mut project.scenes[si], &id, *cascade)?;
        }
        Operation::NodeReparent {
            node_ref,
            parent_ref,
        } => {
            let id = resolve(node_ref, Kind::Node)?;
            let (si, ni) = node_index(project, &id)?;
            let parent = parent_ref
                .as_deref()
                .map(|r| resolve(r, Kind::Node))
                .transpose()?;
            if let Some(parent) = &parent {
                let (psi, _) = node_index(project, parent)?;
                if psi != si {
                    return Err(invalid("Reparent cannot cross a scene boundary"));
                }
            }
            project.scenes[si].nodes[ni].parent = parent;
        }
        Operation::NodeReorder {
            scene_ref,
            parent_ref,
            order,
        } => {
            let id = resolve(scene_ref, Kind::Scene)?;
            let si = scene_index(project, &id)?;
            let parent = parent_ref
                .as_deref()
                .map(|r| resolve(r, Kind::Node))
                .transpose()?;
            if let Some(id) = &parent
                && node_index(project, id)?.0 != si
            {
                return Err(invalid("Reorder parent belongs to another scene"));
            }
            let scene = &mut project.scenes[si];
            exact_order(
                scene
                    .nodes
                    .iter()
                    .filter(|n| n.parent == parent)
                    .map(|n| n.id.clone()),
                order,
            )?;
            let mut siblings = scene
                .nodes
                .iter()
                .filter(|n| n.parent == parent)
                .map(|n| (n.id.clone(), n.clone()))
                .collect::<BTreeMap<_, _>>();
            let mut next = order.iter();
            for node in &mut scene.nodes {
                if node.parent == parent {
                    let id = next
                        .next()
                        .ok_or_else(|| invalid("Invalid sibling order"))?;
                    *node = siblings
                        .remove(id)
                        .ok_or_else(|| invalid("Missing sibling in reorder"))?;
                }
            }
        }
        Operation::AnimationAdd {
            scene_ref,
            animation,
        } => {
            let id = resolve(scene_ref, Kind::Scene)?;
            let si = scene_index(project, &id)?;
            Identities::reserve(&mut identities.animations, &animation.id)?;
            project.scenes[si].animations.push(animation.clone());
        }
        Operation::AnimationPatch {
            animation_ref,
            animation,
        } => {
            let id = resolve(animation_ref, Kind::Animation)?;
            let (si, ai) = animation_index(project, &id)?;
            if animation.id != id {
                return Err(invalid("Animation patch cannot replace identity"));
            }
            project.scenes[si].animations[ai] = animation.clone();
        }
        Operation::AnimationRemove { animation_ref } => {
            let id = resolve(animation_ref, Kind::Animation)?;
            let (si, ai) = animation_index(project, &id)?;
            project.scenes[si].animations.remove(ai);
        }
        Operation::AnimationGroup {
            scene_ref,
            animations,
            mode,
            at,
            stagger_ms,
        } => {
            let id = resolve(scene_ref, Kind::Scene)?;
            let si = scene_index(project, &id)?;
            if animations.is_empty()
                || animations.len() > MAX_TRANSACTION_OPERATIONS
                || *stagger_ms > MAX_SCENE_MS
            {
                return Err(invalid("Animation group exceeds bounds"));
            }
            let scene = &mut project.scenes[si];
            let start = time_anchor(scene, at)?;
            let mut cursor = start;
            for (i, input) in animations.iter().enumerate() {
                if input.at.cue.is_some() || input.at.offset_ms < 0 {
                    return Err(invalid(
                        "Grouped animations require a nonnegative relative offset, without a child cue",
                    ));
                }
                let relative = input.at.offset_ms as u64;
                let base = match mode {
                    GroupMode::Parallel => start.checked_add(
                        (i as u64)
                            .checked_mul(*stagger_ms)
                            .ok_or_else(|| invalid("Group time overflow"))?,
                    ),
                    GroupMode::Sequence => Some(cursor),
                }
                .ok_or_else(|| invalid("Group time overflow"))?;
                let begin = base
                    .checked_add(relative)
                    .ok_or_else(|| invalid("Group time overflow"))?;
                if begin > scene.duration_ms {
                    return Err(invalid("Grouped animation starts after scene end"));
                }
                let mut animation = input.clone();
                animation.at = TimeAnchor {
                    cue: None,
                    offset_ms: begin as i64,
                };
                let (_, end) = validate::animation_times(scene, &animation)?;
                cursor = end
                    .checked_add(*stagger_ms)
                    .ok_or_else(|| invalid("Group time overflow"))?;
                Identities::reserve(&mut identities.animations, &animation.id)?;
                scene.animations.push(animation);
            }
        }
        Operation::CueUpsert { scene_ref, cue } => {
            let id = resolve(scene_ref, Kind::Scene)?;
            let si = scene_index(project, &id)?;
            if let Some(ci) = project.scenes[si].cues.iter().position(|c| c.id == cue.id) {
                // Upsert cannot resurrect a cue removed earlier: that cue is not
                // in the current scene and reserve() below will reject its ID.
                project.scenes[si].cues[ci] = cue.clone();
            } else {
                Identities::reserve(&mut identities.cues, &cue.id)?;
                project.scenes[si].cues.push(cue.clone());
            }
        }
        Operation::CueRemove { cue_ref } => {
            let id = resolve(cue_ref, Kind::Cue)?;
            let (si, ci) = cue_index(project, &id)?;
            project.scenes[si].cues.remove(ci);
            // Any dependent animation makes project_valid fail, rolling back
            // this entire transaction. Remove dependencies explicitly first.
        }
        Operation::AudioSet { tracks } => project.audio = tracks.clone(),
        Operation::AssetRemove { asset_ref } => {
            let id = resolve(asset_ref, Kind::Asset)?;
            let ai = project
                .assets
                .iter()
                .position(|a| a.id == id)
                .ok_or_else(missing)?;
            project.assets.remove(ai);
        }
        Operation::CodeHighlight {
            node_ref,
            selection,
        } => {
            let id = resolve(node_ref, Kind::Node)?;
            let (si, ni) = node_index(project, &id)?;
            let node = &mut project.scenes[si].nodes[ni];
            if node.kind != NodeKind::Code {
                return Err(invalid("Code selection requires a Code node"));
            }
            node.properties.selection = selection.clone();
        }
        Operation::CameraFocus {
            camera_ref,
            target_ref,
            id,
            at,
            duration_ms,
        } => {
            let camera = resolve(camera_ref, Kind::Node)?;
            let target = resolve(target_ref, Kind::Node)?;
            let (si, ni) = node_index(project, &camera)?;
            if node_index(project, &target)?.0 != si
                || project.scenes[si].nodes[ni].kind != NodeKind::Camera
            {
                return Err(invalid(
                    "Camera focus requires a camera and a target in the same scene",
                ));
            }
            Identities::reserve(&mut identities.animations, id)?;
            project.scenes[si].animations.push(Animation {
                id: id.clone(),
                target: camera,
                property: AnimatedProperty::CameraFocus,
                from: None,
                to: AnimatedValue::Text(target),
                at: at.clone(),
                duration_ms: *duration_ms,
                duration_cue: None,
                easing: Easing::default(),
            });
        }
        Operation::DiagramEdgeCreate {
            scene_ref,
            id,
            from_ref,
            to_ref,
            properties,
        } => {
            let scene_id = resolve(scene_ref, Kind::Scene)?;
            let si = scene_index(project, &scene_id)?;
            let from = resolve(from_ref, Kind::Node)?;
            let to = resolve(to_ref, Kind::Node)?;
            let (fsi, fi) = node_index(project, &from)?;
            let (tsi, ti) = node_index(project, &to)?;
            if fsi != si || tsi != si || properties.points.is_some() || properties.edge.is_some() {
                return Err(invalid(
                    "Diagram endpoints must belong to this scene; fixed points/edge overrides are not accepted",
                ));
            }
            let parent = project.scenes[si].nodes[fi].parent.clone();
            if project.scenes[si].nodes[ti].parent != parent {
                return Err(invalid("Diagram endpoints must share a parent"));
            }
            let mut properties = properties.clone();
            properties.edge = Some(Edge { from, to });
            properties.end_arrow.get_or_insert(true);
            properties.stroke.get_or_insert_with(|| "@ink".into());
            properties
                .stroke_width
                .get_or_insert(project.theme.line_width);
            Identities::reserve(&mut identities.nodes, id)?;
            project.scenes[si].nodes.push(Node {
                id: id.clone(),
                name: id.clone(),
                kind: NodeKind::Line,
                parent,
                properties,
            });
        }
        Operation::ComponentCreate {
            scene_ref,
            id,
            component,
            text,
            parent_ref,
            asset,
            properties,
        } => {
            let scene_id = resolve(scene_ref, Kind::Scene)?;
            let si = scene_index(project, &scene_id)?;
            let parent = parent_ref
                .as_deref()
                .map(|r| resolve(r, Kind::Node))
                .transpose()?;
            if let Some(id) = &parent
                && node_index(project, id)?.0 != si
            {
                return Err(invalid("Component parent belongs to another scene"));
            }
            let nodes = component_nodes(
                &project.theme,
                id,
                *component,
                text,
                parent,
                asset.as_deref(),
                properties,
            )?;
            for node in &nodes {
                Identities::reserve(&mut identities.nodes, &node.id)?;
            }
            project.scenes[si].nodes.extend(nodes);
        }
        Operation::AnimationPreset {
            node_ref,
            id,
            preset,
            at,
            duration_ms,
            from_number,
            to_number,
        } => {
            let node = resolve(node_ref, Kind::Node)?;
            let (si, ni) = node_index(project, &node)?;
            let start = time_anchor(&project.scenes[si], at)?;
            if *duration_ms == 0
                || start
                    .checked_add(*duration_ms)
                    .is_none_or(|end| end > project.scenes[si].duration_ms)
            {
                return Err(invalid(
                    "Preset duration must be positive and remain inside the scene",
                ));
            }
            let animations = preset_animations(
                &mut project.scenes[si].nodes[ni],
                id,
                *preset,
                start,
                *duration_ms,
                *from_number,
                *to_number,
            )?;
            for animation in &animations {
                Identities::reserve(&mut identities.animations, &animation.id)?;
            }
            project.scenes[si].animations.extend(animations);
        }
    }
    Ok(())
}

fn component_nodes(
    theme: &Theme,
    id: &str,
    component: Component,
    text: &str,
    parent: Option<String>,
    asset: Option<&str>,
    overrides: &Properties,
) -> Result<Vec<Node>> {
    if text.len() > MAX_CODE {
        return Err(invalid("Component text exceeds bounds"));
    }
    if component == Component::ArchitectureEdge {
        return Err(Error::new(
            crate::ErrorCode::Unsupported,
            "ArchitectureEdge requires diagram_edge_create with revision-bound endpoint refs",
        ));
    }
    let direct_text = matches!(
        component,
        Component::Title | Component::Subtitle | Component::MetricCounter | Component::LogoLockup
    );
    if direct_text {
        if asset.is_some() {
            return Err(invalid("This text component does not accept an asset"));
        }
        let mut properties = Properties {
            text: Some(text.into()),
            ..Properties::default()
        };
        properties.font_size = Some(match component {
            Component::Title | Component::LogoLockup => theme.font_size * 2.0,
            Component::Subtitle => theme.font_size,
            _ => theme.font_size * 1.5,
        });
        properties.font_weight = Some(theme.font_weight);
        let mut properties: Properties = patched(&properties, &serde_json::to_value(overrides)?)?;
        if overrides.text.is_some() {
            return Err(invalid(
                "Use the component text argument, not a second text override",
            ));
        }
        properties.text = Some(text.into());
        return Ok(vec![Node {
            id: id.into(),
            name: text.chars().take(80).collect(),
            kind: NodeKind::Text,
            parent,
            properties,
        }]);
    }
    let browser = component == Component::BrowserFrame;
    if browser != asset.is_some() {
        return Err(invalid(
            "Only BrowserFrame accepts an asset, and it requires one",
        ));
    }
    let code = matches!(component, Component::TerminalWindow | Component::CodePanel);
    let compact = matches!(component, Component::Badge | Component::CapabilityChip);
    let mut properties = Properties {
        width: Some(if compact { 280.0 } else { 720.0 }),
        height: Some(if compact { 76.0 } else { 400.0 }),
        fill: Some("@surface".into()),
        stroke: Some("@ink".into()),
        stroke_width: Some(theme.line_width),
        radius: Some(theme.radius),
        ..Properties::default()
    };
    properties = patched(&properties, &serde_json::to_value(overrides)?)?;
    let width = properties.width.unwrap_or(720.0);
    let height = properties.height.unwrap_or(400.0);
    let root = Node {
        id: id.into(),
        name: format!("{component:?}"),
        kind: NodeKind::Rect,
        parent,
        properties,
    };
    let mut child = Node {
        id: child_id(id, "node", "content"),
        name: "content".into(),
        kind: if browser {
            NodeKind::Image
        } else if code {
            NodeKind::Code
        } else {
            NodeKind::Text
        },
        parent: Some(id.into()),
        properties: Properties::default(),
    };
    if browser {
        if !text.is_empty() {
            return Err(invalid(
                "BrowserFrame does not fabricate browser chrome; text must be empty",
            ));
        }
        child.properties.asset = asset.map(str::to_owned);
        child.properties.width = Some((width - 2.0 * theme.spacing).max(0.0));
        child.properties.height = Some((height - 2.0 * theme.spacing).max(0.0));
    } else if code {
        child.properties.code = Some(text.into());
        child.properties.language = Some(Language::Plain);
        child.properties.font_family = Some(theme.mono_family.clone());
        child.properties.font_size = Some((theme.font_size * 0.7).max(4.0));
    } else {
        child.properties.text = Some(text.into());
        child.properties.font_size = Some(if compact {
            (theme.font_size * 0.7).max(4.0)
        } else {
            theme.font_size
        });
        child.properties.width = Some((width - 2.0 * theme.spacing).max(0.0));
        child.properties.wrap = Some(true);
    }
    Ok(vec![root, child])
}

fn preset_animations(
    node: &mut Node,
    id: &str,
    preset: AnimationPreset,
    start: u64,
    duration: u64,
    from_number: f64,
    to_number: f64,
) -> Result<Vec<Animation>> {
    if !security::identifier(id) {
        return Err(invalid("Invalid preset identity"));
    }
    let target = node.id.clone();
    let make = |id: String, property, from, to, offset: u64, duration_ms| Animation {
        id,
        target: target.clone(),
        property,
        from: Some(from),
        to,
        at: TimeAnchor {
            cue: None,
            offset_ms: offset as i64,
        },
        duration_ms,
        duration_cue: None,
        easing: Easing::EaseOutCubic,
    };
    let result = match preset {
        AnimationPreset::Fade => {
            node.properties.opacity = Some(0.0);
            vec![make(
                id.into(),
                AnimatedProperty::Opacity,
                AnimatedValue::Number(0.0),
                AnimatedValue::Number(1.0),
                start,
                duration,
            )]
        }
        AnimationPreset::Slide => {
            let to = node.properties.position.unwrap_or([0.0, 0.0]);
            let from = [to[0], to[1] + 32.0];
            node.properties.position = Some(from);
            node.properties.opacity = Some(0.0);
            vec![
                make(
                    id.into(),
                    AnimatedProperty::Position,
                    AnimatedValue::Vector(from),
                    AnimatedValue::Vector(to),
                    start,
                    duration,
                ),
                make(
                    child_id(id, "animation", "opacity"),
                    AnimatedProperty::Opacity,
                    AnimatedValue::Number(0.0),
                    AnimatedValue::Number(1.0),
                    start,
                    duration,
                ),
            ]
        }
        AnimationPreset::ScalePunch => {
            let to = node.properties.scale.unwrap_or([1.0, 1.0]);
            let from = [(to[0] * 0.88).max(0.001), (to[1] * 0.88).max(0.001)];
            node.properties.scale = Some(from);
            let mut animation = make(
                id.into(),
                AnimatedProperty::Scale,
                AnimatedValue::Vector(from),
                AnimatedValue::Vector(to),
                start,
                duration,
            );
            animation.easing = Easing::EaseOutBack;
            vec![animation]
        }
        AnimationPreset::TrackingExpansion => {
            if !matches!(node.kind, NodeKind::Text | NodeKind::Code) {
                return Err(invalid("Tracking preset requires text or code"));
            }
            let from = node.properties.letter_spacing.unwrap_or(0.0);
            let to = (from + 8.0).min(200.0);
            node.properties.letter_spacing = Some(from);
            vec![make(
                id.into(),
                AnimatedProperty::LetterSpacing,
                AnimatedValue::Number(from),
                AnimatedValue::Number(to),
                start,
                duration,
            )]
        }
        AnimationPreset::Counter => {
            if node.kind != NodeKind::Text || !from_number.is_finite() || !to_number.is_finite() {
                return Err(invalid("Counter requires a Text node and finite numbers"));
            }
            node.properties.text = Some(from_number.round().to_string());
            vec![make(
                id.into(),
                AnimatedProperty::Counter,
                AnimatedValue::Number(from_number),
                AnimatedValue::Number(to_number),
                start,
                duration,
            )]
        }
        AnimationPreset::Reveal | AnimationPreset::WordReveal | AnimationPreset::LineReveal => {
            if node.kind != NodeKind::Text {
                return Err(invalid("Text reveal presets require a Text node"));
            }
            let full = node.properties.text.clone().unwrap_or_default();
            if full.is_empty() {
                return Err(invalid("Text reveal requires nonempty text"));
            }
            let mut ends = match preset {
                AnimationPreset::Reveal => vec![full.len()],
                AnimationPreset::LineReveal => full
                    .char_indices()
                    .filter_map(|(i, c)| (c == '\n').then_some(i + c.len_utf8()))
                    .collect(),
                _ => {
                    let mut ends = Vec::new();
                    let mut in_word = false;
                    for (i, c) in full.char_indices() {
                        if c.is_whitespace() {
                            if in_word {
                                ends.push(i);
                            }
                            in_word = false;
                        } else {
                            in_word = true;
                        }
                    }
                    ends
                }
            };
            if ends.last().copied() != Some(full.len()) {
                ends.push(full.len());
            }
            if ends.len() > MAX_PRESET_STEPS || duration < ends.len() as u64 {
                return Err(invalid("Reveal step count or duration exceeds bounds"));
            }
            node.properties.text = Some(String::new());
            let count = ends.len() as u64;
            let mut previous = String::new();
            let mut animations = Vec::with_capacity(ends.len());
            for (i, end) in ends.into_iter().enumerate() {
                let next = full[..end].to_string(); // char_indices produced UTF-8 boundaries.
                let offset = duration * i as u64 / count;
                let next_offset = duration * (i as u64 + 1) / count;
                let aid = if i == 0 {
                    id.to_string()
                } else {
                    child_id(id, "animation", &i.to_string())
                };
                animations.push(make(
                    aid,
                    AnimatedProperty::Text,
                    AnimatedValue::Text(previous),
                    AnimatedValue::Text(next.clone()),
                    start + offset,
                    next_offset - offset,
                ));
                previous = next;
            }
            animations
        }
    };
    Ok(result)
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod transaction_tests;
