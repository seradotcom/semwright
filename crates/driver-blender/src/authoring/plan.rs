use super::*;
use schemars::JsonSchema;
use semwright_semantic_composition::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthoringIntent {
    Create { spec: BlenderAuthoringSpec },
    /// Incremental edit, not a destructive regeneration of the model/rig/materials.
    Transform { island: String, entity: String, transform: Transform, meters_per_unit: f64, expected_fingerprint: Digest },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeOperation {
    Collection { island: String, name: String },
    Material { island: String, material: Material },
    Entity { island: String, entity: Entity, meters_per_unit: f64 },
    Relation { island: String, relation: Relation },
    Animation { island: String, animation: Animation, meters_per_unit: f64 },
    Transform { island: String, entity: String, transform: Transform, meters_per_unit: f64 },
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
        ensure(self.source_only && self.exhaustive && self.total == self.items.len(), "planning requires complete source-only native snapshot")?;
        ensure(self.items.len() <= 512, "native snapshot item bound")?;
        bounded_id(&self.native_session)?;
        Ok(BaseStateSet(vec![BaseState {
            key: ResourceKey { provider: "driver:blender".into(), resource: self.island.clone().unwrap_or_else(|| "private-scene".into()) },
            document_id: self.island.clone().unwrap_or_else(|| self.native_session.clone()),
            provider_session: owner.session.clone(),
            generation: self.native_session.clone(),
            revision: Revision::Fingerprint(self.fingerprint.clone()),
            concurrency: Concurrency::BestEffortRevalidate,
        }]))
    }
}
pub fn profile(bindings: Vec<CapabilityBinding>) -> Result<ProfileDescriptor> {
    let p = ProfileDescriptor {
        identity: ProfileIdentity { id: "blender-native-authoring".into(), version: 1, intent_schema: schema_digest::<AuthoringIntent>()?, operation_schema: schema_digest::<NativeOperation>()? },
        capabilities: bindings,
        required_rules: ["blender.native-readback.v1".into(), "blender.effect-conformance.v1".into()].into(),
        allowed_effects: [EffectClass::Inspect,EffectClass::CreateOwnedObject,EffectClass::UpdateOwnedObject].into(),
    };
    p.validate()?; Ok(p)
}
pub fn prepare(
    owner: Owner,
    intent: AuthoringIntent,
    snapshot: &NativeSnapshot,
    new_island: String,
    descriptor: &ProfileDescriptor,
) -> Result<PreparedPlan<AuthoringIntent,NativeOperation>> {
    let base = snapshot.base(&owner)?;
    let mut payloads = Vec::new();
    match &intent {
        AuthoringIntent::Create{spec} => {
            spec.validate()?; local_id(&new_island)?;
            ensure(snapshot.island.is_none(), "new collection plan requires whole source scene observation")?;
            payloads.push(("collection".into(),NativeOperation::Collection{island:new_island.clone(),name:spec.collection.clone()}));
            for material in &spec.materials {
                payloads.push((format!("material-{}",material.id),NativeOperation::Material{island:new_island.clone(),material:material.clone()}));
            }
            let graph: BTreeMap<_,_> = spec.entities.iter().map(|e| (e.id.clone(),match &e.shape { Shape::MeshInstance{source}=>vec![source.clone()],_=>vec![] })).collect();
            for id in dag_order(&graph)? {
                let entity = spec.entities.iter().find(|e| e.id == id).expect("validated entity").clone();
                payloads.push((format!("entity-{id}"),NativeOperation::Entity{island:new_island.clone(),entity,meters_per_unit:spec.meters_per_unit}));
            }
            for (i,relation) in spec.relations.iter().enumerate() {
                payloads.push((format!("relation-{i}"),NativeOperation::Relation{island:new_island.clone(),relation:relation.clone()}));
            }
            if let Some(animation) = &spec.animation {
                payloads.push(("animation".into(),NativeOperation::Animation{island:new_island.clone(),animation:animation.clone(),meters_per_unit:spec.meters_per_unit}));
            }
        }
        AuthoringIntent::Transform{island,entity,transform,meters_per_unit,expected_fingerprint} => {
            local_id(island)?; local_id(entity)?; transform.validate()?; finite(*meters_per_unit,0.0001,100.0)?;
            ensure(snapshot.island.as_deref() == Some(island) && !snapshot.drift, "managed collection identity or manual-edit drift")?;
            ensure(&snapshot.fingerprint == expected_fingerprint, "external edit invalidates transform plan")?;
            ensure(snapshot.items.iter().filter(|row| row.get("entity").and_then(Value::as_str) == Some(entity)).count() == 1, "entity identity ambiguous or absent")?;
            payloads.push((format!("transform-{entity}"),NativeOperation::Transform{island:island.clone(),entity:entity.clone(),transform:transform.clone(),meters_per_unit:*meters_per_unit}));
        }
    }
    let resource = base.0[0].key.clone();
    let mut operations = Vec::new();
    let mut previous = None;
    for (id,payload) in payloads {
        let effect = if matches!(payload,NativeOperation::Transform{..}|NativeOperation::Relation{..}|NativeOperation::Animation{..}) { EffectClass::UpdateOwnedObject } else { EffectClass::CreateOwnedObject };
        let address = Address{resource:resource.clone(),logical_id:id.clone(),property:"declared-native-state".into()};
        operations.push(TypedOperation {
            id:id.clone(),payload,reads:vec![address.clone()],writes:vec![address],
            effects:[effect].into(),depends_on:previous.into_iter().collect(),
            postconditions:["blender.native-readback.v1".into()].into(),
        });
        previous = Some(id);
    }
    let budget = ConvergenceBudget { max_iterations:1,max_operations:MAX_OPERATIONS as u32,max_findings:512,max_observations:64,max_elapsed_ms:300_000 };
    PreparedPlan::prepare(PlanBody {
        contract_version: CONTRACT_VERSION,profile:descriptor.identity.clone(),owner,base,
        intent_digest:canonical_digest(&intent)?,intent,dependencies:BTreeMap::new(),
        observation_scope:operations.iter().flat_map(|o| o.writes.clone()).collect(),
        changes:ChangeSet{operations,atomicity:Atomicity::NonAtomicSequence},
        required_rules:descriptor.required_rules.clone(),budget,require_compare_and_swap:false,
    },descriptor)
}

/// This is a native structural receipt, not C's Project Graph receipt or F's evaluator.
/// Missing owner contracts remain required UNKNOWN checks rather than invented success.
pub fn verification(
    plan: &PreparedPlan<AuthoringIntent,NativeOperation>,
    after: &NativeSnapshot,
    native_match: bool,
    status: ExecutionStatus,
) -> Result<VerificationReport> {
    let base = after.base(&plan.body.owner)?;
    let scope = plan.body.observation_scope.iter().map(|address| Address { resource:base.0[0].key.clone(),..address.clone() }).collect();
    let evidence = ObservationRef {
        id:after.fingerprint.as_str().into(),base:base.clone(),source:EvidenceSource::NativeApi,
        method:"blender-bounded-source-rna".into(),method_version:1,scope,artifact:None,exhaustive:after.exhaustive,
    };
    let checks = vec![
        RuleResult{rule:"blender.native-readback.v1".into(),version:1,verdict:if native_match {Verdict::Pass} else {Verdict::Fail},evidence_class:EvidenceClass::Deterministic,evidence:vec![evidence],reason:None},
        RuleResult{rule:"blender.effect-conformance.v1".into(),version:1,verdict:Verdict::Unknown,evidence_class:EvidenceClass::Deterministic,evidence:vec![],reason:Some("F-owned E0 evaluator and C-owned P0 receipt not consumed in this patch; structural readback alone is not effect conformance".into())},
    ];
    Ok(VerificationReport {
        execution_status:status,
        validation:ValidationReport{plan_digest:plan.digest.clone(),base,required_rules:plan.body.required_rules.clone(),checks},
        support_level:SupportLevel::Native,effects_observed:vec![],effects_unobservable:plan.body.observation_scope.clone(),
    })
}
pub fn phases() -> BTreeSet<Phase> { [Phase::Inspect,Phase::Plan,Phase::Apply,Phase::Measure,Phase::Validate,Phase::Verify].into() }

fn near(actual: &Value, expected: &[f64]) -> bool {
    actual.as_array().is_some_and(|a| a.len()==expected.len() && a.iter().zip(expected).all(|(x,y)| x.as_f64().is_some_and(|x| (x-y).abs() <= 1e-5 * y.abs().max(1.0))))
}
fn native_transform(row: &Value, expected: &Transform, units: f64) -> bool {
    near(&row["translation"],&expected.translation.map(|x| x*units)) && near(&row["rotation"],&expected.rotation) && near(&row["scale"],&expected.scale)
}
/// Validate specifically enumerated source-RNA fields, not global artistry/effect correctness.
/// Material links, frame-domain coverage, modifier fidelity and persistence remain separate gates.
pub fn native_matches(intent: &AuthoringIntent, snapshot: &NativeSnapshot) -> bool {
    if !snapshot.exhaustive || snapshot.total != snapshot.items.len() { return false; }
    let row = |id: &str| snapshot.items.iter().find(|r| r["entity"].as_str() == Some(id));
    match intent {
        AuthoringIntent::Transform{entity,transform,meters_per_unit,..} => row(entity).is_some_and(|r| native_transform(r,transform,*meters_per_unit)),
        AuthoringIntent::Create{spec} => {
            if snapshot.total != spec.entities.len() { return false; }
            for entity in &spec.entities {
                let Some(r) = row(&entity.id) else { return false; };
                if !native_transform(r,&entity.transform,spec.meters_per_unit) { return false; }
                let expected_type = match &entity.shape { Shape::Empty=>"EMPTY",Shape::Armature{..}=>"ARMATURE",Shape::Camera{..}=>"CAMERA",Shape::AreaLight{..}=>"LIGHT",_=>"MESH" };
                if r["type"].as_str() != Some(expected_type) { return false; }
                if matches!(entity.shape,Shape::Box{..}|Shape::Cylinder{..}|Shape::Mesh{..}) && r["vertices"].as_u64() != Some(entity.shape.vertex_budget() as u64) { return false; }
                if let Shape::Armature{bones} = &entity.shape {
                    let Some(actual) = r["bones"].as_array() else { return false; };
                    if actual.len()!=bones.len() { return false; }
                    for bone in bones {
                        let Some(b) = actual.iter().find(|b| b["id"].as_str()==Some(&bone.id)) else { return false; };
                        if b["parent"].as_str()!=bone.parent.as_deref() || !near(&b["head"],&bone.head.map(|v| v*spec.meters_per_unit)) || !near(&b["tail"],&bone.tail.map(|v| v*spec.meters_per_unit)) { return false; }
                    }
                }
                if !entity.materials.is_empty() {
                    let Some(actual) = r["materials"].as_array() else { return false; };
                    if actual.len()!=entity.materials.len() { return false; }
                    for (id,m) in entity.materials.iter().zip(actual) {
                        let Some(expected)=spec.materials.iter().find(|v| &v.id==id) else { return false; };
                        if m["id"].as_str()!=Some(id) || !near(&m["color"],&expected.base_color)
                            || !m["roughness"].as_f64().is_some_and(|x| (x-expected.roughness).abs()<1e-5)
                            || !m["metallic"].as_f64().is_some_and(|x| (x-expected.metallic).abs()<1e-5) { return false; }
                    }
                }
                let skin_count=spec.relations.iter().filter(|r| matches!(r,Relation::Skin{mesh,..} if mesh==&entity.id)).count();
                if r["modifiers"].as_array().map(Vec::len).unwrap_or(0) != entity.modifiers.len()+skin_count { return false; }
            }
            for relation in &spec.relations {
                match relation {
                    Relation::Parent{child,parent} => { if row(child).is_none_or(|r| r["parent"].as_str()!=Some(parent)) { return false; } }
                    Relation::BoneParent{child,armature,bone} => { if row(child).is_none_or(|r| r["parent"].as_str()!=Some(armature) || r["parent_bone"].as_str()!=Some(bone) || r["parent_type"]!="BONE") { return false; } }
                    Relation::Follow{subject,target,..}|Relation::LookAt{subject,target} => {
                        let kind=if matches!(relation,Relation::Follow{..}) { "COPY_LOCATION" } else { "TRACK_TO" };
                        if row(subject).and_then(|r| r["constraints"].as_array()).is_none_or(|rows| !rows.iter().any(|r| r["type"]==kind && r["target"].as_str()==Some(target))) { return false; }
                    }
                    Relation::Skin{mesh,armature,..} => {
                        if row(mesh).and_then(|r| r["modifiers"].as_array()).is_none_or(|rows| !rows.iter().any(|r| r["type"]=="ARMATURE" && r["target"].as_str()==Some(armature))) { return false; }
                    }
                }
            }
            if let Some(animation)=&spec.animation {
                for channel in &animation.channels {
                    let Some(curves)=row(&channel.entity).and_then(|r| r["action"]["curves"].as_array()) else { return false; };
                    let property=match channel.property {AnimatedProperty::Translation=>"location",AnimatedProperty::Rotation=>"rotation_euler",AnimatedProperty::Scale=>"scale"};
                    let path=channel.bone.as_ref().map(|bone| format!("pose.bones[\"{bone}\"].{property}")).unwrap_or_else(|| property.into());
                    for component in 0..3 {
                        let Some(curve)=curves.iter().find(|c| c["path"]==path && c["index"].as_u64()==Some(component as u64)) else { return false; };
                        let Some(keys)=curve["keys"].as_array() else { return false; };
                        if keys.len()!=channel.keys.len() { return false; }
                        for (actual,expected) in keys.iter().zip(&channel.keys) {
                            let scale=if channel.property==AnimatedProperty::Translation {spec.meters_per_unit} else {1.0};
                            if actual[0].as_f64()!=Some(expected.frame as f64) || actual[2]!="LINEAR" || !actual[1].as_f64().is_some_and(|x| (x-expected.value[component]*scale).abs()<1e-5) { return false; }
                        }
                    }
                }
            }
            true
        }
    }
}
