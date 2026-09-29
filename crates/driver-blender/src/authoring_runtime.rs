//! Context-bound orchestration using A's PlanVault and F's effect evaluator.
//! This module is product code inside the first-party driver; it is not another authority service.
use super::{
    Capability, Child, Path, Value, capability, descriptor, descriptor_digest, json, request,
};
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
            serde_json::to_value(schemars::schema_for!(PlanInput)).expect("schema serializes"),
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
            json!({"type":"object","properties":{"island":id.clone(),"evaluated":{"type":"boolean"}},"required":["island","evaluated"],"additionalProperties":false}),
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
                "Bounded Blender native authoring; requires trusted protocol-v2 execution context",
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
async fn native(socket: &Path, suffix: &str, args: Value) -> Result<Value> {
    request(
        socket,
        &format!("driver.blender._authoring.{suffix}"),
        args,
        62,
    )
    .await
}
async fn snapshot(socket: &Path, island: Option<&str>) -> Result<NativeSnapshot> {
    Ok(serde_json::from_value(
        native(socket, "snapshot", json!({"island":island})).await?,
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
                let Some(action) = row.get("action").filter(|value| !value.is_null()) else {
                    continue;
                };
                let action_name = action.get("name").and_then(Value::as_str).ok_or_else(|| {
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
                let curves = action
                    .get("curves")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        Error::new(ErrorCode::PluginProtocolError, "action lacks curves")
                    })?;
                for curve in curves {
                    let path = curve.get("path").and_then(Value::as_str).ok_or_else(|| {
                        Error::new(ErrorCode::PluginProtocolError, "curve lacks path")
                    })?;
                    let index = curve.get("index").and_then(Value::as_u64).ok_or_else(|| {
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
                    let keys = curve.get("keys").and_then(Value::as_array).ok_or_else(|| {
                        Error::new(ErrorCode::PluginProtocolError, "curve lacks keys")
                    })?;
                    for (ordinal, key) in keys.iter().enumerate() {
                        let key_array = key.as_array().ok_or_else(|| {
                            Error::new(ErrorCode::PluginProtocolError, "keyframe row is malformed")
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
    socket: &Path,
    owner: &composition::Owner,
    input: PageInput,
) -> Result<Value> {
    semwright_driver_blender::authoring::local_id(&input.island).map_err(common_error)?;
    if !(1..=64).contains(&input.limit) {
        return Err(Error::invalid("page limit must be 1..64"));
    }
    let snapshot = snapshot(socket, Some(&input.island)).await?;
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
        AuthoringIntent::Transform { island, .. } => Some(island),
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
            | NativeOperation::Transform { island, .. } => island.as_str(),
        })
        .ok_or_else(|| Error::invalid("empty authoring plan"))
}
#[allow(clippy::too_many_arguments)]
pub(super) async fn execute(
    state: &mut State,
    child: &mut Child,
    socket: &Path,
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
            snapshot(socket, args["island"].as_str()).await?,
        )?),
        "inspect.page" => {
            let input: PageInput = serde_json::from_value(args)?;
            inspect_page(state, socket, &owner, input).await
        }
        "plan" => {
            if state.records.len() >= 32 {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "authoring plan cache is full; reconnect instead of evicting attempt history",
                ));
            }
            let input: PlanInput = serde_json::from_value(args)?;
            let before = snapshot(socket, intent_island(&input.intent)).await?;
            let prepared = prepare(
                owner.clone(),
                input.intent,
                &before,
                unique_id(),
                profile_bindings()?,
            )
            .map_err(common_error)?;
            let plan = prepared.plan.clone();
            let plan_ref = unique_id();
            state
                .vault
                .issue(
                    &owner,
                    &plan_ref,
                    &plan,
                    plan.body.budget.clone(),
                    plan.body.changes.operations.len() as u32,
                    None,
                    false,
                )
                .map_err(common_error)?;
            state.records.insert(
                (owner, plan_ref.clone()),
                Record {
                    prepared,
                    before,
                    after: None,
                    global_after: None,
                    report: None,
                    repair: false,
                },
            );
            Ok(serde_json::to_value(PlanOutput { plan_ref, plan })?)
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
                AuthoringIntent::Create { .. } => {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "repair v1 never regenerates a created asset; only an explicit prior transform is repairable",
                    ));
                }
            };
            let before = snapshot(socket, Some(&island)).await?;
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
            let plan_ref = unique_id();
            state
                .vault
                .issue(
                    &owner,
                    &plan_ref,
                    &plan,
                    plan.body.budget.clone(),
                    plan.body.changes.operations.len() as u32,
                    Some(&input.parent_plan_ref),
                    true,
                )
                .map_err(common_error)?;
            state.records.insert(
                (owner, plan_ref.clone()),
                Record {
                    prepared,
                    before,
                    after: None,
                    global_after: None,
                    report: None,
                    repair: true,
                },
            );
            Ok(serde_json::to_value(PlanOutput { plan_ref, plan })?)
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
            let before = snapshot(socket, intent_island(&plan.body.intent)).await?;
            plan.body
                .base
                .check_fresh(&before.base(&owner).map_err(common_error)?, false)
                .map_err(common_error)?;
            native(
                socket,
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
                    native(socket, "apply", json!({"operation":operation.payload})).await?;
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
                    native(socket, "finish", json!({"island":island})).await?,
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
                        snapshot(socket, None).await.ok()
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
                    let _ = child.start_kill();
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
        "measure" => native(socket, "measure", args).await,
        "persist" => native(socket, "persist", args).await,
        "reopen" => native(socket, "reopen", args).await,
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
            let after = snapshot(socket, Some(&island)).await?;
            let global_after = {
                let record = state.records.get(&key).expect("checked record");
                if matches!(
                    record.prepared.plan.body.intent,
                    AuthoringIntent::Create { .. }
                ) {
                    snapshot(socket, None).await.ok()
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
