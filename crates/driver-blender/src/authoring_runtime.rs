//! Context-bound orchestration using A's PlanVault and F's effect evaluator.
//! This module is product code inside the first-party driver; it is not another authority service.
use super::{
    Capability, Child, Path, Value, capability, descriptor, descriptor_digest, json, request,
};
use schemars::JsonSchema;
use semwright_driver_blender::authoring::*;
use semwright_driver_sdk::DriverExecutionContext;
use semwright_semantic_composition as composition;
use semwright_types::{
    Error, ErrorCode, Idempotency, JobProgress, Result, Risk, unique_id,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

type Plan = composition::PreparedPlan<AuthoringIntent, NativeOperation>;

struct Record {
    prepared: PreparedAuthoring,
    before: NativeSnapshot,
    after: Option<NativeSnapshot>,
    global_after: Option<NativeSnapshot>,
    report: Option<composition::VerificationReport>,
}
pub(super) struct State {
    vault: composition::PlanVault,
    records: BTreeMap<(composition::Owner, String), Record>,
    poisoned: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            vault: composition::PlanVault::bounded(32, 32, 128),
            records: BTreeMap::new(),
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
    let rows = [
        (
            "inspect",
            json!({"type":"object","properties":{"island":id.clone()},"additionalProperties":false}),
            snapshot.clone(),
            Risk::ReadOnly,
        ),
        (
            "plan",
            serde_json::to_value(schemars::schema_for!(PlanInput)).expect("schema serializes"),
            serde_json::to_value(schemars::schema_for!(PlanOutput)).expect("schema serializes"),
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
            "measure",
            json!({"type":"object","properties":{"island":id.clone(),"evaluated":{"type":"boolean"}},"required":["island","evaluated"],"additionalProperties":false}),
            json!({"type":"object"}),
            Risk::Mutating,
        ),
        ("validate", reference.clone(), report.clone(), Risk::ReadOnly),
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
                capability
                    .tags
                    .push("artifact-out:model/3d".into());
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
        return Err(Error::invalid(
            "authoring arguments violate typed schema",
        ));
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
                },
            );
            Ok(serde_json::to_value(PlanOutput { plan_ref, plan })?)
        }
        "apply" => {
            let plan_ref = args["plan_ref"]
                .as_str()
                .ok_or_else(|| Error::invalid("plan ref"))?;
            let key = (owner.clone(), plan_ref.to_owned());
            let plan = state
                .records
                .get(&key)
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::PolicyDenied,
                        "plan was not issued to this session",
                    )
                })?
                .prepared
                .plan
                .clone();
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
                    "fingerprint":before.fingerprint
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
                    let global_after = if matches!(plan.body.intent, AuthoringIntent::Create { .. }) {
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
                        .record_observation(
                            &owner,
                            plan_ref,
                            report.validation.checks.len() as u32,
                        )
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
                let record = state.records.get(&key).ok_or_else(|| {
                    Error::new(ErrorCode::PolicyDenied, "plan session mismatch")
                })?;
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
