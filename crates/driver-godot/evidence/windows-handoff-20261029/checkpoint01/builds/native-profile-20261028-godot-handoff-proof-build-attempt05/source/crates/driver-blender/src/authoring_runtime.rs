//! Context-bound orchestration using A's PlanVault and F's effect evaluator.
//! This module is product code inside the first-party driver; it is not another authority service.
use super::{Capability, NativeSession, Value, capability, descriptor, descriptor_digest, json};
use schemars::JsonSchema;
use semwright_driver_blender::authoring::*;
use semwright_driver_sdk::DriverExecutionContext;
use semwright_semantic_composition as composition;
use semwright_types::{Error, ErrorCode, Idempotency, JobProgress, Result, Risk, unique_id};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

type Plan = composition::PreparedPlan<AuthoringIntent, NativeOperation>;

struct Record {
    prepared: PreparedAuthoring,
    before: NativeSnapshot,
    after: Option<NativeSnapshot>,
    global_after: Option<NativeSnapshot>,
    report: Option<composition::VerificationReport>,
    repair: bool,
}
struct PageCursor {
    island: String,
    domain: String,
    fingerprint: composition::Digest,
    native_session: String,
    offset: usize,
}
pub(super) struct State {
    vault: composition::PlanVault,
    records: BTreeMap<(composition::Owner, String), Record>,
    cursors: BTreeMap<(composition::Owner, String), PageCursor>,
    poisoned: bool,
}
impl State {
    pub(super) fn poisoned(&self) -> bool {
        self.poisoned
    }
}
impl Default for State {
    fn default() -> Self {
        Self {
            vault: composition::PlanVault::bounded(32, 32, 128),
            records: BTreeMap::new(),
            cursors: BTreeMap::new(),
            poisoned: false,
        }
    }
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PlanInput {
    intent: AuthoringIntent,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PlanOutput {
    plan_ref: String,
    plan: Plan,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RepairPlanInput {
    parent_plan_ref: String,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MeasureSample {
    frame: i32,
    rate: semwright_media_time::Rate,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MeasureInput {
    island: String,
    evaluated: bool,
    #[serde(default)]
    pairs: Vec<[String; 2]>,
    #[serde(default)]
    sample: Option<MeasureSample>,
}
impl MeasureInput {
    fn validate(&self) -> composition::Result<()> {
        local_id(&self.island)?;
        composition::ensure(self.pairs.len() <= 64, "measurement pair budget")?;
        for pair in &self.pairs {
            local_id(&pair[0])?;
            local_id(&pair[1])?;
            composition::ensure(pair[0] != pair[1], "measurement pair requires distinct IDs")?;
        }
        if let Some(sample) = self.sample {
            composition::ensure(
                self.evaluated,
                "sampled frame measurement requires evaluated=true",
            )?;
            composition::ensure((0..=100_000).contains(&sample.frame), "sample frame bounds")?;
            sample.rate.validate()?;
            composition::ensure(
                sample.rate.num <= 240 && sample.rate.den <= 1001,
                "sample rate outside Blender authoring range",
            )?;
            sample.rate.at(i64::from(sample.frame))?;
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageInput {
    island: String,
    domain: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_page_limit")]
    limit: u32,
}
fn default_page_limit() -> u32 {
    32
}
fn common_error(error: composition::ContractError) -> Error {
    let code = match error {
        composition::ContractError::Denied(_) => ErrorCode::PolicyDenied,
        composition::ContractError::Stale(_) => ErrorCode::StaleReference,
        composition::ContractError::Unknown(_) => ErrorCode::Conflict,
        composition::ContractError::Limit(_) => ErrorCode::ResourceExhausted,
        composition::ContractError::Invalid(_) => ErrorCode::InvalidArgument,
    };
    Error::new(code, error.to_string())
}
pub(super) fn handles(command: &str) -> bool {
    command.starts_with("driver.blender.composition.")
}
pub(super) fn capabilities() -> Vec<Capability> {
    let id = json!({"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Za-z0-9_-]+$"});
    let reference = json!({
        "type":"object","properties":{"plan_ref":id.clone()},
        "required":["plan_ref"],"additionalProperties":false
    });
    let snapshot =
        serde_json::to_value(schemars::schema_for!(NativeSnapshot)).expect("schema serializes");
    let report = serde_json::to_value(schemars::schema_for!(composition::VerificationReport))
        .expect("schema serializes");
    // Do not publish the entire PreparedPlan generic as an external JSON Schema:
    // expanding its repeated intent/operation definitions exceeds Registry's bounded
    // external-schema walk. Rust already constructs and verifies the typed Plan;
    // the descriptor validates the stable transport envelope and digest shape.
    let plan_output = json!({
        "type":"object",
        "properties":{
            "plan_ref":id.clone(),
            "plan":{
                "type":"object",
                "properties":{
                    "body":{"type":"object"},
                    "digest":{"type":"string","pattern":"^[a-f0-9]{64}$"}
                },
                "required":["body","digest"],
                "additionalProperties":false
            }
        },
        "required":["plan_ref","plan"],
        "additionalProperties":false
    });
    let page_domain = json!({"type":"string","enum":[
        "objects","bones","actions","curves","keyframes","properties"
    ]});
    let page_output = json!({
        "type":"object",
        "properties":{
            "native_session":{"type":"string","minLength":1,"maxLength":256},
            "island":id.clone(),
            "domain":page_domain.clone(),
            "fingerprint":{"type":"string","pattern":"^[a-f0-9]{64}$"},
            "offset":{"type":"integer","minimum":0,"maximum":4096},
            "total":{"type":"integer","minimum":0,"maximum":4096},
            "items":{"type":"array","maxItems":64,"items":{"type":"object"}},
            "next_cursor":{"type":["string","null"],"maxLength":64},
            "final_page":{"type":"boolean"},
            "exhaustive":{"type":"boolean"}
        },
        "required":["native_session","island","domain","fingerprint","offset","total","items","next_cursor","final_page","exhaustive"],
        "additionalProperties":false
    });
    let rows = [
        (
            "inspect",
            json!({"type":"object","properties":{"island":id.clone()},"additionalProperties":false}),
            snapshot.clone(),
            Risk::ReadOnly,
        ),
        (
            "inspect.page",
            json!({"type":"object","properties":{
                "island":id.clone(),
                "domain":page_domain.clone(),
                "cursor":{"type":"string","minLength":1,"maxLength":64},
                "limit":{"type":"integer","minimum":1,"maximum":64,"default":32}
            },"required":["island","domain"],"additionalProperties":false}),
            page_output,
            Risk::ReadOnly,
        ),
        (
            "plan",
            plan_transport_input_schema(),
            plan_output.clone(),
            Risk::ReadOnly,
        ),
        (
            "apply",
            reference.clone(),
            json!({"type":"object","properties":{
                "plan_ref":id.clone(),"island":id.clone(),
                "report":{"type":"object"},"snapshot":{"type":"object"}
            },"required":["plan_ref","island","report","snapshot"],"additionalProperties":false}),
            Risk::Mutating,
        ),
        (
            "repair.plan",
            serde_json::to_value(schemars::schema_for!(RepairPlanInput))
                .expect("schema serializes"),
            plan_output.clone(),
            Risk::ReadOnly,
        ),
        (
            "repair.apply",
            reference.clone(),
            json!({"type":"object","properties":{
                "plan_ref":id.clone(),"island":id.clone(),
                "report":{"type":"object"},"snapshot":{"type":"object"}
            },"required":["plan_ref","island","report","snapshot"],"additionalProperties":false}),
            Risk::Mutating,
        ),
        (
            "measure",
            serde_json::to_value(schemars::schema_for!(MeasureInput))
                .expect("measure schema serializes"),
            json!({"type":"object"}),
            Risk::Mutating,
        ),
        (
            "validate",
            reference.clone(),
            report.clone(),
            Risk::ReadOnly,
        ),
        ("verify", reference, report, Risk::ReadOnly),
        (
            "persist",
            json!({"type":"object","properties":{"island":id.clone(),"path":{"type":"string","minLength":7,"maxLength":240}},"required":["island","path"],"additionalProperties":false}),
            json!({"type":"object"}),
            Risk::Mutating,
        ),
        (
            "reopen",
            json!({"type":"object","properties":{
                "island":id,"path":{"type":"string","minLength":7,"maxLength":240},
                "sha256":{"type":"string","pattern":"^[a-f0-9]{64}$"}
            },"required":["island","path","sha256"],"additionalProperties":false}),
            snapshot,
            Risk::Mutating,
        ),
    ];
    rows.into_iter()
        .map(|(phase, input, output, risk)| {
            let mut capability = descriptor(
                &format!("driver.blender.composition.{phase}"),
                "Bounded Blender native authoring; requires trusted protocol-v3 request context",
                input,
                output,
                &["composition"],
            );
            capability.descriptor.risk = risk;
            capability.descriptor.idempotency = if risk == Risk::ReadOnly {
                Idempotency::ReadOnly
            } else {
                Idempotency::NonIdempotent
            };
            capability.descriptor.dry_run = risk == Risk::ReadOnly;
            capability.descriptor.timeout_ms = 60_000;
            capability.tags = vec!["blender".into(), "composition".into(), "native".into()];
            if phase == "persist" {
                capability.tags.push("artifact-out:model/3d".into());
            }
            capability
        })
        .collect()
}
fn profile_bindings() -> Result<Vec<composition::CapabilityBinding>> {
    let mut bindings = Vec::new();
    for (phase, suffix) in [
        (composition::Phase::Inspect, "inspect"),
        (composition::Phase::Plan, "plan"),
        (composition::Phase::Apply, "apply"),
        (composition::Phase::Measure, "measure"),
        (composition::Phase::Validate, "validate"),
        (composition::Phase::RepairPlan, "repair.plan"),
        (composition::Phase::RepairApply, "repair.apply"),
        (composition::Phase::Verify, "verify"),
    ] {
        let capability = capability(&format!("driver.blender.composition.{suffix}"))?;
        bindings.push(composition::CapabilityBinding {
            phase,
            command: capability.descriptor.name.clone(),
            descriptor: composition::Digest::parse(descriptor_digest(&capability.descriptor)?)
                .map_err(common_error)?,
            effects: if phase == composition::Phase::Apply {
                [
                    composition::EffectClass::CreateOwnedObject,
                    composition::EffectClass::UpdateOwnedObject,
                ]
                .into()
            } else if phase == composition::Phase::RepairApply {
                [composition::EffectClass::UpdateOwnedObject].into()
            } else {
                [composition::EffectClass::Inspect].into()
            },
        });
    }
    Ok(bindings)
}
async fn native(channel: &NativeSession<'_>, suffix: &str, args: Value) -> Result<Value> {
    channel
        .request(&format!("driver.blender._authoring.{suffix}"), args, 62)
        .await
}
async fn snapshot(channel: &NativeSession<'_>, island: Option<&str>) -> Result<NativeSnapshot> {
    Ok(serde_json::from_value(
        native(channel, "snapshot", json!({"island":island})).await?,
    )?)
}

fn page_member(kind: &str, value: &Value) -> Result<String> {
    let digest = composition::canonical_digest(value).map_err(common_error)?;
    Ok(format!("{kind}-{}", digest.as_str()))
}

fn insert_page_item(
    values: &mut BTreeMap<String, Value>,
    member: String,
    mut value: Value,
) -> Result<()> {
    if let Some(object) = value.as_object_mut() {
        object.insert("member".into(), Value::String(member.clone()));
    } else {
        return Err(Error::new(
            ErrorCode::Internal,
            "authoring page item must be an object",
        ));
    }
    if values.insert(member, value).is_some() {
        return Err(Error::new(
            ErrorCode::Conflict,
            "native page identity is ambiguous",
        ));
    }
    if values.len() > 4096 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "authoring page universe exceeds 4096 items",
        ));
    }
    Ok(())
}

fn flatten_page(snapshot: &NativeSnapshot, domain: &str) -> Result<Vec<Value>> {
    let mut values = BTreeMap::<String, Value>::new();
    for row in &snapshot.items {
        let entity = row.get("entity").and_then(Value::as_str).ok_or_else(|| {
            Error::new(ErrorCode::PluginProtocolError, "managed row lacks entity")
        })?;
        match domain {
            "objects" => {
                let value = json!({
                    "entity":entity,
                    "name":row.get("name").cloned().unwrap_or(Value::Null),
                    "type":row.get("type").cloned().unwrap_or(Value::Null),
                    "parent":row.get("parent").cloned().unwrap_or(Value::Null),
                    "native_island":row.get("native_island").cloned().unwrap_or(Value::Null)
                });
                insert_page_item(&mut values, format!("object-{entity}"), value)?;
            }
            "bones" => {
                if let Some(bones) = row.get("bones").and_then(Value::as_array) {
                    for bone in bones {
                        let id = bone.get("id").and_then(Value::as_str).ok_or_else(|| {
                            Error::new(ErrorCode::PluginProtocolError, "bone row lacks ID")
                        })?;
                        let value = json!({
                            "entity":entity,
                            "bone":id,
                            "head":bone.get("head").cloned().unwrap_or(Value::Null),
                            "tail":bone.get("tail").cloned().unwrap_or(Value::Null),
                            "parent":bone.get("parent").cloned().unwrap_or(Value::Null),
                            "deform":bone.get("deform").cloned().unwrap_or(Value::Null)
                        });
                        insert_page_item(&mut values, format!("bone-{entity}-{id}"), value)?;
                    }
                }
            }
            "actions" | "curves" | "keyframes" => {
                let actions = row
                    .get("actions")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        Error::new(ErrorCode::PluginProtocolError, "object lacks action list")
                    })?;
                for action in actions {
                    let action_name =
                        action.get("name").and_then(Value::as_str).ok_or_else(|| {
                            Error::new(ErrorCode::PluginProtocolError, "action lacks name")
                        })?;
                    if domain == "actions" {
                        insert_page_item(
                            &mut values,
                            page_member("action", &json!([entity, action_name]))?,
                            json!({
                                "entity":entity,
                                "action":action_name,
                                "slots":action.get("slots").cloned().unwrap_or(Value::Null)
                            }),
                        )?;
                        continue;
                    }
                    let curves =
                        action
                            .get("curves")
                            .and_then(Value::as_array)
                            .ok_or_else(|| {
                                Error::new(ErrorCode::PluginProtocolError, "action lacks curves")
                            })?;
                    for curve in curves {
                        let path = curve.get("path").and_then(Value::as_str).ok_or_else(|| {
                            Error::new(ErrorCode::PluginProtocolError, "curve lacks path")
                        })?;
                        let index =
                            curve.get("index").and_then(Value::as_u64).ok_or_else(|| {
                                Error::new(ErrorCode::PluginProtocolError, "curve lacks index")
                            })?;
                        let curve_identity = json!([entity, action_name, path, index]);
                        if domain == "curves" {
                            insert_page_item(
                                &mut values,
                                page_member("curve", &curve_identity)?,
                                json!({
                                    "entity":entity,
                                    "action":action_name,
                                    "path":path,
                                    "index":index,
                                    "key_count":curve.get("keys").and_then(Value::as_array).map_or(0, Vec::len)
                                }),
                            )?;
                            continue;
                        }
                        let keys =
                            curve.get("keys").and_then(Value::as_array).ok_or_else(|| {
                                Error::new(ErrorCode::PluginProtocolError, "curve lacks keys")
                            })?;
                        for (ordinal, key) in keys.iter().enumerate() {
                            let key_array = key.as_array().ok_or_else(|| {
                                Error::new(
                                    ErrorCode::PluginProtocolError,
                                    "keyframe row is malformed",
                                )
                            })?;
                            if key_array.len() != 3 {
                                return Err(Error::new(
                                    ErrorCode::PluginProtocolError,
                                    "keyframe row has wrong arity",
                                ));
                            }
                            insert_page_item(
                                &mut values,
                                page_member(
                                    "key",
                                    &json!([entity, action_name, path, index, ordinal]),
                                )?,
                                json!({
                                    "entity":entity,
                                    "action":action_name,
                                    "path":path,
                                    "index":index,
                                    "ordinal":ordinal,
                                    "frame":key_array[0],
                                    "value":key_array[1],
                                    "interpolation":key_array[2]
                                }),
                            )?;
                        }
                    }
                }
            }
            "properties" => {
                for property in [
                    "translation",
                    "rotation",
                    "scale",
                    "parent",
                    "parent_type",
                    "parent_bone",
                    "hidden_render",
                    "hidden_viewport",
                    "vertices",
                    "polygons",
                    "uv_layers",
                    "data_users",
                    "data_name",
                ] {
                    let Some(value) = row.get(property) else {
                        continue;
                    };
                    insert_page_item(
                        &mut values,
                        format!("property-{entity}-{property}"),
                        json!({"entity":entity,"property":property,"value":value}),
                    )?;
                }
            }
            _ => {
                return Err(Error::invalid(
                    "authoring page domain is outside the closed enumeration",
                ));
            }
        }
    }
    Ok(values.into_values().collect())
}

async fn inspect_page(
    state: &mut State,
    channel: &NativeSession<'_>,
    owner: &composition::Owner,
    input: PageInput,
) -> Result<Value> {
    semwright_driver_blender::authoring::local_id(&input.island).map_err(common_error)?;
    if !(1..=64).contains(&input.limit) {
        return Err(Error::invalid("page limit must be 1..64"));
    }
    let snapshot = snapshot(channel, Some(&input.island)).await?;
    let mut offset = 0usize;
    if let Some(cursor) = input.cursor {
        composition::bounded_id(&cursor).map_err(common_error)?;
        let bound = state
            .cursors
            .remove(&(owner.clone(), cursor))
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::StaleReference,
                    "page cursor is unknown, replayed or belongs to another session",
                )
            })?;
        if bound.island != input.island
            || bound.domain != input.domain
            || bound.fingerprint != snapshot.fingerprint
            || bound.native_session != snapshot.native_session
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "native state changed between enumeration pages",
            ));
        }
        offset = bound.offset;
    }
    let all = flatten_page(&snapshot, &input.domain)?;
    if offset > all.len() {
        return Err(Error::new(
            ErrorCode::StaleReference,
            "page offset is outside the current snapshot",
        ));
    }
    let end = offset.saturating_add(input.limit as usize).min(all.len());
    let items = all[offset..end].to_vec();
    let next_cursor = if end < all.len() {
        if state.cursors.len() >= 64 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "too many outstanding authoring page cursors",
            ));
        }
        let token = unique_id();
        state.cursors.insert(
            (owner.clone(), token.clone()),
            PageCursor {
                island: input.island.clone(),
                domain: input.domain.clone(),
                fingerprint: snapshot.fingerprint.clone(),
                native_session: snapshot.native_session.clone(),
                offset: end,
            },
        );
        Some(token)
    } else {
        None
    };
    Ok(json!({
        "native_session":snapshot.native_session,
        "island":input.island,
        "domain":input.domain,
        "fingerprint":snapshot.fingerprint,
        "offset":offset,
        "total":all.len(),
        "items":items,
        "next_cursor":next_cursor,
        "final_page":end == all.len(),
        "exhaustive":snapshot.exhaustive
    }))
}
fn intent_island(intent: &AuthoringIntent) -> Option<&str> {
    match intent {
        AuthoringIntent::Create { .. } => None,
        AuthoringIntent::Transform { island, .. }
        | AuthoringIntent::MaterialSlots { island, .. } => Some(island),
    }
}
fn plan_island(plan: &Plan) -> Result<&str> {
    plan.body
        .changes
        .operations
        .first()
        .map(|operation| match &operation.payload {
            NativeOperation::Collection { island, .. }
            | NativeOperation::Texture { island, .. }
            | NativeOperation::Material { island, .. }
            | NativeOperation::Entity { island, .. }
            | NativeOperation::Relation { island, .. }
            | NativeOperation::Animation { island, .. }
            | NativeOperation::Transform { island, .. }
            | NativeOperation::MaterialSlots { island, .. } => island.as_str(),
        })
        .ok_or_else(|| Error::invalid("empty authoring plan"))
}
/// Publish only the exact, already bounded output returned to the registry.
/// Preparation is pure; no vault or record state changes until all preflight gates pass.
#[allow(clippy::too_many_arguments)]
fn publish_plan(
    state: &mut State,
    owner: composition::Owner,
    plan_ref: String,
    prepared: PreparedAuthoring,
    before: NativeSnapshot,
    parent_plan_ref: Option<&str>,
    repair: bool,
    output_schema: &Value,
    check_cancelled: impl FnOnce() -> Result<()>,
) -> Result<Value> {
    let key = (owner.clone(), plan_ref.clone());
    if state.records.contains_key(&key) {
        return Err(Error::new(
            ErrorCode::Conflict,
            "authoring plan reference already exists",
        ));
    }
    let plan = &prepared.plan;
    let output = serde_json::to_value(PlanOutput {
        plan_ref: plan_ref.clone(),
        plan: plan.clone(),
    })?;
    semwright_types::json_budget::value_budget(&output)?;
    if !jsonschema::validator_for(output_schema)
        .map_err(|_| Error::new(ErrorCode::Internal, "invalid authoring output schema"))?
        .is_valid(&output)
    {
        return Err(Error::new(
            ErrorCode::Internal,
            "authoring output violates typed schema",
        ));
    }
    check_cancelled()?;
    // No await or fallible validation between issue and record insertion.
    state
        .vault
        .issue(
            &owner,
            &plan_ref,
            plan,
            plan.body.budget.clone(),
            plan.body.changes.operations.len() as u32,
            parent_plan_ref,
            repair,
        )
        .map_err(common_error)?;
    state.records.insert(
        key,
        Record {
            prepared,
            before,
            after: None,
            global_after: None,
            report: None,
            repair,
        },
    );
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute(
    state: &mut State,
    channel: &NativeSession<'_>,
    command: &str,
    digest: &str,
    args: Value,
    context: &DriverExecutionContext,
) -> Result<Value> {
    context.check_cancelled()?;
    if state.poisoned {
        return Err(Error::unavailable(
            "Native attempt has an uncertain outcome; reconnect a fresh isolated driver and reconcile",
        ));
    }
    let capability = capability(command)?;
    if descriptor_digest(&capability.descriptor)? != digest {
        return Err(Error::new(
            ErrorCode::StaleReference,
            "authoring descriptor changed",
        ));
    }
    if !jsonschema::validator_for(&capability.descriptor.input_schema)
        .map_err(|_| Error::invalid("invalid embedded schema"))?
        .is_valid(&args)
    {
        return Err(Error::invalid("authoring arguments violate typed schema"));
    }
    let owner = composition::Owner {
        session: context.session().into(),
        principal: composition::PrincipalBinding::HostSession,
    };
    owner.validate().map_err(common_error)?;
    let suffix = command
        .strip_prefix("driver.blender.composition.")
        .ok_or_else(|| Error::invalid("authoring namespace"))?;
    match suffix {
        "inspect" => Ok(serde_json::to_value(
            snapshot(channel, args["island"].as_str()).await?,
        )?),
        "inspect.page" => {
            let input: PageInput = serde_json::from_value(args)?;
            inspect_page(state, channel, &owner, input).await
        }
        "plan" => {
            if state.records.len() >= 32 {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "authoring plan cache is full; reconnect instead of evicting attempt history",
                ));
            }
            let input: PlanInput = serde_json::from_value(args)?;
            let before = snapshot(channel, intent_island(&input.intent)).await?;
            let prepared = prepare(
                owner.clone(),
                input.intent,
                &before,
                unique_id(),
                profile_bindings()?,
            )
            .map_err(common_error)?;
            publish_plan(
                state,
                owner,
                unique_id(),
                prepared,
                before,
                None,
                false,
                &capability.descriptor.output_schema,
                || context.check_cancelled(),
            )
        }
        "repair.plan" => {
            if state.records.len() >= 32 {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "authoring plan cache is full; reconnect instead of evicting attempt history",
                ));
            }
            let input: RepairPlanInput = serde_json::from_value(args)?;
            composition::bounded_id(&input.parent_plan_ref).map_err(common_error)?;
            let parent_key = (owner.clone(), input.parent_plan_ref.clone());
            let (parent_intent, parent_report) = {
                let parent = state.records.get(&parent_key).ok_or_else(|| {
                    Error::new(
                        ErrorCode::PolicyDenied,
                        "repair parent was not issued to this session",
                    )
                })?;
                (
                    parent.prepared.plan.body.intent.clone(),
                    parent.report.clone().ok_or_else(|| {
                        Error::new(
                            ErrorCode::Conflict,
                            "repair parent has no completed verified native attempt",
                        )
                    })?,
                )
            };
            let (island, entity, transform, meters_per_unit) = match parent_intent {
                AuthoringIntent::Transform {
                    island,
                    entity,
                    transform,
                    meters_per_unit,
                    ..
                } => (island, entity, transform, meters_per_unit),
                AuthoringIntent::Create { .. } | AuthoringIntent::MaterialSlots { .. } => {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "repair v1 never regenerates or rematerializes an asset; only an explicit prior transform is repairable",
                    ));
                }
            };
            let before = snapshot(channel, Some(&island)).await?;
            let prior_verdict = parent_report.verdict().map_err(common_error)?;
            if prior_verdict == composition::Verdict::Pass && !before.drift {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "repair refused because current native state has no observed drift or failed required check",
                ));
            }
            let repair_intent = AuthoringIntent::Transform {
                island,
                entity,
                transform,
                meters_per_unit,
                expected_fingerprint: before.fingerprint.clone(),
            };
            let prepared =
                prepare_repair(owner.clone(), repair_intent, &before, profile_bindings()?)
                    .map_err(common_error)?;
            let plan = prepared.plan.clone();
            let root_budget = state
                .vault
                .root_budget(&owner, &input.parent_plan_ref)
                .map_err(common_error)?;
            if plan.body.budget != root_budget {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "repair planner attempted to reset the root convergence budget",
                ));
            }
            publish_plan(
                state,
                owner,
                unique_id(),
                prepared,
                before,
                Some(&input.parent_plan_ref),
                true,
                &capability.descriptor.output_schema,
                || context.check_cancelled(),
            )
        }
        "apply" | "repair.apply" => {
            let plan_ref = args["plan_ref"]
                .as_str()
                .ok_or_else(|| Error::invalid("plan ref"))?;
            let key = (owner.clone(), plan_ref.to_owned());
            let (plan, record_is_repair) = {
                let record = state.records.get(&key).ok_or_else(|| {
                    Error::new(
                        ErrorCode::PolicyDenied,
                        "plan was not issued to this session",
                    )
                })?;
                (record.prepared.plan.clone(), record.repair)
            };
            let requested_repair = suffix == "repair.apply";
            if requested_repair != record_is_repair {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "root and repair plans must use their matching apply capability",
                ));
            }
            state
                .vault
                .matches(&owner, plan_ref, &plan)
                .map_err(common_error)?;
            let before = snapshot(channel, intent_island(&plan.body.intent)).await?;
            plan.body
                .base
                .check_fresh(&before.base(&owner).map_err(common_error)?, false)
                .map_err(common_error)?;
            native(
                channel,
                "begin",
                json!({
                    "island":intent_island(&plan.body.intent),
                    "fingerprint":before.fingerprint,
                    "allow_drift":requested_repair
                }),
            )
            .await?;
            let permit = state
                .vault
                .begin(&owner, plan_ref, &plan, context.request_id())
                .map_err(common_error)?;
            let token = context.cancellation();
            let total = plan.body.changes.operations.len() as u64;
            let attempt = async {
                context.report_progress(
                    JobProgress {
                        completed: 0,
                        total: Some(total),
                        message: Some("Applying bounded native authoring operations".into()),
                    },
                    vec![],
                )?;
                for (index, operation) in plan.body.changes.operations.iter().enumerate() {
                    context.check_cancelled()?;
                    native(channel, "apply", json!({"operation":operation.payload})).await?;
                    context.report_progress(
                        JobProgress {
                            completed: index as u64 + 1,
                            total: Some(total),
                            message: None,
                        },
                        vec![],
                    )?;
                }
                let island = plan_island(&plan)?;
                serde_json::from_value::<NativeSnapshot>(
                    native(channel, "finish", json!({"island":island})).await?,
                )
                .map_err(Into::into)
            };
            let result = tokio::select! {
                result = attempt => result,
                _ = token.cancelled() => Err(Error::new(
                    ErrorCode::Cancelled,
                    "authoring cancellation; native effects may be partial",
                )),
            };
            match result {
                Ok(after) => {
                    // The native operation completed. Verification is a separate observation,
                    // and a missing whole-scene preservation probe remains UNKNOWN, not rollback.
                    state
                        .vault
                        .finish(
                            permit,
                            composition::ExecutionStatus::Completed,
                            plan.body
                                .changes
                                .operations
                                .iter()
                                .map(|operation| operation.id.clone())
                                .collect(),
                        )
                        .map_err(common_error)?;
                    let global_after = if matches!(plan.body.intent, AuthoringIntent::Create { .. })
                    {
                        snapshot(channel, None).await.ok()
                    } else {
                        None
                    };
                    let evaluation = {
                        let record = state.records.get(&key).expect("record exists");
                        evaluate_native_effects(
                            &record.prepared,
                            context.request_id(),
                            &record.before,
                            &after,
                            global_after.as_ref(),
                            composition::ExecutionStatus::Completed,
                        )
                        .map_err(common_error)?
                    };
                    let report = evaluation.report;
                    state
                        .vault
                        .record_observation(&owner, plan_ref, report.validation.checks.len() as u32)
                        .map_err(common_error)?;
                    if let Some(record) = state.records.get_mut(&key) {
                        record.after = Some(after.clone());
                        record.global_after = global_after;
                        record.report = Some(report.clone());
                    }
                    Ok(json!({
                        "plan_ref":plan_ref,
                        "island":plan_island(&plan)?,
                        "snapshot":after,
                        "report":report
                    }))
                }
                Err(error) => {
                    // The failing operation may have changed native state before failing. Never
                    // refund, retry, claim rollback or leave it working in the background.
                    let _ = channel.close().await;
                    state.poisoned = true;
                    state
                        .vault
                        .finish(
                            permit,
                            composition::ExecutionStatus::Unknown,
                            vec!["partial-effects-require-reconciliation".into()],
                        )
                        .map_err(common_error)?;
                    Err(error)
                }
            }
        }
        "measure" => {
            let input: MeasureInput = serde_json::from_value(args)?;
            input.validate().map_err(common_error)?;
            let sample = input.sample;
            let mut output = native(channel, "measure", serde_json::to_value(&input)?).await?;
            if let Some(sample) = sample {
                let observed = output.get("sample_observed").ok_or_else(|| {
                    Error::new(
                        ErrorCode::PluginProtocolError,
                        "sampled native measurement omitted observed frame metadata",
                    )
                })?;
                if observed.get("frame").and_then(Value::as_i64) != Some(i64::from(sample.frame)) {
                    return Err(Error::new(
                        ErrorCode::PluginProtocolError,
                        "sampled native measurement returned a different frame",
                    ));
                }
                let exact_time = sample
                    .rate
                    .at(i64::from(sample.frame))
                    .map_err(common_error)?;
                output
                    .as_object_mut()
                    .ok_or_else(|| Error::new(ErrorCode::PluginProtocolError, "measure output"))?
                    .insert(
                        "sample_time".into(),
                        json!({
                            "frame": sample.frame,
                            "rate": sample.rate,
                            "time": exact_time,
                            "domain": "media-time"
                        }),
                    );
            }
            Ok(output)
        }
        "persist" => native(channel, "persist", args).await,
        "reopen" => native(channel, "reopen", args).await,
        "validate" | "verify" => {
            let plan_ref = args["plan_ref"]
                .as_str()
                .ok_or_else(|| Error::invalid("plan ref"))?;
            let key = (owner.clone(), plan_ref.to_owned());
            let island = {
                let record = state
                    .records
                    .get(&key)
                    .ok_or_else(|| Error::new(ErrorCode::PolicyDenied, "plan session mismatch"))?;
                plan_island(&record.prepared.plan)?.to_owned()
            };
            let after = snapshot(channel, Some(&island)).await?;
            let global_after = {
                let record = state.records.get(&key).expect("checked record");
                if matches!(
                    record.prepared.plan.body.intent,
                    AuthoringIntent::Create { .. }
                ) {
                    snapshot(channel, None).await.ok()
                } else {
                    None
                }
            };
            let evaluation = {
                let record = state.records.get(&key).expect("checked record");
                if record.after.is_none() {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "plan has no completed native attempt",
                    ));
                }
                evaluate_native_effects(
                    &record.prepared,
                    context.request_id(),
                    &record.before,
                    &after,
                    global_after.as_ref(),
                    composition::ExecutionStatus::Completed,
                )
                .map_err(common_error)?
            };
            state
                .vault
                .record_observation(
                    &owner,
                    plan_ref,
                    evaluation.report.validation.checks.len() as u32,
                )
                .map_err(common_error)?;
            Ok(serde_json::to_value(evaluation.report)?)
        }
        _ => Err(Error::new(
            ErrorCode::Unsupported,
            "authoring phase not implemented",
        )),
    }
}

#[cfg(test)]
mod plan_publication_tests {
    use super::*;
    use std::cell::Cell;

    const VALID: &str = include_str!("../tests/fixtures/authoring_plan_output_valid.json");
    const OVERSIZED: &str = include_str!("../tests/fixtures/authoring_plan_output_budget.json");

    fn owner() -> composition::Owner {
        composition::Owner {
            session: "publication-test-session".into(),
            principal: composition::PrincipalBinding::HostSession,
        }
    }
    fn snapshot() -> NativeSnapshot {
        NativeSnapshot {
            native_session: "native-publication-fixture".into(),
            island: None,
            fingerprint: composition::Digest::of_bytes(b"unchanged-native-before"),
            drift: false,
            total: 0,
            items: vec![],
            source_only: true,
            exhaustive: true,
        }
    }
    fn prepared(source: &str, island: &str) -> PreparedAuthoring {
        let spec: BlenderAuthoringSpec = serde_json::from_str(source).unwrap();
        prepare(
            owner(),
            AuthoringIntent::Create { spec },
            &snapshot(),
            island.into(),
            profile_bindings().unwrap(),
        )
        .unwrap()
    }
    fn output_schema() -> Value {
        capability("driver.blender.composition.plan")
            .unwrap()
            .descriptor
            .output_schema
    }
    fn publish(state: &mut State, name: &str, plan: PreparedAuthoring) -> Value {
        publish_plan(
            state,
            owner(),
            name.into(),
            plan,
            snapshot(),
            None,
            false,
            &output_schema(),
            || Ok(()),
        )
        .unwrap()
    }

    #[test]
    fn oversized_response_never_reaches_publication_and_preserves_prior_authority() {
        let mut state = State::default();
        let prior = publish(&mut state, "prior-plan", prepared(VALID, "prior_island"));
        let prior_plan = state.records[&(owner(), "prior-plan".into())]
            .prepared
            .plan
            .clone();
        state
            .vault
            .matches(&owner(), "prior-plan", &prior_plan)
            .unwrap();

        // This is the exact original 1062-vertex recipe rejected by the physical registry.
        // Its input fits. Its duplicated intent/operation response does not.
        let bad = prepared(OVERSIZED, "oversized_island");
        let input = serde_json::to_value(PlanInput {
            intent: bad.plan.body.intent.clone(),
        })
        .unwrap();
        semwright_types::json_budget::value_budget(&input).unwrap();
        let bad_plan = bad.plan.clone();
        let exact_output = serde_json::to_value(PlanOutput {
            plan_ref: "oversized-plan".into(),
            plan: bad_plan.clone(),
        })
        .unwrap();
        assert_eq!(
            semwright_types::json_budget::value_budget(&exact_output)
                .unwrap_err()
                .code,
            ErrorCode::ResourceExhausted
        );
        let publication_gate_calls = Cell::new(0);
        let error = publish_plan(
            &mut state,
            owner(),
            "oversized-plan".into(),
            bad,
            snapshot(),
            None,
            false,
            &output_schema(),
            || {
                publication_gate_calls.set(publication_gate_calls.get() + 1);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::ResourceExhausted);
        // This callback is immediately before issue; it must not be reached on budget failure.
        assert_eq!(publication_gate_calls.get(), 0);
        assert_eq!(state.records.len(), 1);
        assert!(
            !state
                .records
                .contains_key(&(owner(), "oversized-plan".into()))
        );
        assert!(
            state
                .vault
                .matches(&owner(), "oversized-plan", &bad_plan)
                .is_err()
        );
        state
            .vault
            .matches(&owner(), "prior-plan", &prior_plan)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&state.records[&(owner(), "prior-plan".into())].prepared.plan)
                .unwrap(),
            prior["plan"]
        );

        let next = publish(&mut state, "next-plan", prepared(VALID, "next_island"));
        assert_eq!(state.records.len(), 2);
        assert_eq!(next["plan_ref"], "next-plan");
        let next_plan = &state.records[&(owner(), "next-plan".into())].prepared.plan;
        state
            .vault
            .matches(&owner(), "next-plan", next_plan)
            .unwrap();
        semwright_types::json_budget::value_budget(&next).unwrap();
        assert!(
            jsonschema::validator_for(&output_schema())
                .unwrap()
                .is_valid(&next)
        );
    }

    #[test]
    fn cancellation_and_output_schema_failure_do_not_issue_plans() {
        for cancelled in [false, true] {
            let mut state = State::default();
            let candidate = prepared(VALID, "rejected_island");
            let plan = candidate.plan.clone();
            let schema = if cancelled {
                output_schema()
            } else {
                json!(false)
            };
            let gate = Cell::new(false);
            let result = publish_plan(
                &mut state,
                owner(),
                "rejected-plan".into(),
                candidate,
                snapshot(),
                None,
                false,
                &schema,
                || {
                    gate.set(true);
                    Err(Error::new(
                        ErrorCode::Cancelled,
                        "cancelled before publication",
                    ))
                },
            );
            assert_eq!(
                result.unwrap_err().code,
                if cancelled {
                    ErrorCode::Cancelled
                } else {
                    ErrorCode::Internal
                }
            );
            assert_eq!(gate.get(), cancelled);
            assert!(state.records.is_empty());
            assert!(
                state
                    .vault
                    .matches(&owner(), "rejected-plan", &plan)
                    .is_err()
            );
        }
    }

    #[test]
    fn duplicate_reference_cannot_overwrite_a_prior_plan() {
        let mut state = State::default();
        let prior = publish(&mut state, "same-plan", prepared(VALID, "original_island"));
        let called = Cell::new(false);
        let error = publish_plan(
            &mut state,
            owner(),
            "same-plan".into(),
            prepared(VALID, "replacement_island"),
            snapshot(),
            None,
            false,
            &output_schema(),
            || {
                called.set(true);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(!called.get());
        assert_eq!(state.records.len(), 1);
        let unchanged = &state.records[&(owner(), "same-plan".into())].prepared.plan;
        assert_eq!(serde_json::to_value(unchanged).unwrap(), prior["plan"]);
        state
            .vault
            .matches(&owner(), "same-plan", unchanged)
            .unwrap();
    }
}
