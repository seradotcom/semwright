//! Versioned Blender-domain data. No executable strings, generic RNA setters or filesystem writes.
use schemars::JsonSchema;
use semwright_media_time::Rate;
use semwright_semantic_composition::{Result, canonical_bytes, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_ENTITIES: usize = 128;
pub const MAX_VERTICES: usize = 8192;
pub const MAX_FACES: usize = 8192;
pub const MAX_BONES: usize = 64;
pub const MAX_KEYS: usize = 512;
pub const MAX_OPERATIONS: usize = 512;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlenderAuthoringSpec {
    pub version: u32,
    /// Local authoring alias, not Project Graph's LogicalAssetId.
    pub collection: String,
    pub meters_per_unit: f64,
    #[serde(default)]
    pub textures: Vec<TextureAsset>,
    pub materials: Vec<Material>,
    pub entities: Vec<Entity>,
    pub relations: Vec<Relation>,
    pub animation: Option<Animation>,
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TextureColorSpace {
    Srgb,
    NonColor,
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TextureChannel {
    Color,
    Red,
    Green,
    Blue,
    Alpha,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextureAsset {
    pub id: String,
    pub path: String,
    pub sha256: String,
    pub color_space: TextureColorSpace,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextureBinding {
    pub texture: String,
    pub channel: TextureChannel,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalTextureBinding {
    pub texture: String,
    pub strength: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub id: String,
    pub base_color: [f64; 4],
    pub roughness: f64,
    pub metallic: f64,
    #[serde(default = "default_emission_color")]
    pub emission_color: [f64; 4],
    #[serde(default)]
    pub emission_strength: f64,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    #[serde(default)]
    pub base_color_texture: Option<TextureBinding>,
    #[serde(default)]
    pub roughness_texture: Option<TextureBinding>,
    #[serde(default)]
    pub metallic_texture: Option<TextureBinding>,
    #[serde(default)]
    pub normal_texture: Option<NormalTextureBinding>,
    #[serde(default)]
    pub emission_texture: Option<TextureBinding>,
    #[serde(default)]
    pub opacity_texture: Option<TextureBinding>,
}
fn default_emission_color() -> [f64; 4] {
    [0.0, 0.0, 0.0, 1.0]
}
fn default_opacity() -> f64 {
    1.0
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    /// Parent-local values; all lengths use meters_per_unit, rotations are radians.
    pub translation: [f64; 3],
    pub rotation: [f64; 3],
    pub scale: [f64; 3],
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub shape: Shape,
    pub transform: Transform,
    pub materials: Vec<String>,
    pub modifiers: Vec<Modifier>,
    /// Source-mesh shading intent. This does not claim custom-normal authoring.
    #[serde(default)]
    pub shade_smooth: bool,
    /// Closed typed custom-attribute subset; no arbitrary Blender attribute type string.
    #[serde(default)]
    pub attributes: Vec<MeshAttribute>,
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum MeshAttributeDomain {
    Point,
    Face,
    Corner,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeshAttributeData {
    Float { values: Vec<f64> },
    Vector { values: Vec<[f64; 3]> },
    Color { values: Vec<[f64; 4]> },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeshAttribute {
    pub id: String,
    pub domain: MeshAttributeDomain,
    pub data: MeshAttributeData,
}

impl Entity {
    pub fn dependency_ids(&self) -> Vec<String> {
        let mut ids = BTreeSet::new();
        if let Shape::MeshInstance { source } | Shape::MeshCopy { source } = &self.shape {
            ids.insert(source.clone());
        }
        for modifier in &self.modifiers {
            if let Modifier::Boolean { target, .. } = modifier {
                ids.insert(target.clone());
            }
        }
        ids.into_iter().collect()
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    Box {
        size: [f64; 3],
    },
    Cylinder {
        radius: f64,
        depth: f64,
        segments: u32,
    },
    Mesh {
        vertices: Vec<[f64; 3]>,
        faces: Vec<Vec<u32>>,
        uv: Option<Vec<[f64; 2]>>,
    },
    Empty,
    Armature {
        bones: Vec<Bone>,
    },
    Camera {
        lens_mm: f64,
        clip_start: f64,
        clip_end: f64,
    },
    AreaLight {
        energy_watts: f64,
        size: f64,
        color: [f64; 3],
    },
    /// Explicit shared mesh datablock. Material/modifier writes are forbidden.
    MeshInstance {
        source: String,
    },
    /// Explicit copy-on-write of one concrete managed mesh datablock.
    MeshCopy {
        source: String,
    },
    /// Editable legacy Curve datablock with one bounded POLY spline.
    Curve {
        points: Vec<[f64; 3]>,
        cyclic: bool,
        extrude: f64,
        bevel_depth: f64,
        bevel_resolution: u32,
    },
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BooleanOperation {
    Difference,
    Union,
    Intersect,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bone {
    pub id: String,
    pub head: [f64; 3],
    pub tail: [f64; 3],
    pub parent: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Modifier {
    Bevel {
        width: f64,
        segments: u32,
    },
    Mirror {
        axes: [bool; 3],
    },
    Subdivision {
        levels: u32,
    },
    Array {
        count: u32,
        offset: [f64; 3],
    },
    /// Non-destructive, exact-solver boolean against another managed mesh object.
    Boolean {
        operation: BooleanOperation,
        target: String,
    },
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintSpace {
    World,
    Local,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Relation {
    Parent {
        child: String,
        parent: String,
    },
    BoneParent {
        child: String,
        armature: String,
        bone: String,
    },
    /// A native COPY_LOCATION constraint, not a one-time coordinate rewrite.
    Follow {
        subject: String,
        target: String,
        offset: bool,
    },
    /// Axis-selective native COPY_LOCATION relation with explicit coordinate spaces.
    Align {
        subject: String,
        target: String,
        axes: [bool; 3],
        offset: bool,
        owner_space: ConstraintSpace,
        target_space: ConstraintSpace,
    },
    /// A native TRACK_TO constraint, with fixed local -Z / +Y axis convention.
    LookAt {
        subject: String,
        target: String,
    },
    Skin {
        mesh: String,
        armature: String,
        weights: Vec<Weight>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Weight {
    pub vertex: u32,
    pub bone: String,
    pub weight: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Animation {
    pub id: String,
    pub rate: Rate,
    pub channels: Vec<Channel>,
    /// Optional NLA arrangement over the managed Action generated for each entity.
    /// Tracks cannot reference arbitrary external Actions.
    #[serde(default)]
    pub nla_tracks: Vec<NlaTrack>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NlaTrack {
    pub id: String,
    pub entity: String,
    pub strips: Vec<NlaStrip>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NlaStrip {
    pub id: String,
    pub start_frame: i32,
    pub action_frame_start: i32,
    pub action_frame_end: i32,
    pub repeat: f64,
    pub scale: f64,
    pub influence: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub entity: String,
    pub bone: Option<String>,
    pub property: AnimatedProperty,
    pub keys: Vec<Key>,
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AnimatedProperty {
    Translation,
    Rotation,
    Scale,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub frame: i32,
    pub value: [f64; 3],
}

/// Compact public transport schema for composition.plan. The full AuthoringIntent
/// schema remains part of ProfileIdentity and is enforced by strict serde decode plus
/// BlenderAuthoringSpec::validate; the external Registry envelope is deliberately
/// shallow enough to stay within its bounded schema walk.
pub fn plan_transport_input_schema() -> serde_json::Value {
    let id = serde_json::json!({
        "type":"string","minLength":1,"maxLength":64,
        "pattern":"^[A-Za-z0-9_-]+$"
    });
    let vec3 = serde_json::json!({
        "type":"array","minItems":3,"maxItems":3,
        "items":{"type":"number"}
    });
    serde_json::json!({
        "type":"object",
        "properties":{
            "intent":{
                "oneOf":[
                    {
                        "type":"object",
                        "properties":{
                            "kind":{"const":"create"},
                            "spec":{
                                "type":"object",
                                "properties":{
                                    "version":{"type":"integer","const":1},
                                    "collection":id.clone(),
                                    "meters_per_unit":{"type":"number","exclusiveMinimum":0.0,"maximum":100.0},
                                    "textures":{"type":"array","maxItems":32,"items":{"type":"object","maxProperties":8}},
                                    "materials":{"type":"array","maxItems":64,"items":{"type":"object","maxProperties":24}},
                                    "entities":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"object","maxProperties":16}},
                                    "relations":{"type":"array","maxItems":256,"items":{"type":"object","maxProperties":16}},
                                    "animation":{"type":["object","null"],"maxProperties":8}
                                },
                                "required":["version","collection","meters_per_unit","materials","entities","relations"],
                                "additionalProperties":false
                            }
                        },
                        "required":["kind","spec"],
                        "additionalProperties":false
                    },
                    {
                        "type":"object",
                        "properties":{
                            "kind":{"const":"transform"},
                            "island":id.clone(),
                            "entity":id.clone(),
                            "transform":{
                                "type":"object",
                                "properties":{
                                    "translation":vec3.clone(),
                                    "rotation":vec3.clone(),
                                    "scale":vec3
                                },
                                "required":["translation","rotation","scale"],
                                "additionalProperties":false
                            },
                            "meters_per_unit":{"type":"number","exclusiveMinimum":0.0,"maximum":100.0},
                            "expected_fingerprint":{"type":"string","pattern":"^[a-f0-9]{64}$"}
                        },
                        "required":["kind","island","entity","transform","meters_per_unit","expected_fingerprint"],
                        "additionalProperties":false
                    },
                    {
                        "type":"object",
                        "properties":{
                            "kind":{"const":"material_slots"},
                            "island":id.clone(),
                            "entity":id.clone(),
                            "materials":{"type":"array","maxItems":16,"items":id},
                            "expected_fingerprint":{"type":"string","pattern":"^[a-f0-9]{64}$"}
                        },
                        "required":["kind","island","entity","materials","expected_fingerprint"],
                        "additionalProperties":false
                    }
                ]
            }
        },
        "required":["intent"],
        "additionalProperties":false
    })
}

pub fn local_id(s: &str) -> Result<()> {
    ensure(
        !s.is_empty()
            && s.len() <= 64
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "local ID must be 1..64 ASCII letters, digits, underscore or hyphen",
    )
}
fn relative_asset_path(value: &str) -> Result<()> {
    ensure(
        !value.is_empty() && value.len() <= 1024,
        "texture path length",
    )?;
    ensure(
        !value.starts_with('/') && !value.contains('\0') && !value.contains('\\'),
        "texture path must be clean relative POSIX path",
    )?;
    let mut extension = None;
    for part in value.split('/') {
        ensure(
            !part.is_empty() && part != "." && part != "..",
            "texture path component",
        )?;
        extension = part
            .rsplit_once('.')
            .map(|(_, suffix)| suffix.to_ascii_lowercase());
    }
    ensure(
        matches!(
            extension.as_deref(),
            Some("png" | "jpg" | "jpeg" | "exr" | "tif" | "tiff" | "tga" | "bmp")
        ),
        "texture codec allowlist",
    )
}
pub fn finite(x: f64, low: f64, high: f64) -> Result<()> {
    ensure(
        x.is_finite() && (low..=high).contains(&x),
        "finite number outside domain bounds",
    )
}
fn vector(v: &[f64; 3], low: f64, high: f64) -> Result<()> {
    for x in v {
        finite(*x, low, high)?;
    }
    Ok(())
}
impl Transform {
    pub fn validate(&self) -> Result<()> {
        vector(&self.translation, -10_000.0, 10_000.0)?;
        vector(&self.rotation, -100.0, 100.0)?;
        vector(&self.scale, -100.0, 100.0)?;
        ensure(
            self.scale.iter().all(|x| x.abs() >= 0.0001),
            "singular or near-zero transform scale",
        )
    }
}
impl Shape {
    pub fn vertex_budget(&self) -> usize {
        match self {
            Self::Box { .. } => 8,
            Self::Cylinder { segments, .. } => *segments as usize * 2,
            Self::Mesh { vertices, .. } => vertices.len(),
            Self::Curve {
                points,
                bevel_resolution,
                ..
            } => points
                .len()
                .saturating_mul((*bevel_resolution as usize + 1).saturating_mul(8)),
            _ => 0,
        }
    }

    /// Source topology before non-destructive modifiers. Used only for typed
    /// mesh-attribute cardinality; evaluated topology is measured separately.
    pub fn mesh_topology_counts(&self) -> Option<(usize, usize, usize)> {
        match self {
            Self::Box { .. } => Some((8, 6, 24)),
            Self::Cylinder { segments, .. } => {
                let segments = *segments as usize;
                Some((segments * 2, segments + 2, segments * 6))
            }
            Self::Mesh {
                vertices, faces, ..
            } => Some((
                vertices.len(),
                faces.len(),
                faces.iter().map(Vec::len).sum(),
            )),
            _ => None,
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Box { size } => vector(size, 0.0001, 10_000.0)?,
            Self::Cylinder {
                radius,
                depth,
                segments,
            } => {
                finite(*radius, 0.0001, 1_000.0)?;
                finite(*depth, 0.0001, 10_000.0)?;
                ensure((3..=128).contains(segments), "cylinder tessellation budget")?;
            }
            Self::Mesh {
                vertices,
                faces,
                uv,
            } => {
                ensure(
                    (3..=MAX_VERTICES).contains(&vertices.len())
                        && (1..=MAX_FACES).contains(&faces.len()),
                    "mesh allocation budget",
                )?;
                for v in vertices {
                    vector(v, -10_000.0, 10_000.0)?;
                }
                let mut loops = 0;
                let mut seen = BTreeSet::new();
                for f in faces {
                    ensure((3..=32).contains(&f.len()), "face corner budget")?;
                    let ids: BTreeSet<_> = f.iter().copied().collect();
                    ensure(
                        ids.len() == f.len() && f.iter().all(|i| (*i as usize) < vertices.len()),
                        "invalid mesh indices or repeated corner",
                    )?;
                    ensure(seen.insert(ids), "duplicate vertex-set face")?;
                    // Nonzero fan area rejects collapsed polygons, not general self-intersection.
                    let a = vertices[f[0] as usize];
                    let area = f[1..]
                        .windows(2)
                        .map(|w| {
                            let b = vertices[w[0] as usize];
                            let c = vertices[w[1] as usize];
                            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                            let n = [
                                u[1] * v[2] - u[2] * v[1],
                                u[2] * v[0] - u[0] * v[2],
                                u[0] * v[1] - u[1] * v[0],
                            ];
                            n.iter().map(|x| x * x).sum::<f64>()
                        })
                        .sum::<f64>();
                    ensure(area > 1e-20, "collapsed face")?;
                    loops += f.len();
                }
                ensure(loops <= 32768, "mesh loop budget")?;
                if let Some(uv) = uv {
                    ensure(uv.len() == loops, "UV values must cover every face corner")?;
                    for p in uv {
                        for x in p {
                            finite(*x, -1000.0, 1000.0)?;
                        }
                    }
                }
            }
            Self::Armature { bones } => {
                ensure(!bones.is_empty() && bones.len() <= MAX_BONES, "bone budget")?;
                let mut parents = BTreeMap::new();
                for bone in bones {
                    local_id(&bone.id)?;
                    vector(&bone.head, -10_000.0, 10_000.0)?;
                    vector(&bone.tail, -10_000.0, 10_000.0)?;
                    ensure(
                        bone.head
                            .iter()
                            .zip(bone.tail)
                            .map(|(a, b)| (a - b) * (a - b))
                            .sum::<f64>()
                            >= 1e-12,
                        "zero-length bone",
                    )?;
                    ensure(
                        parents
                            .insert(bone.id.clone(), bone.parent.iter().cloned().collect())
                            .is_none(),
                        "duplicate bone ID",
                    )?;
                }
                dag_order(&parents)?;
            }
            Self::Camera {
                lens_mm,
                clip_start,
                clip_end,
            } => {
                finite(*lens_mm, 1.0, 500.0)?;
                finite(*clip_start, 0.0001, 1000.0)?;
                finite(*clip_end, 0.001, 100_000.0)?;
                ensure(clip_end > clip_start, "camera clip interval")?;
            }
            Self::AreaLight {
                energy_watts,
                size,
                color,
            } => {
                finite(*energy_watts, 0.0, 100_000.0)?;
                finite(*size, 0.0001, 1000.0)?;
                vector(color, 0.0, 1.0)?;
            }
            Self::MeshInstance { source } | Self::MeshCopy { source } => local_id(source)?,
            Self::Curve {
                points,
                cyclic,
                extrude,
                bevel_depth,
                bevel_resolution,
            } => {
                ensure((2..=256).contains(&points.len()), "curve point budget")?;
                if *cyclic {
                    ensure(points.len() >= 3, "cyclic curve requires three points")?;
                }
                for point in points {
                    vector(point, -10_000.0, 10_000.0)?;
                }
                finite(*extrude, 0.0, 1000.0)?;
                finite(*bevel_depth, 0.0, 1000.0)?;
                ensure(
                    *extrude > 0.0 || *bevel_depth > 0.0,
                    "curve requires explicit extrusion or bevel geometry",
                )?;
                ensure(*bevel_resolution <= 8, "curve bevel resolution budget")?;
            }
            Self::Empty => {}
        }
        Ok(())
    }
}
/// Deterministic topological order. Missing refs and cycles are separate from object naming.
pub fn dag_order(graph: &BTreeMap<String, Vec<String>>) -> Result<Vec<String>> {
    ensure(graph.len() <= 4096, "dependency graph budget")?;
    let mut emitted = BTreeSet::new();
    let mut order = Vec::new();
    for deps in graph.values() {
        ensure(
            deps.iter().all(|id| graph.contains_key(id)),
            "missing dependency",
        )?;
    }
    while order.len() < graph.len() {
        let next = graph
            .iter()
            .find(|(id, deps)| !emitted.contains(*id) && deps.iter().all(|d| emitted.contains(d)))
            .map(|(id, _)| id.clone());
        let Some(id) = next else {
            return Err(semwright_semantic_composition::ContractError::Invalid(
                "dependency cycle".into(),
            ));
        };
        emitted.insert(id.clone());
        order.push(id);
    }
    Ok(order)
}

impl MeshAttributeData {
    fn len(&self) -> usize {
        match self {
            Self::Float { values } => values.len(),
            Self::Vector { values } => values.len(),
            Self::Color { values } => values.len(),
        }
    }

    fn validate_values(&self) -> Result<()> {
        match self {
            Self::Float { values } => {
                for value in values {
                    finite(*value, -1_000_000.0, 1_000_000.0)?;
                }
            }
            Self::Vector { values } => {
                for value in values {
                    vector(value, -1_000_000.0, 1_000_000.0)?;
                }
            }
            Self::Color { values } => {
                for value in values {
                    for component in value {
                        finite(*component, 0.0, 1.0)?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn validate_mesh_attributes(
    attributes: &[MeshAttribute],
    counts: (usize, usize, usize),
) -> Result<()> {
    ensure(attributes.len() <= 16, "mesh attribute count budget")?;
    let mut ids = BTreeSet::new();
    let mut total_values = 0usize;
    for attribute in attributes {
        local_id(&attribute.id)?;
        ensure(
            ids.insert(attribute.id.clone()),
            "duplicate mesh attribute ID",
        )?;
        attribute.data.validate_values()?;
        let expected = match attribute.domain {
            MeshAttributeDomain::Point => counts.0,
            MeshAttributeDomain::Face => counts.1,
            MeshAttributeDomain::Corner => counts.2,
        };
        ensure(
            attribute.data.len() == expected,
            "mesh attribute cardinality must match declared domain",
        )?;
        total_values = total_values.saturating_add(expected);
    }
    ensure(total_values <= 65_536, "mesh attribute value budget")
}

impl BlenderAuthoringSpec {
    pub fn validate(&self) -> Result<()> {
        ensure(self.version == 1, "unknown BlenderAuthoringSpec version")?;
        local_id(&self.collection)?;
        finite(self.meters_per_unit, 0.0001, 100.0)?;
        ensure(
            !self.entities.is_empty()
                && self.entities.len() <= MAX_ENTITIES
                && self.textures.len() <= 32
                && self.materials.len() <= 64
                && self.relations.len() <= 256,
            "authoring table budget",
        )?;
        let mut textures = BTreeMap::new();
        for texture in &self.textures {
            local_id(&texture.id)?;
            relative_asset_path(&texture.path)?;
            ensure(
                texture.sha256.len() == 64
                    && texture
                        .sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "texture SHA-256 must be lowercase hex",
            )?;
            ensure(
                textures.insert(texture.id.clone(), texture).is_none(),
                "duplicate texture ID",
            )?;
        }
        let mut materials = BTreeSet::new();
        for m in &self.materials {
            local_id(&m.id)?;
            ensure(materials.insert(m.id.clone()), "duplicate material ID")?;
            for x in m.base_color {
                finite(x, 0.0, 1.0)?;
            }
            ensure(
                m.base_color[3] == 1.0,
                "transparent material fidelity requires the subsequent material-graph increment",
            )?;
            finite(m.roughness, 0.0, 1.0)?;
            finite(m.metallic, 0.0, 1.0)?;
            for x in m.emission_color {
                finite(x, 0.0, 1.0)?;
            }
            ensure(
                m.emission_color[3] == 1.0,
                "emission color alpha is not an opacity channel",
            )?;
            finite(m.emission_strength, 0.0, 10.0)?;
            finite(m.opacity, 0.0, 1.0)?;
            for binding in [&m.base_color_texture, &m.emission_texture]
                .into_iter()
                .flatten()
            {
                local_id(&binding.texture)?;
                ensure(
                    binding.channel == TextureChannel::Color,
                    "color texture binding requires color channel",
                )?;
                ensure(
                    textures
                        .get(&binding.texture)
                        .is_some_and(|texture| texture.color_space == TextureColorSpace::Srgb),
                    "color texture requires declared sRGB asset",
                )?;
            }
            for binding in [
                &m.roughness_texture,
                &m.metallic_texture,
                &m.opacity_texture,
            ]
            .into_iter()
            .flatten()
            {
                local_id(&binding.texture)?;
                ensure(
                    binding.channel != TextureChannel::Color,
                    "scalar texture binding requires explicit component channel",
                )?;
                ensure(
                    textures
                        .get(&binding.texture)
                        .is_some_and(|texture| texture.color_space == TextureColorSpace::NonColor),
                    "scalar texture requires declared non-color asset",
                )?;
            }
            if let Some(binding) = &m.normal_texture {
                local_id(&binding.texture)?;
                finite(binding.strength, 0.0, 4.0)?;
                ensure(
                    textures
                        .get(&binding.texture)
                        .is_some_and(|texture| texture.color_space == TextureColorSpace::NonColor),
                    "normal texture requires declared non-color asset",
                )?;
            }
        }
        let mut entities = BTreeMap::new();
        let mut deps = BTreeMap::<String, Vec<String>>::new();
        let mut multipliers = BTreeMap::<String, usize>::new();
        let mut predicted_vertices = 0usize;
        for entity in &self.entities {
            local_id(&entity.id)?;
            ensure(
                !entity.name.is_empty()
                    && entity.name.len() <= 80
                    && !entity.name.chars().any(char::is_control),
                "native display name budget",
            )?;
            ensure(
                entities.insert(entity.id.clone(), entity).is_none(),
                "duplicate entity ID",
            )?;
            entity.shape.validate()?;
            entity.transform.validate()?;
            ensure(
                entity.materials.len() <= 8
                    && entity.materials.iter().all(|id| materials.contains(id)),
                "missing material or slot budget",
            )?;
            ensure(entity.modifiers.len() <= 8, "modifier budget")?;
            ensure(
                entity.materials.is_empty()
                    || matches!(
                        entity.shape,
                        Shape::Box { .. }
                            | Shape::Cylinder { .. }
                            | Shape::Mesh { .. }
                            | Shape::MeshCopy { .. }
                            | Shape::Curve { .. }
                    ),
                "material writes require an owned mesh, explicit mesh_copy, or curve",
            )?;
            match &entity.shape {
                Shape::Box { .. } | Shape::Cylinder { .. } | Shape::Mesh { .. } => {
                    validate_mesh_attributes(
                        &entity.attributes,
                        entity
                            .shape
                            .mesh_topology_counts()
                            .expect("concrete mesh counts"),
                    )?;
                }
                Shape::MeshCopy { .. } => {
                    // Cardinality is validated after the source shape is resolved.
                    ensure(entity.attributes.len() <= 16, "mesh attribute count budget")?;
                    for attribute in &entity.attributes {
                        local_id(&attribute.id)?;
                        attribute.data.validate_values()?;
                    }
                }
                Shape::MeshInstance { .. } => {
                    ensure(
                        entity.attributes.is_empty() && !entity.shade_smooth,
                        "shared mesh instance cannot mutate attributes or shading",
                    )?;
                }
                _ => ensure(
                    entity.attributes.is_empty() && !entity.shade_smooth,
                    "mesh attributes/shading require owned mesh geometry",
                )?,
            }
            let mut multiplier = 1usize;
            for modifier in &entity.modifiers {
                ensure(
                    matches!(
                        entity.shape,
                        Shape::Box { .. }
                            | Shape::Cylinder { .. }
                            | Shape::Mesh { .. }
                            | Shape::MeshCopy { .. }
                    ),
                    "modifier requires an owned or explicitly copied mesh",
                )?;
                match modifier {
                    Modifier::Bevel { width, segments } => {
                        finite(*width, 0.00001, 100.0)?;
                        ensure((1..=4).contains(segments), "bevel segment budget")?;
                        multiplier = multiplier.saturating_mul(32);
                    }
                    Modifier::Mirror { axes } => {
                        ensure(axes.iter().any(|v| *v), "mirror has no axis")?;
                        multiplier =
                            multiplier.saturating_mul(1 << axes.iter().filter(|v| **v).count());
                    }
                    Modifier::Subdivision { levels } => {
                        ensure(*levels <= 2, "subdivision budget")?;
                        multiplier = multiplier.saturating_mul(4usize.pow(*levels));
                    }
                    Modifier::Array { count, offset } => {
                        ensure((1..=16).contains(count), "array budget")?;
                        vector(offset, -1000.0, 1000.0)?;
                        multiplier = multiplier.saturating_mul(*count as usize);
                    }
                    Modifier::Boolean { target, .. } => {
                        local_id(target)?;
                        ensure(target != &entity.id, "boolean cannot target itself")?;
                        multiplier = multiplier.saturating_mul(4);
                    }
                }
            }
            predicted_vertices = predicted_vertices
                .saturating_add(entity.shape.vertex_budget().saturating_mul(multiplier));
            multipliers.insert(entity.id.clone(), multiplier);
            deps.insert(entity.id.clone(), entity.dependency_ids());
        }
        for entity in &self.entities {
            if let Shape::MeshInstance { source } | Shape::MeshCopy { source } = &entity.shape {
                let source_entity = entities.get(source).ok_or_else(|| {
                    semwright_semantic_composition::ContractError::Invalid(
                        "instance/copy source is missing".into(),
                    )
                })?;
                ensure(
                    matches!(
                        source_entity.shape,
                        Shape::Box { .. } | Shape::Cylinder { .. } | Shape::Mesh { .. }
                    ),
                    "instance/copy source must be a concrete owned mesh",
                )?;
                predicted_vertices = predicted_vertices.saturating_add(
                    source_entity
                        .shape
                        .vertex_budget()
                        .saturating_mul(*multipliers.get(&entity.id).unwrap_or(&1)),
                );
                if matches!(entity.shape, Shape::MeshCopy { .. }) {
                    validate_mesh_attributes(
                        &entity.attributes,
                        source_entity
                            .shape
                            .mesh_topology_counts()
                            .expect("validated concrete copy source"),
                    )?;
                }
            }
        }
        for entity in &self.entities {
            for modifier in &entity.modifiers {
                if let Modifier::Boolean { target, .. } = modifier {
                    let target = entities.get(target).ok_or_else(|| {
                        semwright_semantic_composition::ContractError::Invalid(
                            "boolean target is missing".into(),
                        )
                    })?;
                    ensure(
                        matches!(
                            target.shape,
                            Shape::Box { .. }
                                | Shape::Cylinder { .. }
                                | Shape::Mesh { .. }
                                | Shape::MeshCopy { .. }
                        ),
                        "boolean target must be a managed mesh object",
                    )?;
                }
            }
        }
        ensure(
            predicted_vertices <= 262_144,
            "conservative evaluated geometry budget",
        )?;
        let mut parented = BTreeSet::new();
        let mut skinned = BTreeSet::new();
        for relation in &self.relations {
            let (subject, target) = match relation {
                Relation::Parent { child, parent } => {
                    ensure(parented.insert(child), "multiple native parents")?;
                    (child, parent)
                }
                Relation::BoneParent {
                    child,
                    armature,
                    bone,
                } => {
                    ensure(parented.insert(child), "multiple native parents")?;
                    check_bone(&entities, armature, bone)?;
                    (child, armature)
                }
                Relation::Follow {
                    subject, target, ..
                }
                | Relation::LookAt { subject, target } => (subject, target),
                Relation::Align {
                    subject,
                    target,
                    axes,
                    ..
                } => {
                    ensure(
                        axes.iter().any(|axis| *axis),
                        "align requires at least one axis",
                    )?;
                    (subject, target)
                }
                Relation::Skin {
                    mesh,
                    armature,
                    weights,
                } => {
                    ensure(skinned.insert(mesh), "multiple skin bindings")?;
                    let Some(entity) = entities.get(mesh) else {
                        return Err(semwright_semantic_composition::ContractError::Invalid(
                            "missing skin mesh".into(),
                        ));
                    };
                    ensure(
                        matches!(
                            entity.shape,
                            Shape::Box { .. } | Shape::Cylinder { .. } | Shape::Mesh { .. }
                        ) && entity.modifiers.is_empty(),
                        "skinning requires concrete source geometry without prior topology modifiers",
                    )?;
                    ensure(
                        !weights.is_empty() && weights.len() <= 32768,
                        "weight budget",
                    )?;
                    let mut totals = BTreeMap::<u32, f64>::new();
                    let mut seen = BTreeSet::new();
                    for weight in weights {
                        check_bone(&entities, armature, &weight.bone)?;
                        finite(weight.weight, 0.0, 1.0)?;
                        ensure(
                            (weight.vertex as usize) < entity.shape.vertex_budget()
                                && seen.insert((weight.vertex, &weight.bone)),
                            "duplicate weight or vertex outside topology",
                        )?;
                        *totals.entry(weight.vertex).or_default() += weight.weight;
                    }
                    ensure(
                        totals.len() == entity.shape.vertex_budget()
                            && totals.values().all(|x| (x - 1.0).abs() <= 1e-6),
                        "skin weights must cover vertices and sum to one",
                    )?;
                    (mesh, armature)
                }
            };
            ensure(
                subject != target
                    && entities.contains_key(subject)
                    && entities.contains_key(target),
                "missing relationship subject/target or self relation",
            )?;
            deps.get_mut(subject)
                .expect("validated subject")
                .push(target.clone());
        }
        dag_order(&deps)?;
        if let Some(animation) = &self.animation {
            local_id(&animation.id)?;
            animation.rate.validate()?;
            ensure(
                animation.rate.num <= 240 && animation.rate.den <= 1001,
                "native frame-rate realization range",
            )?;
            ensure(
                !animation.channels.is_empty() && animation.channels.len() <= 128,
                "animation channel budget",
            )?;
            let mut channels = BTreeSet::new();
            let mut frame_ranges = BTreeMap::<String, (i32, i32)>::new();
            let mut keys = 0;
            for channel in &animation.channels {
                ensure(
                    entities.contains_key(&channel.entity)
                        && channels.insert((&channel.entity, &channel.bone, channel.property)),
                    "missing or duplicate animation channel",
                )?;
                if let Some(bone) = &channel.bone {
                    check_bone(&entities, &channel.entity, bone)?;
                }
                ensure(
                    !channel.keys.is_empty()
                        && channel.keys.windows(2).all(|w| w[0].frame < w[1].frame),
                    "animation keys must be nonempty and strictly ordered",
                )?;
                for key in &channel.keys {
                    ensure((0..=100_000).contains(&key.frame), "frame outside range")?;
                    animation.rate.at(key.frame.into())?;
                    vector(&key.value, -10_000.0, 10_000.0)?;
                    if channel.property == AnimatedProperty::Scale {
                        ensure(
                            key.value.iter().all(|x| x.abs() >= 0.0001),
                            "singular scale key",
                        )?;
                    }
                }
                let first = channel.keys.first().expect("nonempty keys").frame;
                let last = channel.keys.last().expect("nonempty keys").frame;
                frame_ranges
                    .entry(channel.entity.clone())
                    .and_modify(|range| {
                        range.0 = range.0.min(first);
                        range.1 = range.1.max(last);
                    })
                    .or_insert((first, last));
                keys += channel.keys.len();
            }
            ensure(keys <= MAX_KEYS, "keyframe budget")?;
            ensure(animation.nla_tracks.len() <= 16, "NLA track budget")?;
            let mut track_ids = BTreeSet::new();
            let mut total_strips = 0usize;
            for track in &animation.nla_tracks {
                local_id(&track.id)?;
                local_id(&track.entity)?;
                ensure(track_ids.insert(track.id.clone()), "duplicate NLA track ID")?;
                let Some((action_start, action_end)) = frame_ranges.get(&track.entity) else {
                    return Err(semwright_semantic_composition::ContractError::Invalid(
                        "NLA track entity has no managed animation channels".into(),
                    ));
                };
                ensure(
                    !track.strips.is_empty() && track.strips.len() <= 16,
                    "NLA strip budget",
                )?;
                let mut strip_ids = BTreeSet::new();
                for strip in &track.strips {
                    local_id(&strip.id)?;
                    ensure(strip_ids.insert(strip.id.clone()), "duplicate NLA strip ID")?;
                    ensure(
                        (0..=100_000).contains(&strip.start_frame)
                            && strip.action_frame_start >= *action_start
                            && strip.action_frame_end <= *action_end
                            && strip.action_frame_start < strip.action_frame_end,
                        "NLA frame range outside managed Action",
                    )?;
                    finite(strip.repeat, 0.1, 32.0)?;
                    finite(strip.scale, 0.01, 32.0)?;
                    finite(strip.influence, 0.0, 1.0)?;
                    let end = strip.start_frame as f64
                        + (strip.action_frame_end - strip.action_frame_start) as f64
                            * strip.repeat
                            * strip.scale;
                    ensure(
                        end.is_finite() && end <= 100_000.0,
                        "NLA evaluated frame budget",
                    )?;
                    total_strips += 1;
                }
            }
            ensure(total_strips <= 64, "total NLA strip budget")?;
        }
        ensure(
            self.operation_count() <= MAX_OPERATIONS,
            "compiled operation budget",
        )?;
        canonical_bytes(self)?;
        Ok(())
    }
    pub fn operation_count(&self) -> usize {
        1 + self.textures.len()
            + self.materials.len()
            + self.entities.len()
            + self.relations.len()
            + usize::from(self.animation.is_some())
    }
}
fn check_bone(entities: &BTreeMap<String, &Entity>, rig: &str, bone: &str) -> Result<()> {
    ensure(
        entities.get(rig).is_some_and(
            |e| matches!(&e.shape,Shape::Armature{bones} if bones.iter().any(|b| b.id == bone)),
        ),
        "missing armature or bone",
    )
}
