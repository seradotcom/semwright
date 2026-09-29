//! Bounded native observation wire and projection validation.
//! Parsing this data does not establish authority: only the provider's isolated
//! process runner may admit it as native evidence for an authenticated plan.
use schemars::JsonSchema;
use semwright_project_graph::ProjectId;
use semwright_semantic_composition::{Digest, Owner, canonical_digest, strict_decode};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const NATIVE_VERSION: u32 = 1;
pub const PROBE_SOURCE: &str =
    include_str!("../../../../integrations/godot/authoring/native_observer.gd");
pub const MAX_OBSERVATION_BYTES: usize = 8_388_608;
pub const MAX_NATIVE_NODES: usize = 4096;
pub const MAX_NATIVE_TRACKS: usize = 2048;
pub const MAX_NATIVE_KEYS: usize = 16_384;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeMode {
    Inspect,
    SaveCandidate,
    ReopenCandidate,
    Play,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputStep {
    pub tick: u32,
    pub action: String,
    pub pressed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeRequest {
    pub version: u32,
    pub nonce: String,
    pub source_fingerprint: Digest,
    pub mode: ProbeMode,
    pub scene: String,
    pub ticks: u32,
    pub inputs: Vec<InputStep>,
    pub checkpoints: Vec<u32>,
    pub variables: Vec<String>,
    pub capture: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeVerification {
    Inspect,
    Persistence,
    Play {
        ticks: u32,
        inputs: Vec<InputStep>,
        checkpoints: Vec<u32>,
        variables: Vec<String>,
        capture: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeVerifyRequest {
    pub plan_id: String,
    pub scene: String,
    pub verification: NativeVerification,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeEvidenceBinding {
    pub owner: Owner,
    pub request_id: String,
    pub project: ProjectId,
    pub slug: String,
    pub plan_digest: Digest,
    pub intent_digest: Digest,
    pub source_fingerprint: Digest,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeVerifyResult {
    Inspect {
        binding: NativeEvidenceBinding,
        observation: NativeObservation,
    },
    Persistence {
        binding: NativeEvidenceBinding,
        writer: NativeObservation,
        reader: NativeObservation,
        evidence: semwright_effect_conformance::ObservedValue,
    },
    Play {
        binding: NativeEvidenceBinding,
        observation: NativeObservation,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeResourceRef {
    pub class: String,
    pub path: String,
    pub uid: Option<String>,
    pub instance_id: String,
    pub local_to_scene: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum NativeValue {
    Null,
    Bool(bool),
    Int(String),
    Float(f64),
    Text(String),
    Vector2([f64; 2]),
    Vector3([f64; 3]),
    Quaternion([f64; 4]),
    Color([f64; 4]),
    Transform2([f64; 6]),
    Transform3([f64; 12]),
    Numbers(Vec<f64>),
    Resource(NativeResourceRef),
    Unsupported(String),
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeNode {
    pub path: String,
    pub class: String,
    pub instance_id: String,
    pub parent: Option<String>,
    pub owner: Option<String>,
    pub scene_file: String,
    pub logical_id: Option<String>,
    pub logical_key: Option<String>,
    pub groups: Vec<String>,
    pub properties: BTreeMap<String, NativeValue>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeResource {
    pub binding: String,
    pub resource: NativeResourceRef,
    pub properties: BTreeMap<String, NativeValue>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeKey {
    pub time: f64,
    pub transition: f64,
    pub value: NativeValue,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeTrack {
    pub index: u32,
    pub track_type: u32,
    pub path: String,
    pub enabled: bool,
    pub interpolation: u32,
    pub imported: bool,
    pub key_count: u32,
    pub keys: Vec<NativeKey>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeAnimation {
    pub player: String,
    pub library: String,
    pub name: String,
    pub root: String,
    pub length: f64,
    pub loop_mode: u32,
    pub resource: NativeResourceRef,
    pub track_count: u32,
    pub tracks: Vec<NativeTrack>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeConnection {
    pub source: String,
    pub signal: String,
    pub target: String,
    pub method: String,
    pub flags: u32,
    pub binds: Vec<NativeValue>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeProjection {
    pub nodes: Vec<NativeNode>,
    pub resources: Vec<NativeResource>,
    pub animations: Vec<NativeAnimation>,
    pub connections: Vec<NativeConnection>,
    pub unknown: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeDependency {
    pub source: String,
    pub path: String,
    pub uid: Option<String>,
    pub sha256: Option<Digest>,
    pub exists: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeFrame {
    pub requested_tick: u32,
    pub native_frame: String,
    pub scene_instance: String,
    pub scene_path: String,
    pub state: Option<String>,
    pub fault: Option<String>,
    pub ticks: Option<u64>,
    pub events: Option<u64>,
    pub variables: BTreeMap<String, NativeValue>,
    pub positions: BTreeMap<String, NativeValue>,
    pub labels: BTreeMap<String, String>,
    pub capture_sha256: Option<Digest>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeObservation {
    pub version: u32,
    pub nonce: String,
    pub source_fingerprint: Digest,
    pub mode: ProbeMode,
    pub engine_version: String,
    pub process_id: String,
    pub loaded_scene: String,
    pub loaded_scene_sha256: Digest,
    pub candidate_sha256: Option<Digest>,
    pub authored: NativeProjection,
    pub live: Option<NativeProjection>,
    pub frames: Vec<RuntimeFrame>,
    pub dependencies: Vec<NativeDependency>,
    pub dependency_complete: bool,
    pub inputs_delivered: u32,
    pub elapsed_physics_frames: u32,
    pub failures: Vec<String>,
}
fn invalid(message: &str) -> semwright_types::Error {
    semwright_types::Error::invalid(message)
}
fn ensure(condition: bool, message: &str) -> semwright_types::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(invalid(message))
    }
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 48
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn finite(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite() && v.abs() <= 1.0e12)
}
impl NativeRequest {
    pub fn validate(&self, declared_actions: &BTreeSet<String>) -> semwright_types::Result<()> {
        ensure(
            self.version == NATIVE_VERSION,
            "Native probe request version",
        )?;
        ensure(
            self.nonce.len() >= 16
                && self.nonce.len() <= 80
                && self
                    .nonce
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "Native probe nonce",
        )?;
        let scene = self
            .scene
            .strip_prefix("res://scenes/")
            .and_then(|p| p.strip_suffix(".tscn"));
        ensure(
            scene.is_some_and(identifier),
            "Native observation requires a managed scene locator",
        )?;
        ensure(
            self.variables.len() <= 64
                && self.variables.iter().all(|v| identifier(v))
                && self.variables.iter().collect::<BTreeSet<_>>().len() == self.variables.len(),
            "Native variable readback scope",
        )?;
        ensure(
            self.inputs.len() <= 256 && self.checkpoints.len() <= 32 && self.ticks <= 3600,
            "Native scenario input/tick/checkpoint budget",
        )?;
        if self.mode != ProbeMode::Play {
            ensure(
                self.ticks == 0
                    && self.inputs.is_empty()
                    && self.checkpoints.is_empty()
                    && !self.capture,
                "Non-play native observation may not inject input or capture frames",
            )?;
        } else {
            ensure(
                self.ticks > 0 && !self.checkpoints.is_empty(),
                "Play requires bounded ticks and checkpoints",
            )?;
        }
        let mut previous = 0;
        let mut state = BTreeMap::new();
        for step in &self.inputs {
            ensure(
                step.tick > 0 && step.tick <= self.ticks && step.tick >= previous,
                "Native input ordering/tick range",
            )?;
            ensure(
                declared_actions.contains(&step.action),
                "Native scenario input action is not declared",
            )?;
            ensure(
                state.get(&step.action).copied().unwrap_or(false) != step.pressed,
                "Duplicate native input state transition",
            )?;
            state.insert(step.action.clone(), step.pressed);
            previous = step.tick;
        }
        ensure(
            state.values().all(|pressed| !pressed),
            "Native input sequence must release every held action",
        )?;
        previous = 0;
        for checkpoint in &self.checkpoints {
            ensure(
                *checkpoint > previous && *checkpoint <= self.ticks,
                "Native checkpoint ordering/range",
            )?;
            previous = *checkpoint;
        }
        Ok(())
    }
    pub fn loaded_scene(&self) -> String {
        if self.mode == ProbeMode::ReopenCandidate {
            self.scene.replace("res://scenes/", "res://__sw_saved/")
        } else {
            self.scene.clone()
        }
    }
}
impl NativeValue {
    fn validate(&self) -> semwright_types::Result<()> {
        let valid = match self {
            Self::Null | Self::Bool(_) => true,
            Self::Int(v) => v.len() <= 21 && v.parse::<i64>().is_ok(),
            Self::Float(v) => finite(&[*v]),
            Self::Text(v) => v.len() <= 8192 && !v.contains('\0'),
            Self::Vector2(v) => finite(v),
            Self::Vector3(v) => finite(v),
            Self::Quaternion(v) | Self::Color(v) => finite(v),
            Self::Transform2(v) => finite(v),
            Self::Transform3(v) => finite(v),
            Self::Numbers(v) => v.len() <= 256 && finite(v),
            Self::Resource(v) => {
                v.validate()?;
                true
            }
            Self::Unsupported(reason) => !reason.is_empty() && reason.len() <= 256,
        };
        ensure(valid, "Invalid or unbounded native value")
    }
    fn remove_ephemeral_identity(&mut self) {
        if let Self::Resource(resource) = self {
            resource.instance_id = "0".into();
            resource.uid = None;
        }
    }
}
impl NativeResourceRef {
    fn validate(&self) -> semwright_types::Result<()> {
        ensure(
            !self.class.is_empty() && self.class.len() <= 128 && self.path.len() <= 1024,
            "Native resource identity bounds",
        )?;
        ensure(
            self.instance_id.parse::<u64>().is_ok(),
            "Native resource instance ID must be an integer string",
        )?;
        ensure(
            self.uid
                .as_ref()
                .is_none_or(|s| s.starts_with("uid://") && s.len() <= 80),
            "Native resource UID",
        )
    }
}
fn properties(values: &BTreeMap<String, NativeValue>) -> semwright_types::Result<()> {
    ensure(values.len() <= 128, "Native property scope limit")?;
    for (name, value) in values {
        ensure(
            !name.is_empty() && name.len() <= 128,
            "Native property name bound",
        )?;
        value.validate()?;
    }
    Ok(())
}
impl NativeProjection {
    pub fn validate(&self) -> semwright_types::Result<()> {
        ensure(
            !self.nodes.is_empty() && self.nodes.len() <= MAX_NATIVE_NODES,
            "Native node count",
        )?;
        ensure(
            self.resources.len() <= 4096
                && self.animations.len() <= 128
                && self.connections.len() <= 4096
                && self.unknown.len() <= 128,
            "Native projection collection budget",
        )?;
        let paths = self
            .nodes
            .iter()
            .map(|n| n.path.as_str())
            .collect::<BTreeSet<_>>();
        ensure(
            paths.len() == self.nodes.len() && paths.contains("."),
            "Duplicate or missing native root path",
        )?;
        for node in &self.nodes {
            ensure(
                node.path.len() <= 1024 && node.class.len() <= 128 && !node.class.is_empty(),
                "Native node path/class",
            )?;
            ensure(
                node.instance_id.parse::<u64>().is_ok(),
                "Native node instance ID",
            )?;
            ensure(
                node.parent
                    .as_ref()
                    .is_none_or(|p| paths.contains(p.as_str())),
                "Native parent absent from complete node scope",
            )?;
            ensure(
                node.owner
                    .as_ref()
                    .is_none_or(|p| paths.contains(p.as_str())),
                "Native owner absent from complete node scope",
            )?;
            ensure(
                node.groups.len() <= 64 && node.groups.iter().all(|g| g.len() <= 128),
                "Native groups bound",
            )?;
            if let Some(id) = &node.logical_id {
                semwright_project_graph::LogicalAssetId::parse(id.clone())
                    .map_err(|_| invalid("Invalid persistent native metadata ID"))?;
            }
            ensure(
                node.logical_key.as_ref().is_none_or(|k| k.len() <= 256),
                "Native logical key bound",
            )?;
            properties(&node.properties)?;
        }
        let mut bindings = BTreeSet::new();
        for resource in &self.resources {
            ensure(
                resource.binding.len() <= 2048 && bindings.insert(&resource.binding),
                "Duplicate resource binding",
            )?;
            resource.resource.validate()?;
            properties(&resource.properties)?;
        }
        let mut animations = BTreeSet::new();
        let mut tracks = 0usize;
        let mut keys = 0usize;
        for animation in &self.animations {
            ensure(
                paths.contains(animation.player.as_str())
                    && animation.name.len() <= 256
                    && animation.library.len() <= 256
                    && animation.root.len() <= 1024,
                "Native animation binding",
            )?;
            ensure(
                animations.insert((&animation.player, &animation.library, &animation.name)),
                "Duplicate native animation",
            )?;
            ensure(
                animation.length.is_finite()
                    && animation.length > 0.0
                    && animation.length <= 100_000.0,
                "Native animation length",
            )?;
            ensure(
                animation.track_count as usize == animation.tracks.len(),
                "Native animation metadata was truncated",
            )?;
            animation.resource.validate()?;
            for (index, track) in animation.tracks.iter().enumerate() {
                ensure(
                    track.index as usize == index && track.path.len() <= 2048,
                    "Native animation stable track order",
                )?;
                ensure(
                    track.key_count as usize == track.keys.len(),
                    "Native animation keys were truncated",
                )?;
                let mut time = -1.0;
                for key in &track.keys {
                    ensure(
                        key.time.is_finite()
                            && key.time >= time
                            && key.time >= 0.0
                            && key.time <= animation.length + 0.00001
                            && key.transition.is_finite(),
                        "Native key time/transition",
                    )?;
                    key.value.validate()?;
                    time = key.time;
                    keys += 1;
                }
                tracks += 1;
            }
        }
        ensure(
            tracks <= MAX_NATIVE_TRACKS && keys <= MAX_NATIVE_KEYS,
            "Native animation collection budget",
        )?;
        for connection in &self.connections {
            ensure(
                paths.contains(connection.source.as_str())
                    && paths.contains(connection.target.as_str()),
                "Native signal target outside scene scope",
            )?;
            ensure(
                connection.signal.len() <= 128
                    && connection.method.len() <= 128
                    && connection.binds.len() <= 16,
                "Native signal metadata bounds",
            )?;
            for value in &connection.binds {
                value.validate()?;
            }
        }
        ensure(
            self.unknown.iter().all(|s| !s.is_empty() && s.len() <= 512),
            "Native unknown coverage reasons",
        )
    }
    /// Stable content projection excludes process-local IDs and UID cache state.
    /// Resource sharing is retained as equivalence classes of observed bindings.
    pub fn stable_digest(&self) -> semwright_types::Result<Digest> {
        self.validate()?;
        let mut normalized = self.clone();
        let mut aliases: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for resource in &self.resources {
            aliases
                .entry(resource.resource.instance_id.clone())
                .or_default()
                .push(resource.binding.clone());
        }
        for node in &mut normalized.nodes {
            node.instance_id = "0".into();
            node.scene_file = normalized_path(&node.scene_file);
            node.groups.sort();
            for value in node.properties.values_mut() {
                normalize_value(value);
            }
        }
        for resource in &mut normalized.resources {
            normalize_resource(&mut resource.resource);
            for value in resource.properties.values_mut() {
                normalize_value(value);
            }
        }
        for animation in &mut normalized.animations {
            normalize_resource(&mut animation.resource);
            for track in &mut animation.tracks {
                for key in &mut track.keys {
                    normalize_value(&mut key.value);
                }
            }
        }
        for connection in &mut normalized.connections {
            for value in &mut connection.binds {
                normalize_value(value);
            }
        }
        normalized.nodes.sort_by(|a, b| a.path.cmp(&b.path));
        normalized
            .resources
            .sort_by(|a, b| a.binding.cmp(&b.binding));
        normalized.animations.sort_by(|a, b| {
            (&a.player, &a.library, &a.name).cmp(&(&b.player, &b.library, &b.name))
        });
        normalized.connections.sort_by(|a, b| {
            (&a.source, &a.signal, &a.target, &a.method)
                .cmp(&(&b.source, &b.signal, &b.target, &b.method))
        });
        normalized.unknown.sort();
        let mut aliases = aliases.into_values().collect::<Vec<_>>();
        for group in &mut aliases {
            group.sort();
        }
        aliases.sort();
        canonical_digest(&("godot-native-projection-v1", normalized, aliases))
            .map_err(|e| invalid(&e.to_string()))
    }
}
fn normalized_path(path: &str) -> String {
    if path.contains("::") {
        "<native-subresource>".into()
    } else {
        path.replace("res://__sw_saved/", "res://scenes/")
    }
}
fn normalize_resource(resource: &mut NativeResourceRef) {
    resource.instance_id = "0".into();
    resource.uid = None;
    resource.path = normalized_path(&resource.path);
}
fn normalize_value(value: &mut NativeValue) {
    value.remove_ephemeral_identity();
    if let NativeValue::Resource(resource) = value {
        normalize_resource(resource);
    }
}
impl NativeObservation {
    /// Data validation only. It does not accept caller-provided native evidence.
    pub fn validate(&self, request: &NativeRequest) -> semwright_types::Result<()> {
        ensure(
            self.version == NATIVE_VERSION
                && self.nonce == request.nonce
                && self.source_fingerprint == request.source_fingerprint
                && self.mode == request.mode,
            "Native observation request/source mismatch",
        )?;
        ensure(
            self.process_id.parse::<u32>().is_ok_and(|pid| pid > 0)
                && self.engine_version.len() <= 128
                && self.engine_version.starts_with("4.7.2.stable"),
            "Native process/runtime identity mismatch",
        )?;
        ensure(
            self.loaded_scene == request.loaded_scene(),
            "Native loaded scene mismatch",
        )?;
        ensure(
            self.failures.is_empty(),
            "Native observation reported failures",
        )?;
        ensure(
            (self.mode == ProbeMode::SaveCandidate) == self.candidate_sha256.is_some(),
            "Missing or unexpected candidate save receipt",
        )?;
        self.authored.validate()?;
        if let Some(live) = &self.live {
            live.validate()?;
        }
        ensure(
            self.live.is_some() == (self.mode == ProbeMode::Play),
            "Native live readback mode mismatch",
        )?;
        ensure(
            self.frames.len() == request.checkpoints.len(),
            "Required runtime checkpoints were not all observed",
        )?;
        ensure(
            self.inputs_delivered as usize == request.inputs.len(),
            "Required input events were not all delivered",
        )?;
        ensure(
            self.dependencies.len() <= 4096,
            "Native dependency scope limit",
        )?;
        let mut dependencies = BTreeSet::new();
        for dependency in &self.dependencies {
            ensure(
                dependency.path.len() <= 1024
                    && dependency.source.len() <= 1024
                    && dependencies.insert((&dependency.source, &dependency.path)),
                "Native dependency identity/order",
            )?;
            ensure(
                !dependency.exists || dependency.sha256.is_some(),
                "Existing native dependency lacks bytes evidence",
            )?;
        }
        for (frame, tick) in self.frames.iter().zip(&request.checkpoints) {
            ensure(
                frame.requested_tick == *tick
                    && frame.native_frame.parse::<u64>().is_ok()
                    && frame.scene_instance.parse::<u64>().is_ok(),
                "Native frame binding mismatch",
            )?;
            ensure(
                frame.capture_sha256.is_some() == request.capture,
                "Required native frame capture missing or unexpected",
            )?;
            ensure(
                frame.variables.keys().cloned().collect::<BTreeSet<_>>()
                    == request.variables.iter().cloned().collect(),
                "Required runtime variable scope mismatch",
            )?;
            properties(&frame.variables)?;
            ensure(
                frame.positions.len() <= MAX_NATIVE_NODES && frame.labels.len() <= MAX_NATIVE_NODES,
                "Runtime node observation budget",
            )?;
            for value in frame.positions.values() {
                value.validate()?;
            }
            ensure(
                frame.labels.values().all(|text| text.len() <= 8192),
                "Runtime label readback limit",
            )?;
        }
        if self.mode == ProbeMode::Play {
            ensure(
                self.elapsed_physics_frames >= request.ticks,
                "Native physics did not execute the requested ticks",
            )?;
        } else {
            ensure(
                self.elapsed_physics_frames == 0,
                "Non-play probe changed its runtime tick scope",
            )?;
        }
        Ok(())
    }
    pub fn snapshot_digest(&self) -> semwright_types::Result<Digest> {
        canonical_digest(&(
            "godot-native-snapshot-v1",
            &self.nonce,
            &self.source_fingerprint,
            &self.process_id,
            self.authored.stable_digest()?,
            self.live
                .as_ref()
                .map(NativeProjection::stable_digest)
                .transpose()?,
        ))
        .map_err(|e| invalid(&e.to_string()))
    }
}
pub fn decode_observation(
    bytes: &[u8],
    request: &NativeRequest,
) -> semwright_types::Result<NativeObservation> {
    ensure(
        bytes.len() <= MAX_OBSERVATION_BYTES,
        "Native observation byte budget",
    )?;
    let observation: NativeObservation =
        strict_decode(bytes).map_err(|e| invalid(&e.to_string()))?;
    observation.validate(request)?;
    Ok(observation)
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackSummary {
    pub player: String,
    pub library: String,
    pub animation: String,
    pub root: String,
    pub index: u32,
    pub track_type: u32,
    pub path: String,
    pub enabled: bool,
    pub key_count: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackPage {
    pub snapshot: Digest,
    pub source_fingerprint: Digest,
    pub total: u32,
    pub tracks: Vec<TrackSummary>,
    pub next_cursor: Option<String>,
}
fn cursor_offset(
    cursor: Option<&str>,
    snapshot: &Digest,
    total: usize,
) -> semwright_types::Result<usize> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    ensure(cursor.len() <= 100, "Native track cursor bound")?;
    let fields = cursor.split('.').collect::<Vec<_>>();
    ensure(
        fields.len() == 3 && fields[0] == "gtr1" && fields[1] == snapshot.as_str(),
        "Stale native track cursor",
    )?;
    let offset = fields[2]
        .parse::<usize>()
        .map_err(|_| invalid("Invalid native track cursor offset"))?;
    ensure(
        offset <= total,
        "Native track cursor offset beyond collection",
    )?;
    Ok(offset)
}
pub fn track_page(
    observation: &NativeObservation,
    current_source: &Digest,
    cursor: Option<&str>,
    limit: u16,
) -> semwright_types::Result<TrackPage> {
    ensure(
        (1..=64).contains(&limit),
        "Native track page limit must be 1..64",
    )?;
    ensure(
        current_source == &observation.source_fingerprint,
        "Source changed after native observation; reacquire",
    )?;
    let snapshot = observation.snapshot_digest()?;
    let mut animations = observation.authored.animations.iter().collect::<Vec<_>>();
    animations
        .sort_by(|a, b| (&a.player, &a.library, &a.name).cmp(&(&b.player, &b.library, &b.name)));
    let rows = animations
        .into_iter()
        .flat_map(|animation| {
            animation.tracks.iter().map(move |track| TrackSummary {
                player: animation.player.clone(),
                library: animation.library.clone(),
                animation: animation.name.clone(),
                root: animation.root.clone(),
                index: track.index,
                track_type: track.track_type,
                path: track.path.clone(),
                enabled: track.enabled,
                key_count: track.key_count,
            })
        })
        .collect::<Vec<_>>();
    let offset = cursor_offset(cursor, &snapshot, rows.len())?;
    let end = offset.saturating_add(usize::from(limit)).min(rows.len());
    Ok(TrackPage {
        snapshot: snapshot.clone(),
        source_fingerprint: current_source.clone(),
        total: rows.len() as u32,
        tracks: rows[offset..end].to_vec(),
        next_cursor: (end < rows.len()).then(|| format!("gtr1.{}.{end}", snapshot.as_str())),
    })
}
fn dependency_sentinels(
    observation: &NativeObservation,
) -> semwright_types::Result<Vec<(String, String, bool, Option<Digest>)>> {
    ensure(
        observation.dependency_complete,
        "Persistence requires complete native dependency enumeration",
    )?;
    let mut rows = observation
        .dependencies
        .iter()
        .map(|dependency| {
            (
                normalized_path(&dependency.source),
                normalized_path(&dependency.path),
                dependency.exists,
                dependency.sha256.clone(),
            )
        })
        .collect::<Vec<_>>();
    rows.sort();
    ensure(
        rows.windows(2).all(|pair| pair[0] != pair[1]),
        "Normalized native dependency identity collision",
    )?;
    Ok(rows)
}

/// Build F's existing persistence value from two separately admitted native runs.
/// This function checks coherence; it does not turn client JSON into trusted evidence.
pub fn persistence_value(
    writer: &NativeObservation,
    reader: &NativeObservation,
) -> semwright_types::Result<semwright_effect_conformance::ObservedValue> {
    ensure(
        writer.mode == ProbeMode::SaveCandidate && reader.mode == ProbeMode::ReopenCandidate,
        "Persistence requires native save and a fresh-process reopen",
    )?;
    ensure(
        writer.process_id != reader.process_id
            && writer.nonce != reader.nonce
            && writer.source_fingerprint == reader.source_fingerprint,
        "Persistence process/source binding mismatch",
    )?;
    ensure(
        dependency_sentinels(writer)? == dependency_sentinels(reader)?,
        "External native dependency sentinel changed across save/reopen",
    )?;
    let saved = writer
        .candidate_sha256
        .clone()
        .ok_or_else(|| invalid("Native candidate hash missing"))?;
    Ok(semwright_effect_conformance::ObservedValue::Reopened {
        writer_process: writer.process_id.clone(),
        reader_process: reader.process_id.clone(),
        before_projection: writer.authored.stable_digest()?,
        after_projection: reader.authored.stable_digest()?,
        saved_digest: saved,
        reopened_digest: reader.loaded_scene_sha256.clone(),
    })
}
