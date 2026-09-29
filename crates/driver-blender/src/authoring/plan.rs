use super::*;
use schemars::JsonSchema;
use semwright_semantic_composition::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthoringIntent {
    Create {
        spec: BlenderAuthoringSpec,
    },
    /// Incremental edit, not a destructive regeneration of the model/rig/materials.
    Transform {
        island: String,
        entity: String,
        transform: Transform,
        meters_per_unit: f64,
        expected_fingerprint: Digest,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeOperation {
    Collection {
        island: String,
        name: String,
    },
    Texture {
        island: String,
        texture: TextureAsset,
    },
    Material {
        island: String,
        material: Material,
    },
    Entity {
        island: String,
        entity: Entity,
        meters_per_unit: f64,
    },
    Relation {
        island: String,
        relation: Relation,
    },
    Animation {
        island: String,
        animation: Animation,
        meters_per_unit: f64,
    },
    Transform {
        island: String,
        entity: String,
        transform: Transform,
        meters_per_unit: f64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeSnapshot {
    pub native_session: String,
    pub island: Option<String>,
    pub fingerprint: Digest,
    pub drift: bool,
    pub total: usize,
    pub items: Vec<Value>,
    pub source_only: bool,
    pub exhaustive: bool,
}
impl NativeSnapshot {
    pub fn base(&self, owner: &Owner) -> Result<BaseStateSet> {
        ensure(
            self.source_only && self.exhaustive && self.total == self.items.len(),
            "planning requires complete source-only native snapshot",
        )?;
        ensure(self.items.len() <= 512, "native snapshot item bound")?;
        bounded_id(&self.native_session)?;
        Ok(BaseStateSet(vec![BaseState {
            // One stable resource key spans whole-scene planning and managed-island readback.
            // The native island remains an observed property, not a fabricated global revision.
            key: ResourceKey {
                provider: "driver:blender".into(),
                resource: "authoring-workspace".into(),
            },
            document_id: "blender-authoring-workspace".into(),
            provider_session: owner.session.clone(),
            generation: self.native_session.clone(),
            revision: Revision::Fingerprint(self.fingerprint.clone()),
            concurrency: Concurrency::BestEffortRevalidate,
        }]))
    }
}
pub fn profile(
    bindings: Vec<CapabilityBinding>,
    required_rules: BTreeSet<String>,
) -> Result<ProfileDescriptor> {
    let p = ProfileDescriptor {
        identity: ProfileIdentity {
            id: "blender-native-authoring".into(),
            version: 1,
            intent_schema: schema_digest::<AuthoringIntent>()?,
            operation_schema: schema_digest::<NativeOperation>()?,
        },
        capabilities: bindings,
        required_rules,
        allowed_effects: [
            EffectClass::Inspect,
            EffectClass::CreateOwnedObject,
            EffectClass::UpdateOwnedObject,
        ]
        .into(),
    };
    p.validate()?;
    Ok(p)
}
fn prepare_internal(
    owner: Owner,
    intent: AuthoringIntent,
    snapshot: &NativeSnapshot,
    new_island: String,
    bindings: Vec<CapabilityBinding>,
    allow_observed_drift: bool,
) -> Result<PreparedAuthoring> {
    let base = snapshot.base(&owner)?;
    let mut payloads = Vec::new();
    match &intent {
        AuthoringIntent::Create { spec } => {
            spec.validate()?;
            local_id(&new_island)?;
            ensure(
                snapshot.island.is_none(),
                "new collection plan requires whole source scene observation",
            )?;
            payloads.push((
                "collection".into(),
                NativeOperation::Collection {
                    island: new_island.clone(),
                    name: spec.collection.clone(),
                },
            ));
            for texture in &spec.textures {
                payloads.push((
                    format!("texture-{}", texture.id),
                    NativeOperation::Texture {
                        island: new_island.clone(),
                        texture: texture.clone(),
                    },
                ));
            }
            for material in &spec.materials {
                payloads.push((
                    format!("material-{}", material.id),
                    NativeOperation::Material {
                        island: new_island.clone(),
                        material: material.clone(),
                    },
                ));
            }
            let graph: BTreeMap<_, _> = spec
                .entities
                .iter()
                .map(|entity| (entity.id.clone(), entity.dependency_ids()))
                .collect();
            for id in dag_order(&graph)? {
                let entity = spec
                    .entities
                    .iter()
                    .find(|entity| entity.id == id)
                    .expect("validated entity")
                    .clone();
                payloads.push((
                    format!("entity-{id}"),
                    NativeOperation::Entity {
                        island: new_island.clone(),
                        entity,
                        meters_per_unit: spec.meters_per_unit,
                    },
                ));
            }
            for (index, relation) in spec.relations.iter().enumerate() {
                payloads.push((
                    format!("relation-{index}"),
                    NativeOperation::Relation {
                        island: new_island.clone(),
                        relation: relation.clone(),
                    },
                ));
            }
            if let Some(animation) = &spec.animation {
                payloads.push((
                    "animation".into(),
                    NativeOperation::Animation {
                        island: new_island.clone(),
                        animation: animation.clone(),
                        meters_per_unit: spec.meters_per_unit,
                    },
                ));
            }
        }
        AuthoringIntent::Transform {
            island,
            entity,
            transform,
            meters_per_unit,
            expected_fingerprint,
        } => {
            local_id(island)?;
            local_id(entity)?;
            transform.validate()?;
            finite(*meters_per_unit, 0.0001, 100.0)?;
            ensure(
                snapshot.island.as_deref() == Some(island)
                    && (allow_observed_drift || !snapshot.drift),
                "managed collection identity or manual-edit drift",
            )?;
            ensure(
                &snapshot.fingerprint == expected_fingerprint,
                "external edit invalidates transform plan",
            )?;
            ensure(
                snapshot
                    .items
                    .iter()
                    .filter(|row| row.get("entity").and_then(Value::as_str) == Some(entity))
                    .count()
                    == 1,
                "entity identity ambiguous or absent",
            )?;
            payloads.push((
                format!("transform-{entity}"),
                NativeOperation::Transform {
                    island: island.clone(),
                    entity: entity.clone(),
                    transform: transform.clone(),
                    meters_per_unit: *meters_per_unit,
                },
            ));
        }
    }
    let resource = base.0[0].key.clone();
    let mut operations = Vec::new();
    let mut previous = None;
    for (id, payload) in payloads {
        let effect = if matches!(
            payload,
            NativeOperation::Transform { .. }
                | NativeOperation::Relation { .. }
                | NativeOperation::Animation { .. }
        ) {
            EffectClass::UpdateOwnedObject
        } else {
            EffectClass::CreateOwnedObject
        };
        let address = Address {
            resource: resource.clone(),
            logical_id: id.clone(),
            property: "declared-native-state".into(),
        };
        operations.push(TypedOperation {
            id: id.clone(),
            payload,
            reads: vec![address.clone()],
            writes: vec![address],
            effects: [effect].into(),
            depends_on: previous.into_iter().collect(),
            postconditions: BTreeSet::new(),
        });
        previous = Some(id);
    }
    let mut changes = ChangeSet {
        operations,
        atomicity: Atomicity::NonAtomicSequence,
    };
    let contract = effect_contract(&intent, &changes)?;
    let required_rules = contract.required_rules();
    if let Some(final_operation) = changes.operations.last_mut() {
        final_operation.postconditions = required_rules.clone();
    }
    let descriptor = profile(bindings, required_rules.clone())?;
    let budget = ConvergenceBudget {
        // Root apply plus at most two explicitly requested repair attempts. Child
        // repairs inherit this root budget in A's PlanVault; no reset is allowed.
        max_iterations: 3,
        max_operations: MAX_OPERATIONS as u32,
        max_findings: 512,
        max_observations: 64,
        max_elapsed_ms: 300_000,
    };
    let dependencies = BTreeMap::from([("effects.contract".into(), contract.digest()?)]);
    let plan = PreparedPlan::prepare(
        PlanBody {
            contract_version: CONTRACT_VERSION,
            profile: descriptor.identity.clone(),
            owner,
            base,
            intent_digest: canonical_digest(&intent)?,
            intent,
            dependencies,
            observation_scope: changes
                .operations
                .iter()
                .flat_map(|operation| operation.writes.clone())
                .collect(),
            changes,
            required_rules,
            budget,
            require_compare_and_swap: false,
        },
        &descriptor,
    )?;
    Ok(PreparedAuthoring {
        plan,
        profile: descriptor,
        contract,
    })
}

pub fn prepare(
    owner: Owner,
    intent: AuthoringIntent,
    snapshot: &NativeSnapshot,
    new_island: String,
    bindings: Vec<CapabilityBinding>,
) -> Result<PreparedAuthoring> {
    prepare_internal(owner, intent, snapshot, new_island, bindings, false)
}

/// Narrow repair planner. It accepts only a caller-explicit Transform intent over
/// a fresh exact fingerprint. Observed managed-island drift may be repaired, but
/// the repair cannot create/delete entities, alter topology/materials/rigs, or
/// widen the plan's native operation class.
pub fn prepare_repair(
    owner: Owner,
    intent: AuthoringIntent,
    snapshot: &NativeSnapshot,
    bindings: Vec<CapabilityBinding>,
) -> Result<PreparedAuthoring> {
    ensure(
        matches!(intent, AuthoringIntent::Transform { .. }),
        "repair v1 only supports an explicit transform intent",
    )?;
    prepare_internal(
        owner,
        intent,
        snapshot,
        "repair-does-not-create-islands".into(),
        bindings,
        true,
    )
}

pub fn phases() -> BTreeSet<Phase> {
    [
        Phase::Inspect,
        Phase::Plan,
        Phase::Apply,
        Phase::Measure,
        Phase::Validate,
        Phase::RepairPlan,
        Phase::RepairApply,
        Phase::Verify,
    ]
    .into()
}

fn near(actual: &Value, expected: &[f64]) -> bool {
    actual.as_array().is_some_and(|a| {
        a.len() == expected.len()
            && a.iter().zip(expected).all(|(x, y)| {
                x.as_f64()
                    .is_some_and(|x| (x - y).abs() <= 1e-5 * y.abs().max(1.0))
            })
    })
}
fn native_transform(row: &Value, expected: &Transform, units: f64) -> bool {
    near(
        &row["translation"],
        &expected.translation.map(|x| x * units),
    ) && near(&row["rotation"], &expected.rotation)
        && near(&row["scale"], &expected.scale)
}
/// Validate specifically enumerated source-RNA fields, not global artistry/effect correctness.
/// Material links, frame-domain coverage, modifier fidelity and persistence remain separate gates.
pub fn native_matches(intent: &AuthoringIntent, snapshot: &NativeSnapshot) -> bool {
    if !snapshot.exhaustive || snapshot.total != snapshot.items.len() {
        return false;
    }
    let row = |id: &str| {
        snapshot
            .items
            .iter()
            .find(|r| r["entity"].as_str() == Some(id))
    };
    match intent {
        AuthoringIntent::Transform {
            entity,
            transform,
            meters_per_unit,
            ..
        } => row(entity).is_some_and(|r| native_transform(r, transform, *meters_per_unit)),
        AuthoringIntent::Create { spec } => {
            if snapshot.total != spec.entities.len() {
                return false;
            }
            for entity in &spec.entities {
                let Some(r) = row(&entity.id) else {
                    return false;
                };
                if !native_transform(r, &entity.transform, spec.meters_per_unit) {
                    return false;
                }
                let expected_type = match &entity.shape {
                    Shape::Empty => "EMPTY",
                    Shape::Armature { .. } => "ARMATURE",
                    Shape::Camera { .. } => "CAMERA",
                    Shape::AreaLight { .. } => "LIGHT",
                    Shape::Curve { .. } => "CURVE",
                    _ => "MESH",
                };
                if r["type"].as_str() != Some(expected_type) {
                    return false;
                }
                let expected_vertices = match &entity.shape {
                    Shape::Box { .. } | Shape::Cylinder { .. } | Shape::Mesh { .. } => {
                        Some(entity.shape.vertex_budget())
                    }
                    Shape::MeshInstance { source } | Shape::MeshCopy { source } => spec
                        .entities
                        .iter()
                        .find(|candidate| &candidate.id == source)
                        .map(|candidate| candidate.shape.vertex_budget()),
                    _ => None,
                };
                if expected_vertices
                    .is_some_and(|count| r["vertices"].as_u64() != Some(count as u64))
                {
                    return false;
                }
                match &entity.shape {
                    Shape::MeshInstance { source } => {
                        let Some(source_row) = row(source) else {
                            return false;
                        };
                        if r["data_name"] != source_row["data_name"] {
                            return false;
                        }
                    }
                    Shape::MeshCopy { source } => {
                        let Some(source_row) = row(source) else {
                            return false;
                        };
                        if r["data_name"] == source_row["data_name"]
                            || r["data_users"].as_u64() != Some(1)
                        {
                            return false;
                        }
                    }
                    Shape::Curve {
                        points,
                        cyclic,
                        extrude,
                        bevel_depth,
                        bevel_resolution,
                    } => {
                        let curve = &r["curve"];
                        if curve["cyclic"].as_bool() != Some(*cyclic)
                            || curve["bevel_resolution"].as_u64() != Some(*bevel_resolution as u64)
                            || !curve["extrude"].as_f64().is_some_and(|value| {
                                (value - extrude * spec.meters_per_unit).abs() < 1e-5
                            })
                            || !curve["bevel_depth"].as_f64().is_some_and(|value| {
                                (value - bevel_depth * spec.meters_per_unit).abs() < 1e-5
                            })
                            || curve["points"].as_array().is_none_or(|actual| {
                                actual.len() != points.len()
                                    || actual.iter().zip(points).any(|(actual, expected)| {
                                        !near(
                                            actual,
                                            &expected.map(|value| value * spec.meters_per_unit),
                                        )
                                    })
                            })
                        {
                            return false;
                        }
                    }
                    _ => {}
                }
                if let Shape::Armature { bones } = &entity.shape {
                    let Some(actual) = r["bones"].as_array() else {
                        return false;
                    };
                    if actual.len() != bones.len() {
                        return false;
                    }
                    for bone in bones {
                        let Some(b) = actual.iter().find(|b| b["id"].as_str() == Some(&bone.id))
                        else {
                            return false;
                        };
                        if b["parent"].as_str() != bone.parent.as_deref()
                            || !near(&b["head"], &bone.head.map(|v| v * spec.meters_per_unit))
                            || !near(&b["tail"], &bone.tail.map(|v| v * spec.meters_per_unit))
                        {
                            return false;
                        }
                    }
                }
                if !entity.materials.is_empty() {
                    let Some(actual) = r["materials"].as_array() else {
                        return false;
                    };
                    if actual.len() != entity.materials.len() {
                        return false;
                    }
                    for (id, m) in entity.materials.iter().zip(actual) {
                        let Some(expected) = spec.materials.iter().find(|v| &v.id == id) else {
                            return false;
                        };
                        let expected_diffuse = [
                            expected.base_color[0],
                            expected.base_color[1],
                            expected.base_color[2],
                            expected.opacity,
                        ];
                        if m["id"].as_str() != Some(id)
                            || !near(&m["color"], &expected_diffuse)
                            || !near(&m["base_color"], &expected.base_color)
                            || !near(&m["emission_color"], &expected.emission_color)
                            || !m["opacity"]
                                .as_f64()
                                .is_some_and(|value| (value - expected.opacity).abs() < 1e-5)
                            || !m["emission_strength"].as_f64().is_some_and(|value| {
                                (value - expected.emission_strength).abs() < 1e-5
                            })
                            || !m["roughness"]
                                .as_f64()
                                .is_some_and(|x| (x - expected.roughness).abs() < 1e-5)
                            || !m["metallic"]
                                .as_f64()
                                .is_some_and(|x| (x - expected.metallic).abs() < 1e-5)
                        {
                            return false;
                        }
                        let Some(actual_bindings) = m["texture_bindings"].as_array() else {
                            return false;
                        };
                        let expected_asset = |texture_id: &str| {
                            spec.textures
                                .iter()
                                .find(|texture| texture.id == texture_id)
                        };
                        let channel_name = |channel: TextureChannel| match channel {
                            TextureChannel::Color => "color",
                            TextureChannel::Red => "red",
                            TextureChannel::Green => "green",
                            TextureChannel::Blue => "blue",
                            TextureChannel::Alpha => "alpha",
                        };
                        let color_space_name = |space: TextureColorSpace| match space {
                            TextureColorSpace::Srgb => "sRGB",
                            TextureColorSpace::NonColor => "Non-Color",
                        };
                        for (role, binding) in [
                            ("base_color", expected.base_color_texture.as_ref()),
                            ("roughness", expected.roughness_texture.as_ref()),
                            ("metallic", expected.metallic_texture.as_ref()),
                            ("emission", expected.emission_texture.as_ref()),
                            ("opacity", expected.opacity_texture.as_ref()),
                        ] {
                            if let Some(binding) = binding {
                                let Some(asset) = expected_asset(&binding.texture) else {
                                    return false;
                                };
                                if !actual_bindings.iter().any(|actual| {
                                    actual["role"].as_str() == Some(role)
                                        && actual["texture"].as_str()
                                            == Some(binding.texture.as_str())
                                        && actual["channel"].as_str()
                                            == Some(channel_name(binding.channel))
                                        && actual["colorspace"].as_str()
                                            == Some(color_space_name(asset.color_space))
                                        && actual["sha256"].as_str() == Some(asset.sha256.as_str())
                                        && actual["topology_valid"].as_bool() == Some(true)
                                }) {
                                    return false;
                                }
                            }
                        }
                        if let Some(binding) = &expected.normal_texture {
                            let Some(asset) = expected_asset(&binding.texture) else {
                                return false;
                            };
                            if !actual_bindings.iter().any(|actual| {
                                actual["role"] == "normal"
                                    && actual["texture"].as_str() == Some(binding.texture.as_str())
                                    && actual["channel"] == "color"
                                    && actual["colorspace"].as_str()
                                        == Some(color_space_name(asset.color_space))
                                    && actual["sha256"].as_str() == Some(asset.sha256.as_str())
                                    && actual["topology_valid"].as_bool() == Some(true)
                                    && actual["strength"].as_f64().is_some_and(|value| {
                                        (value - binding.strength).abs() < 1e-5
                                    })
                            }) {
                                return false;
                            }
                        }
                    }
                }
                let skin_count = spec
                    .relations
                    .iter()
                    .filter(|r| matches!(r,Relation::Skin{mesh,..} if mesh==&entity.id))
                    .count();
                let Some(native_modifiers) = r["modifiers"].as_array() else {
                    return false;
                };
                if native_modifiers.len() != entity.modifiers.len() + skin_count {
                    return false;
                }
                for modifier in &entity.modifiers {
                    if let Modifier::Boolean { operation, target } = modifier {
                        let expected_operation = match operation {
                            BooleanOperation::Difference => "DIFFERENCE",
                            BooleanOperation::Union => "UNION",
                            BooleanOperation::Intersect => "INTERSECT",
                        };
                        if !native_modifiers.iter().any(|row| {
                            row["type"] == "BOOLEAN"
                                && row["target"].as_str() == Some(target)
                                && row["operation"].as_str() == Some(expected_operation)
                                && row["solver"] == "EXACT"
                        }) {
                            return false;
                        }
                    }
                }
            }
            for relation in &spec.relations {
                match relation {
                    Relation::Parent { child, parent } => {
                        if row(child).is_none_or(|r| r["parent"].as_str() != Some(parent)) {
                            return false;
                        }
                    }
                    Relation::BoneParent {
                        child,
                        armature,
                        bone,
                    } => {
                        if row(child).is_none_or(|r| {
                            r["parent"].as_str() != Some(armature)
                                || r["parent_bone"].as_str() != Some(bone)
                                || r["parent_type"] != "BONE"
                        }) {
                            return false;
                        }
                    }
                    Relation::Follow {
                        subject, target, ..
                    }
                    | Relation::LookAt { subject, target } => {
                        let kind = if matches!(relation, Relation::Follow { .. }) {
                            "COPY_LOCATION"
                        } else {
                            "TRACK_TO"
                        };
                        if row(subject)
                            .and_then(|r| r["constraints"].as_array())
                            .is_none_or(|rows| {
                                !rows.iter().any(|r| {
                                    r["type"] == kind && r["target"].as_str() == Some(target)
                                })
                            })
                        {
                            return false;
                        }
                    }
                    Relation::Skin { mesh, armature, .. } => {
                        if row(mesh)
                            .and_then(|r| r["modifiers"].as_array())
                            .is_none_or(|rows| {
                                !rows.iter().any(|r| {
                                    r["type"] == "ARMATURE"
                                        && r["target"].as_str() == Some(armature)
                                })
                            })
                        {
                            return false;
                        }
                    }
                }
            }
            if let Some(animation) = &spec.animation {
                for channel in &animation.channels {
                    let Some(entity_row) = row(&channel.entity) else {
                        return false;
                    };
                    let Some(actions) = entity_row["actions"].as_array() else {
                        return false;
                    };
                    if actions.len() != 1 {
                        return false;
                    }
                    let Some(curves) = actions[0]["curves"].as_array() else {
                        return false;
                    };
                    let property = match channel.property {
                        AnimatedProperty::Translation => "location",
                        AnimatedProperty::Rotation => "rotation_euler",
                        AnimatedProperty::Scale => "scale",
                    };
                    let path = channel
                        .bone
                        .as_ref()
                        .map(|bone| format!("pose.bones[\"{bone}\"].{property}"))
                        .unwrap_or_else(|| property.into());
                    for component in 0..3 {
                        let Some(curve) = curves.iter().find(|c| {
                            c["path"] == path && c["index"].as_u64() == Some(component as u64)
                        }) else {
                            return false;
                        };
                        let Some(keys) = curve["keys"].as_array() else {
                            return false;
                        };
                        if keys.len() != channel.keys.len() {
                            return false;
                        }
                        for (actual, expected) in keys.iter().zip(&channel.keys) {
                            let scale = if channel.property == AnimatedProperty::Translation {
                                spec.meters_per_unit
                            } else {
                                1.0
                            };
                            if actual[0].as_f64() != Some(expected.frame as f64)
                                || actual[2] != "LINEAR"
                                || !actual[1].as_f64().is_some_and(|x| {
                                    (x - expected.value[component] * scale).abs() < 1e-5
                                })
                            {
                                return false;
                            }
                        }
                    }
                }
                for entity in &spec.entities {
                    let expected_tracks = animation
                        .nla_tracks
                        .iter()
                        .filter(|track| track.entity == entity.id)
                        .collect::<Vec<_>>();
                    let Some(entity_row) = row(&entity.id) else {
                        return false;
                    };
                    let Some(actual_tracks) = entity_row["nla_tracks"].as_array() else {
                        return false;
                    };
                    if actual_tracks.len() != expected_tracks.len() {
                        return false;
                    }
                    for expected_track in expected_tracks {
                        let expected_name = format!("SW_NLA_{}", expected_track.id);
                        let Some(actual_track) = actual_tracks
                            .iter()
                            .find(|track| track["name"].as_str() == Some(expected_name.as_str()))
                        else {
                            return false;
                        };
                        let Some(actual_strips) = actual_track["strips"].as_array() else {
                            return false;
                        };
                        if actual_strips.len() != expected_track.strips.len() {
                            return false;
                        }
                        for expected_strip in &expected_track.strips {
                            let expected_strip_name = format!("SW_NLA_{}", expected_strip.id);
                            let Some(actual_strip) = actual_strips.iter().find(|strip| {
                                strip["name"].as_str() == Some(expected_strip_name.as_str())
                            }) else {
                                return false;
                            };
                            let expected_end = expected_strip.start_frame as f64
                                + (expected_strip.action_frame_end
                                    - expected_strip.action_frame_start)
                                    as f64
                                    * expected_strip.repeat
                                    * expected_strip.scale;
                            for (field, expected) in [
                                ("frame_start", expected_strip.start_frame as f64),
                                ("frame_end", expected_end),
                                (
                                    "action_frame_start",
                                    expected_strip.action_frame_start as f64,
                                ),
                                ("action_frame_end", expected_strip.action_frame_end as f64),
                                ("repeat", expected_strip.repeat),
                                ("scale", expected_strip.scale),
                                ("influence", expected_strip.influence),
                            ] {
                                if !actual_strip[field].as_f64().is_some_and(|value| {
                                    (value - expected).abs() <= 1e-5 * expected.abs().max(1.0)
                                }) {
                                    return false;
                                }
                            }
                        }
                    }
                }
            }
            true
        }
    }
}
