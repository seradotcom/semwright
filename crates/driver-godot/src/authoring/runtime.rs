//! Broker-routed Godot authoring orchestration over A/C/F contracts.
use super::{
    model::{GENERATOR_VERSION, GodotAuthoringSpec},
    native_observation::NativeVerifyResult,
    profile::*,
    store::{PreparedFiles, Snapshot, Store, WriteReceipt},
    validate,
};
use crate::config::AuthoringConfig;
use semwright_effect_conformance::{
    AdapterIdentity, AdapterObservation, Attribution, EFFECT_CONTRACT_VERSION, EffectContract,
    EffectEvaluation, EffectLimit, EffectRule, EvaluationContext, EvidenceAdapter, EvidenceBinding,
    Obligation, ObservationCoverage, ObservationMethod, ObservedValue, Predicate, ReadbackState,
    collect, evaluate,
};
use semwright_project_graph::{
    Coverage, DependencyClass, DerivationId, Determinant, Equivalence, ExecutionReceipt,
    Fingerprint, GraphError, OperationIdentity, ProjectionDigest, ReceiptAdapter, ReceiptId,
    RevisionCandidate, RevisionPin, SCHEMA_VERSION,
};
use semwright_semantic_composition::{
    Address, Atomicity, BaseState, BaseStateSet, ChangeSet, Concurrency, ContractError, Controller,
    ConvergenceBudget, Digest, EffectClass, EvidenceSource, ExecutionStatus, Owner, Phase,
    PlanBody, PlanVault, PreparedPlan, ProfileDescriptor, ResourceKey, Revision, State,
    SupportLevel, TypedOperation, Verdict, VerificationReport, canonical_digest,
};
use semwright_types::{Error, ErrorCode, Result, unique_id};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const MAX_PLANS: usize = 64;
const OPERATION_ID: &str = "publish_managed_project";
const NATIVE_READBACK_PROPERTY: &str = "native_readback";
const NATIVE_RUNTIME_PROPERTY: &str = "native_runtime";
const NATIVE_PERSISTENCE_PROPERTY: &str = "native_persistence";
const NATIVE_REVISION_METHOD: &str = "godot_native_project_observation";
const NATIVE_REVISION_METHOD_VERSION: u32 = 1;

struct StoredPlan {
    plan: PreparedPlan<GodotAuthoringSpec, GodotOperation>,
    prepared: PreparedFiles,
    effects: EffectContract,
    repair: bool,
    root_plan_id: String,
    applied: Option<WriteReceipt>,
    apply_request_id: Option<String>,
}

struct RootControl {
    controller: Controller,
    started: Instant,
}

#[derive(Debug, Clone)]
pub(crate) struct NativePlanContext {
    pub(crate) owner: Owner,
    pub(crate) project: String,
    pub(crate) project_id: semwright_project_graph::ProjectId,
    pub(crate) plan_digest: Digest,
    pub(crate) intent_digest: Digest,
}

pub struct AuthoringRuntime {
    store: Store,
    profile: ProfileDescriptor,
    vault: PlanVault,
    plans: BTreeMap<(Owner, String), StoredPlan>,
    controllers: BTreeMap<(Owner, String), RootControl>,
}

impl AuthoringRuntime {
    pub fn new(config: AuthoringConfig, profile: ProfileDescriptor) -> Result<Self> {
        profile.validate().map_err(composition_error)?;
        let required: BTreeSet<_> = [
            RULE_MANAGED_CURRENT.to_owned(),
            RULE_INTENT_MATCH.to_owned(),
        ]
        .into();
        if profile.required_rules != required {
            return Err(Error::new(
                ErrorCode::Internal,
                "Godot authoring profile required-rule mismatch",
            ));
        }
        Ok(Self {
            store: Store::new(config)?,
            profile,
            vault: PlanVault::bounded(MAX_PLANS, MAX_PLANS, 256),
            plans: BTreeMap::new(),
            controllers: BTreeMap::new(),
        })
    }

    pub fn execute(
        &mut self,
        command: &str,
        args: &Value,
        owner: Owner,
        request_id: &str,
        check_cancelled: impl Fn() -> Result<()>,
    ) -> Result<Value> {
        owner.validate().map_err(composition_error)?;
        let value = match command {
            "driver.godot.composition.inspect" => {
                let request: InspectRequest = decode(args)?;
                serde_json::to_value(self.inspect(&request.project)?)?
            }
            "driver.godot.composition.plan" => {
                let request: PlanRequest = decode(args)?;
                serde_json::to_value(self.plan(&owner, request.spec)?)?
            }
            "driver.godot.composition.apply" => {
                let request: PlanRefRequest = decode(args)?;
                serde_json::to_value(self.apply(
                    &owner,
                    &request.plan_id,
                    false,
                    request_id,
                    check_cancelled,
                )?)?
            }
            "driver.godot.composition.measure" => {
                let request: PlanRefRequest = decode(args)?;
                serde_json::to_value(self.measure(&owner, &request.plan_id)?)?
            }
            "driver.godot.composition.validate" => {
                let request: PlanRefRequest = decode(args)?;
                serde_json::to_value(self.validate(&owner, &request.plan_id)?)?
            }
            "driver.godot.composition.repair.plan" => {
                let request: RepairPlanRequest = decode(args)?;
                serde_json::to_value(self.repair_plan_with_request(&owner, request, request_id)?)?
            }
            "driver.godot.composition.repair.apply" => {
                let request: PlanRefRequest = decode(args)?;
                serde_json::to_value(self.apply(
                    &owner,
                    &request.plan_id,
                    true,
                    request_id,
                    check_cancelled,
                )?)?
            }
            "driver.godot.composition.verify" => {
                let request: PlanRefRequest = decode(args)?;
                serde_json::to_value(self.verify(&owner, &request.plan_id)?)?
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "Godot authoring route invariant violated",
                ));
            }
        };
        Ok(value)
    }

    pub(crate) fn evaluate_native(
        &mut self,
        owner: &Owner,
        plan_id: &str,
        request_id: &str,
        scene: &str,
        result: &NativeVerifyResult,
    ) -> Result<EffectEvaluation> {
        let (plan, effects, project) = {
            let stored = self.stored_plan(owner, plan_id)?;
            (
                stored.plan.clone(),
                stored.effects.clone(),
                stored.plan.body.intent.project.clone(),
            )
        };
        self.vault
            .matches(owner, plan_id, &plan)
            .map_err(composition_error)?;
        let snapshot = self.store.snapshot(&project)?;
        let record = snapshot.record().ok_or_else(|| {
            Error::new(
                ErrorCode::Conflict,
                "Native effect evaluation requires provider derivation state",
            )
        })?;
        let binding = result.binding();
        if binding.owner != *owner
            || binding.request_id != request_id
            || binding.project != record.project
            || binding.slug != project
            || binding.scene != scene
            || binding.plan_digest != plan.digest
            || binding.intent_digest != plan.body.intent_digest
            || binding.source_fingerprint != snapshot.fingerprint
        {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Native evidence binding differs from authenticated plan/state",
            ));
        }
        let current_base = base_state(owner, &project, &snapshot)?;
        let context = EvaluationContext {
            owner: owner.clone(),
            request_id: request_id.into(),
            plan_digest: plan.digest.clone(),
            contract_digest: effects.digest().map_err(composition_error)?,
            before: plan.body.base.clone(),
            after: current_base,
            operations: plan
                .body
                .changes
                .operations
                .iter()
                .map(|operation| operation.id.clone())
                .collect(),
            observation_scope: plan.body.observation_scope.iter().cloned().collect(),
            execution_status: ExecutionStatus::Completed,
            support_level: SupportLevel::Native,
            budget: plan.body.budget.clone(),
        };
        context
            .validate_plan(&plan, &self.profile, &effects)
            .map_err(composition_error)?;
        let store = StoreEvidenceAdapter {
            owner,
            project: &project,
            snapshot: &snapshot,
            post_base: &context.after,
        };
        let mut adapter = NativeEffectAdapter {
            store,
            scene,
            result,
        };
        let batch = collect(&effects, &context, &mut adapter).map_err(composition_error)?;
        let evaluation = evaluate(&effects, &context, &batch).map_err(composition_error)?;
        let findings = evaluation
            .coverage
            .iter()
            .filter(|coverage| coverage.required && !coverage.sufficient)
            .count() as u32;
        self.vault
            .record_observation(owner, plan_id, findings)
            .map_err(composition_error)?;
        Ok(evaluation)
    }

    /// Build C's untrusted read-only revision candidate from an already-bound
    /// native result. The durable binding generation is supplied by the trusted
    /// Project Graph host; D never derives or increments that authority locally.
    pub fn native_revision_candidate(
        &self,
        owner: &Owner,
        plan_id: &str,
        request_id: &str,
        scene: &str,
        binding_generation: u64,
        result: &NativeVerifyResult,
    ) -> Result<RevisionCandidate> {
        if binding_generation == 0 {
            return Err(Error::invalid(
                "Project Graph binding generation must be nonzero",
            ));
        }
        let context = self.native_plan_context(owner, plan_id, scene)?;
        let snapshot = self.store.snapshot(&context.project)?;
        let record = snapshot.record().ok_or_else(|| {
            Error::new(
                ErrorCode::Conflict,
                "Native revision admission requires provider derivation state",
            )
        })?;
        let binding = result.binding();
        if binding.owner != *owner
            || binding.request_id != request_id
            || binding.project != context.project_id
            || binding.slug != context.project
            || binding.scene != scene
            || binding.plan_digest != context.plan_digest
            || binding.intent_digest != context.intent_digest
            || binding.source_fingerprint != snapshot.fingerprint
        {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Native revision evidence differs from authenticated plan/state",
            ));
        }
        let native = match result {
            NativeVerifyResult::Inspect { observation, .. }
            | NativeVerifyResult::Play { observation, .. } => observation,
            NativeVerifyResult::Persistence { reader, .. } => reader,
        };
        if native.source_fingerprint != snapshot.fingerprint {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Native revision source fingerprint differs from managed state",
            ));
        }
        let asset = record
            .bindings
            .get(&format!("scene:{scene}"))
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::Conflict, "Managed scene identity is missing"))?;
        let projection = native.authored.stable_digest()?;
        let live_complete = native
            .live
            .as_ref()
            .is_none_or(|projection| projection.unknown.is_empty());
        let runtime_complete = !matches!(result, NativeVerifyResult::Play { .. })
            || (native.live.is_some()
                && !native.frames.is_empty()
                && native.frames.iter().all(|frame| frame.fault.is_none()));
        let complete = native.dependency_complete
            && native.authored.unknown.is_empty()
            && live_complete
            && native.failures.is_empty()
            && runtime_complete;
        let coverage = if complete {
            Coverage::complete()
        } else {
            Coverage::unknown()
        };
        let properties: &[&str] = match result {
            NativeVerifyResult::Inspect { .. } => &[NATIVE_READBACK_PROPERTY],
            NativeVerifyResult::Persistence { .. } => {
                &[NATIVE_READBACK_PROPERTY, NATIVE_PERSISTENCE_PROPERTY]
            }
            NativeVerifyResult::Play { .. } => &[NATIVE_READBACK_PROPERTY, NATIVE_RUNTIME_PROPERTY],
        };
        let resource = project_resource(&context.project);
        let scope = properties
            .iter()
            .map(|property| Address {
                resource: resource.clone(),
                logical_id: scene.to_owned(),
                property: (*property).to_owned(),
            })
            .collect();
        let candidate = RevisionCandidate {
            version: SCHEMA_VERSION,
            asset,
            fingerprint: Fingerprint {
                bytes: Some(native.loaded_scene_sha256.clone()),
                projection: Some(ProjectionDigest {
                    digest: projection,
                    method: NATIVE_REVISION_METHOD.into(),
                    method_version: NATIVE_REVISION_METHOD_VERSION,
                }),
            },
            equivalence: Equivalence::Projection,
            observed_unix_ms: now_unix_ms()?,
            binding_generation,
            observation: semwright_semantic_composition::ObservationRef {
                id: format!("godot_native_revision_{}", unique_id()),
                base: base_state(owner, &context.project, &snapshot)?,
                source: EvidenceSource::NativeApi,
                method: NATIVE_REVISION_METHOD.into(),
                method_version: NATIVE_REVISION_METHOD_VERSION,
                scope,
                artifact: None,
                exhaustive: complete,
            },
            coverage,
        };
        candidate.validate().map_err(graph_error)?;
        Ok(candidate)
    }

    pub fn inspect(&self, project: &str) -> Result<SnapshotView> {
        let snapshot = self.store.snapshot(project)?;
        Ok(snapshot_view(project, &snapshot))
    }

    pub(crate) fn native_plan_context(
        &self,
        owner: &Owner,
        plan_id: &str,
        scene: &str,
    ) -> Result<NativePlanContext> {
        validate::id(scene).map_err(|error| Error::invalid(error.to_string()))?;
        let stored = self.stored_plan(owner, plan_id)?;
        self.vault
            .matches(owner, plan_id, &stored.plan)
            .map_err(composition_error)?;
        if stored.applied.is_none() {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Native verification requires a completed authoring apply",
            ));
        }
        if !stored
            .plan
            .body
            .intent
            .scenes
            .iter()
            .any(|candidate| candidate.id == scene)
        {
            return Err(Error::new(
                ErrorCode::NotFound,
                "Native verification scene is not in the prepared Godot intent",
            ));
        }
        let snapshot = self.store.snapshot(&stored.plan.body.intent.project)?;
        if snapshot.status != "IN_SYNC" {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Native verification requires current managed sources",
            ));
        }
        let record = snapshot.record().ok_or_else(|| {
            Error::new(
                ErrorCode::Conflict,
                "Native verification requires a provider derivation record",
            )
        })?;
        if record.intent_digest != stored.plan.body.intent_digest {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Native verification intent differs from the prepared plan",
            ));
        }
        Ok(NativePlanContext {
            owner: owner.clone(),
            project: record.slug.clone(),
            project_id: record.project.clone(),
            plan_digest: stored.plan.digest.clone(),
            intent_digest: stored.plan.body.intent_digest.clone(),
        })
    }

    pub fn plan(&mut self, owner: &Owner, spec: GodotAuthoringSpec) -> Result<PlanResult> {
        self.ensure_capacity()?;
        let prepared = self.store.prepare(&spec, false, false)?;
        let budget = ConvergenceBudget {
            max_iterations: 4,
            max_operations: 8,
            max_findings: 64,
            max_observations: 64,
            max_elapsed_ms: 600_000,
        };
        self.issue(owner, prepared, spec, budget, None, false)
    }

    pub fn repair_plan(&mut self, owner: &Owner, request: RepairPlanRequest) -> Result<PlanResult> {
        self.repair_plan_with_request(owner, request, &format!("godot_reconcile_{}", unique_id()))
    }

    pub fn repair_plan_with_request(
        &mut self,
        owner: &Owner,
        request: RepairPlanRequest,
        request_id: &str,
    ) -> Result<PlanResult> {
        self.ensure_capacity()?;
        if matches!(request.mode, RepairMode::ReconcilePartialPublication) {
            return self.reconcile_partial_plan(owner, &request.parent_plan_id, request_id);
        }
        let parent_key = (owner.clone(), request.parent_plan_id.clone());
        let (spec, root_plan_id) = {
            let parent = self.plans.get(&parent_key).ok_or_else(|| {
                Error::new(
                    ErrorCode::PermissionDenied,
                    "Unknown Godot authoring parent plan",
                )
            })?;
            if parent.applied.is_none() {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Repair requires a completed parent application",
                ));
            }
            (parent.plan.body.intent.clone(), parent.root_plan_id.clone())
        };
        let control = self
            .controllers
            .get(&(owner.clone(), root_plan_id.clone()))
            .ok_or_else(|| Error::new(ErrorCode::Internal, "Missing authoring controller"))?;
        if control.controller.state != State::RepairPlanned {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Repair is only available after deterministic validation selected one candidate",
            ));
        }
        let snapshot = self.store.snapshot(&spec.project)?;
        if snapshot.files.iter().any(|file| file.state == "diverged") {
            return Err(Error::new(
                ErrorCode::Conflict,
                "DIVERGED: external edits require a new human/model decision",
            ));
        }
        if !snapshot.files.iter().any(|file| file.state == "missing") {
            return Err(Error::new(
                ErrorCode::Conflict,
                "No supported missing-managed-source repair candidate is present",
            ));
        }
        let prepared = self.store.prepare(&spec, true, false)?;
        let budget = self
            .vault
            .root_budget(owner, &request.parent_plan_id)
            .map_err(composition_error)?;
        self.issue(
            owner,
            prepared,
            spec,
            budget,
            Some(&request.parent_plan_id),
            true,
        )
    }

    /// Read-only reconciliation through A, followed by a new ordinary repair plan.
    /// The pending Store journal and actual managed bytes must still match the
    /// original target. This neither retries the parent nor resets its controller.
    fn reconcile_partial_plan(
        &mut self,
        owner: &Owner,
        parent_id: &str,
        request_id: &str,
    ) -> Result<PlanResult> {
        let (parent_plan, parent_target, root_plan_id) = {
            let parent = self.stored_plan(owner, parent_id)?;
            (
                parent.plan.clone(),
                parent.prepared.target.clone(),
                parent.root_plan_id.clone(),
            )
        };
        self.vault
            .matches(owner, parent_id, &parent_plan)
            .map_err(composition_error)?;
        let control_key = (owner.clone(), root_plan_id);
        let control = self
            .controllers
            .get(&control_key)
            .ok_or_else(|| Error::new(ErrorCode::Internal, "Missing reconciliation controller"))?;
        if !matches!(
            control.controller.state,
            State::PartiallyApplied | State::Unknown
        ) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Only an uncertain application can be reconciled",
            ));
        }
        let spec = parent_plan.body.intent.clone();
        // prepare is read-only, rejects human edits/collisions and retains the
        // pending target's project, bindings, file identities and revision.
        let prepared = self.store.prepare(&spec, false, true)?;
        if prepared.target.project != parent_target.project
            || prepared.target.intent_digest != parent_target.intent_digest
            || prepared.target.manifest_digest != parent_target.manifest_digest
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Pending target differs from the original authorized publication",
            ));
        }
        let expected_base = base_state(owner, &spec.project, &prepared.before)?;
        let scope = parent_plan
            .body
            .observation_scope
            .iter()
            .filter(|address| {
                address.property == "managed_files" || address.property == "intent_digest"
            })
            .cloned()
            .collect::<Vec<_>>();
        let permit = self
            .vault
            .begin_reconciliation(
                owner,
                parent_id,
                &parent_plan,
                request_id,
                expected_base,
                scope.clone(),
            )
            .map_err(composition_error)?;
        // Observe again after A's observation reservation. A rejects a changed
        // fingerprint/provider binding; apply revalidates the same state later.
        let fresh = self.store.snapshot(&spec.project)?;
        let observed = observation(owner, &spec.project, &fresh, scope)?;
        self.vault
            .finish_reconciliation(permit, observed)
            .map_err(composition_error)?;
        self.controllers
            .get_mut(&control_key)
            .expect("controller checked")
            .controller
            .reconciled_for_repair(&parent_plan.digest)
            .map_err(composition_error)?;
        let budget = self
            .vault
            .root_budget(owner, parent_id)
            .map_err(composition_error)?;
        self.issue(owner, prepared, spec, budget, Some(parent_id), true)
    }

    fn issue(
        &mut self,
        owner: &Owner,
        prepared: PreparedFiles,
        spec: GodotAuthoringSpec,
        budget: ConvergenceBudget,
        parent: Option<&str>,
        repair: bool,
    ) -> Result<PlanResult> {
        let base = base_state(owner, &spec.project, &prepared.before)?;
        let intent_digest = canonical_digest(&spec).map_err(composition_error)?;
        let resource = project_resource(&spec.project);
        let writes: Vec<Address> = prepared
            .writes
            .iter()
            .map(|path| Address {
                resource: resource.clone(),
                logical_id: path.clone(),
                property: "bytes".into(),
            })
            .collect();
        let mut reads: Vec<Address> = prepared
            .before
            .files
            .iter()
            .map(|file| Address {
                resource: resource.clone(),
                logical_id: file.path.clone(),
                property: "bytes".into(),
            })
            .collect();
        reads.push(Address {
            resource: resource.clone(),
            logical_id: spec.project.clone(),
            property: "managed_state".into(),
        });
        let effects: BTreeSet<_> = if prepared.writes.is_empty() {
            [EffectClass::Inspect].into()
        } else if prepared.before.exists {
            [EffectClass::UpdateOwnedObject].into()
        } else {
            [EffectClass::CreateOwnedObject].into()
        };
        let operation = TypedOperation {
            id: OPERATION_ID.into(),
            payload: GodotOperation::PublishManagedProject {
                project: spec.project.clone(),
                target_revision: prepared.target.revision,
                base_fingerprint: prepared.before.fingerprint.clone(),
                manifest_digest: prepared.target.manifest_digest.clone(),
                writes: prepared.writes.clone(),
                repair,
            },
            reads,
            writes: writes.clone(),
            effects: effects.clone(),
            depends_on: vec![],
            postconditions: vec![RULE_MANAGED_CURRENT.into(), RULE_INTENT_MATCH.into()]
                .into_iter()
                .collect(),
        };
        let contract = effect_contract(&self.profile, &spec, &intent_digest, &operation)?;
        let required_rules = contract.required_rules();
        if required_rules != self.profile.required_rules {
            return Err(Error::new(
                ErrorCode::Internal,
                "Godot effect rules differ from trusted profile",
            ));
        }
        let effect_contract_digest = contract.digest().map_err(composition_error)?;
        let changes = ChangeSet {
            operations: vec![operation],
            atomicity: Atomicity::NonAtomicSequence,
        };
        let mut dependencies = BTreeMap::new();
        dependencies.insert(
            "generator".into(),
            Digest::of_bytes(GENERATOR_VERSION.as_bytes()),
        );
        dependencies.insert(
            "project_graph_contract".into(),
            Digest::of_bytes(format!("project-graph-v{SCHEMA_VERSION}").as_bytes()),
        );
        dependencies.insert("effects.contract".into(), effect_contract_digest.clone());
        let mut observation_scope = vec![
            Address {
                resource: resource.clone(),
                logical_id: spec.project.clone(),
                property: "managed_files".into(),
            },
            Address {
                resource: resource.clone(),
                logical_id: spec.project.clone(),
                property: "intent_digest".into(),
            },
        ];
        observation_scope.extend(native_addresses(&resource, &spec));
        let body = PlanBody {
            contract_version: semwright_semantic_composition::CONTRACT_VERSION,
            profile: self.profile.identity.clone(),
            owner: owner.clone(),
            base,
            intent: spec,
            intent_digest,
            dependencies,
            changes,
            required_rules,
            observation_scope,
            budget: budget.clone(),
            require_compare_and_swap: false,
        };
        let plan = PreparedPlan::prepare(body, &self.profile).map_err(composition_error)?;
        let plan_id = format!("godot_plan_{}", unique_id());
        let root_plan_id = if let Some(parent_id) = parent {
            let parent_plan = self
                .plans
                .get(&(owner.clone(), parent_id.to_owned()))
                .ok_or_else(|| Error::new(ErrorCode::PermissionDenied, "Unknown parent plan"))?;
            let root = parent_plan.root_plan_id.clone();
            let control = self
                .controllers
                .get(&(owner.clone(), root.clone()))
                .ok_or_else(|| Error::new(ErrorCode::Internal, "Missing repair controller"))?;
            if control.controller.state != State::RepairPlanned {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Repair controller is not ready",
                ));
            }
            self.vault
                .issue(
                    owner,
                    &plan_id,
                    &plan,
                    budget,
                    plan.body.changes.operations.len() as u32,
                    Some(parent_id),
                    true,
                )
                .map_err(composition_error)?;
            self.controllers
                .get_mut(&(owner.clone(), root.clone()))
                .expect("controller checked above")
                .controller
                .bind_repair(plan.digest.clone())
                .map_err(composition_error)?;
            root
        } else {
            self.vault
                .issue(
                    owner,
                    &plan_id,
                    &plan,
                    budget.clone(),
                    plan.body.changes.operations.len() as u32,
                    None,
                    false,
                )
                .map_err(composition_error)?;
            let controller = Controller::new(
                plan.digest.clone(),
                budget,
                self.profile.required_rules.clone(),
            )
            .map_err(composition_error)?;
            self.controllers.insert(
                (owner.clone(), plan_id.clone()),
                RootControl {
                    controller,
                    started: Instant::now(),
                },
            );
            plan_id.clone()
        };
        let result = PlanResult {
            plan_id: plan_id.clone(),
            root_plan_id: root_plan_id.clone(),
            plan_digest: plan.digest.clone(),
            effect_contract_digest,
            base: plan.body.base.clone(),
            intent_digest: plan.body.intent_digest.clone(),
            target_revision: prepared.target.revision,
            writes: prepared.writes.clone(),
            repair,
            no_op: prepared.writes.is_empty(),
        };
        self.plans.insert(
            (owner.clone(), plan_id),
            StoredPlan {
                plan,
                prepared,
                effects: contract,
                repair,
                root_plan_id,
                applied: None,
                apply_request_id: None,
            },
        );
        Ok(result)
    }

    pub fn apply(
        &mut self,
        owner: &Owner,
        plan_id: &str,
        repair: bool,
        request_id: &str,
        check_cancelled: impl Fn() -> Result<()>,
    ) -> Result<ApplyResult> {
        let key = (owner.clone(), plan_id.to_owned());
        let (plan, prepared, root_plan_id, plan_repair) = {
            let stored = self.plans.get(&key).ok_or_else(|| {
                Error::new(ErrorCode::PermissionDenied, "Unknown Godot authoring plan")
            })?;
            (
                stored.plan.clone(),
                stored.prepared.clone(),
                stored.root_plan_id.clone(),
                stored.repair,
            )
        };
        if plan_repair != repair {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Normal and repair authoring plans cannot cross apply routes",
            ));
        }
        let control_key = (owner.clone(), root_plan_id);
        let expected = if repair {
            State::RepairPlanned
        } else {
            State::Prepared
        };
        if self
            .controllers
            .get(&control_key)
            .map(|control| control.controller.state)
            != Some(expected)
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Authoring controller is not in an applicable state",
            ));
        }
        let permit = self
            .vault
            .begin(owner, plan_id, &plan, request_id)
            .map_err(composition_error)?;
        self.controllers
            .get_mut(&control_key)
            .expect("controller state checked")
            .controller
            .applying(repair)
            .map_err(composition_error)?;
        let result = self.store.apply(&prepared, check_cancelled);
        match result {
            Ok(write) => {
                let effects = write
                    .written
                    .iter()
                    .map(|path| {
                        format!(
                            "managed-write:{}",
                            Digest::of_bytes(path.as_bytes()).as_str()
                        )
                    })
                    .collect();
                if let Err(error) = self
                    .vault
                    .finish(permit, ExecutionStatus::Completed, effects)
                {
                    let _ = self
                        .controllers
                        .get_mut(&control_key)
                        .expect("controller exists")
                        .controller
                        .executed(ExecutionStatus::Unknown);
                    return Err(composition_error(error).uncertain());
                }
                if let Err(error) = self
                    .controllers
                    .get_mut(&control_key)
                    .expect("controller exists")
                    .controller
                    .executed(ExecutionStatus::Completed)
                {
                    return Err(composition_error(error).uncertain());
                }
                let controller_state = self
                    .controllers
                    .get(&control_key)
                    .expect("controller exists")
                    .controller
                    .state;
                let stored = self.plans.get_mut(&key).expect("plan exists");
                stored.applied = Some(write.clone());
                stored.apply_request_id = Some(request_id.to_owned());
                Ok(ApplyResult {
                    plan_id: plan_id.into(),
                    plan_digest: plan.digest,
                    execution_status: ExecutionStatus::Completed,
                    project: prepared.target.slug,
                    revision: write.revision,
                    written: write.written,
                    unchanged: write.unchanged,
                    post_fingerprint: write.post_fingerprint,
                    source_state: write.source_state,
                    controller_state,
                })
            }
            Err(error) => {
                let status = status_for_error(&error);
                let finish = self.vault.finish(permit, status, vec![]);
                let _ = self
                    .controllers
                    .get_mut(&control_key)
                    .expect("controller exists")
                    .controller
                    .executed(status);
                if let Err(vault_error) = finish {
                    return Err(composition_error(vault_error).uncertain());
                }
                Err(error)
            }
        }
    }

    pub fn measure(&mut self, owner: &Owner, plan_id: &str) -> Result<MeasureResult> {
        let (plan, project) = {
            let stored = self.stored_plan(owner, plan_id)?;
            (stored.plan.clone(), stored.plan.body.intent.project.clone())
        };
        self.vault
            .matches(owner, plan_id, &plan)
            .map_err(composition_error)?;
        let snapshot = self.store.snapshot(&project)?;
        let observation = observation(
            owner,
            &project,
            &snapshot,
            plan.body.observation_scope.clone(),
        )?;
        self.vault
            .record_observation(owner, plan_id, 0)
            .map_err(composition_error)?;
        Ok(MeasureResult {
            plan_id: plan_id.into(),
            observation,
            snapshot: snapshot_view(&project, &snapshot),
        })
    }

    pub fn validate(&mut self, owner: &Owner, plan_id: &str) -> Result<ValidateResult> {
        let (evaluation, progress, candidate_count, root_plan_id) =
            self.validation(owner, plan_id)?;
        let report = evaluation.report.validation;
        let findings = report
            .checks
            .iter()
            .filter(|check| {
                report.required_rules.contains(&check.rule) && check.verdict != Verdict::Pass
            })
            .count() as u32;
        self.vault
            .record_observation(owner, plan_id, findings)
            .map_err(composition_error)?;
        let control = self
            .controllers
            .get_mut(&(owner.clone(), root_plan_id))
            .ok_or_else(|| Error::new(ErrorCode::Internal, "Missing authoring controller"))?;
        if control.controller.state == State::Observing {
            let elapsed = control
                .started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            control
                .controller
                .observed(&report, progress.clone(), candidate_count, elapsed)
                .map_err(composition_error)?;
        }
        Ok(ValidateResult {
            plan_id: plan_id.into(),
            report,
            progress,
            controller_state: control.controller.state,
        })
    }

    pub fn verify(&mut self, owner: &Owner, plan_id: &str) -> Result<VerifyResult> {
        let key = (owner.clone(), plan_id.to_owned());
        let (root_plan_id, applied_request_id, repair) = {
            let stored = self.plans.get(&key).ok_or_else(|| {
                Error::new(ErrorCode::PermissionDenied, "Unknown Godot authoring plan")
            })?;
            if stored.applied.is_none() {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Verification requires a completed apply",
                ));
            }
            (
                stored.root_plan_id.clone(),
                stored.apply_request_id.clone().ok_or_else(|| {
                    Error::new(ErrorCode::Internal, "Missing apply request identity")
                })?,
                stored.repair,
            )
        };
        let (evaluation, progress, candidate_count, _) = self.validation(owner, plan_id)?;
        let report = evaluation.report.validation.clone();
        self.vault
            .record_observation(
                owner,
                plan_id,
                report
                    .checks
                    .iter()
                    .filter(|check| {
                        report.required_rules.contains(&check.rule)
                            && check.verdict != Verdict::Pass
                    })
                    .count() as u32,
            )
            .map_err(composition_error)?;
        let control_key = (owner.clone(), root_plan_id);
        let controller_state = {
            let control = self
                .controllers
                .get_mut(&control_key)
                .ok_or_else(|| Error::new(ErrorCode::Internal, "Missing authoring controller"))?;
            if control.controller.state == State::Observing {
                let elapsed = control
                    .started
                    .elapsed()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64;
                control
                    .controller
                    .observed(&report, progress, candidate_count, elapsed)
                    .map_err(composition_error)?;
            }
            control.controller.state
        };
        let verification = evaluation.report;
        let receipt = self.project_receipt(
            owner,
            plan_id,
            &applied_request_id,
            repair,
            verification.clone(),
        )?;
        Ok(VerifyResult {
            plan_id: plan_id.into(),
            report: verification,
            receipt,
            controller_state,
        })
    }

    fn validation(
        &self,
        owner: &Owner,
        plan_id: &str,
    ) -> Result<(EffectEvaluation, Vec<u64>, usize, String)> {
        let stored = self.stored_plan(owner, plan_id)?;
        self.vault
            .matches(owner, plan_id, &stored.plan)
            .map_err(composition_error)?;
        let project = &stored.plan.body.intent.project;
        let snapshot = self.store.snapshot(project)?;
        let current_base = base_state(owner, project, &snapshot)?;
        let request_id = stored.apply_request_id.as_deref().ok_or_else(|| {
            Error::new(
                ErrorCode::Conflict,
                "Effect validation requires a completed authoring apply",
            )
        })?;
        let context = EvaluationContext {
            owner: owner.clone(),
            request_id: request_id.into(),
            plan_digest: stored.plan.digest.clone(),
            contract_digest: stored.effects.digest().map_err(composition_error)?,
            before: stored.plan.body.base.clone(),
            after: current_base,
            operations: stored
                .plan
                .body
                .changes
                .operations
                .iter()
                .map(|operation| operation.id.clone())
                .collect(),
            observation_scope: stored.plan.body.observation_scope.iter().cloned().collect(),
            execution_status: ExecutionStatus::Completed,
            support_level: SupportLevel::Composed,
            budget: stored.plan.body.budget.clone(),
        };
        context
            .validate_plan(&stored.plan, &self.profile, &stored.effects)
            .map_err(composition_error)?;
        let mut adapter = StoreEvidenceAdapter {
            owner,
            project,
            snapshot: &snapshot,
            post_base: &context.after,
        };
        let batch = collect(&stored.effects, &context, &mut adapter).map_err(composition_error)?;
        let evaluation = evaluate(&stored.effects, &context, &batch).map_err(composition_error)?;
        evaluation.verdict().map_err(composition_error)?;
        let bad_files = snapshot
            .files
            .iter()
            .filter(|file| file.state != "current")
            .count() as u64;
        let intent_bad = if snapshot
            .record()
            .is_none_or(|record| record.intent_digest != stored.plan.body.intent_digest)
        {
            1
        } else {
            0
        };
        let has_diverged = snapshot.files.iter().any(|file| file.state == "diverged");
        let has_missing = snapshot.files.iter().any(|file| file.state == "missing");
        let candidate_count = usize::from(has_missing && !has_diverged && intent_bad == 0);
        Ok((
            evaluation,
            vec![bad_files, intent_bad],
            candidate_count,
            stored.root_plan_id.clone(),
        ))
    }

    fn project_receipt(
        &self,
        owner: &Owner,
        plan_id: &str,
        request_id: &str,
        repair: bool,
        verification: VerificationReport,
    ) -> Result<ExecutionReceipt> {
        let stored = self.stored_plan(owner, plan_id)?;
        let command = if repair {
            "driver.godot.composition.repair.apply"
        } else {
            "driver.godot.composition.apply"
        };
        let phase = if repair {
            Phase::RepairApply
        } else {
            Phase::Apply
        };
        let descriptor = self
            .profile
            .capabilities
            .iter()
            .find(|binding| binding.phase == phase && binding.command == command)
            .map(|binding| binding.descriptor.clone())
            .ok_or_else(|| Error::new(ErrorCode::Internal, "Missing profile apply binding"))?;
        let runtime = runtime_identity();
        let outputs: Vec<_> = stored
            .prepared
            .target
            .files
            .values()
            .filter(|file| file.active)
            .map(|file| RevisionPin {
                asset: file.asset.clone(),
                revision: file.revision.clone(),
                fingerprint: Fingerprint {
                    bytes: Some(file.sha256.clone()),
                    projection: None,
                },
                equivalence: Equivalence::ExactBytes,
            })
            .collect();
        let mut unknown_frontier: BTreeSet<_> = [DependencyClass::Runtime].into();
        if !stored.plan.body.intent.assets.is_empty() {
            unknown_frontier.insert(DependencyClass::ImportSettings);
        }
        let effect_digest = stored.effects.digest().map_err(composition_error)?;
        let receipt = ExecutionReceipt {
            version: SCHEMA_VERSION,
            id: ReceiptId::new(),
            derivation: DerivationId::new(),
            project: stored.prepared.target.project.clone(),
            owner: owner.clone(),
            request_id: request_id.into(),
            operation: OperationIdentity {
                capability: command.into(),
                descriptor: descriptor.clone(),
                runtime: runtime.clone(),
                plan: stored.plan.digest.clone(),
                parameters: stored.plan.body.intent_digest.clone(),
                recipe: None,
            },
            source_base: stored.plan.body.base.clone(),
            inputs: vec![],
            outputs,
            determinants: vec![
                Determinant {
                    class: DependencyClass::Runtime,
                    key: "godot_authoring_runtime_identity".into(),
                    digest: runtime.clone(),
                },
                Determinant {
                    class: DependencyClass::Contract,
                    key: "godot_effect_contract".into(),
                    digest: effect_digest,
                },
                Determinant {
                    class: DependencyClass::Contract,
                    key: "godot_generator".into(),
                    digest: Digest::of_bytes(GENERATOR_VERSION.as_bytes()),
                },
            ],
            coverage: Coverage {
                complete: false,
                unknown_frontier,
            },
            verification,
            completed_unix_ms: now_unix_ms()?,
        };
        let adapter =
            ReceiptAdapter::registered(command.into(), descriptor, runtime).map_err(graph_error)?;
        let admitted = adapter
            .admit(owner, request_id, receipt)
            .map_err(graph_error)?;
        Ok(admitted.record().clone())
    }
    fn stored_plan(&self, owner: &Owner, plan_id: &str) -> Result<&StoredPlan> {
        self.plans
            .get(&(owner.clone(), plan_id.to_owned()))
            .ok_or_else(|| Error::new(ErrorCode::PermissionDenied, "Unknown Godot authoring plan"))
    }

    fn ensure_capacity(&self) -> Result<()> {
        if self.plans.len() >= MAX_PLANS {
            Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Godot authoring plan capacity reached",
            ))
        } else {
            Ok(())
        }
    }
}

fn decode<T: DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone()).map_err(Into::into)
}

fn project_resource(project: &str) -> ResourceKey {
    ResourceKey {
        provider: "godot".into(),
        resource: format!("managed:{project}"),
    }
}

fn base_state(owner: &Owner, project: &str, snapshot: &Snapshot) -> Result<BaseStateSet> {
    let revision = snapshot.record().map_or(0, |record| record.revision);
    let document_id = snapshot
        .record()
        .map(|record| record.project.as_str().to_owned())
        .unwrap_or_else(|| format!("new:{project}"));
    let base = BaseStateSet(vec![BaseState {
        key: project_resource(project),
        document_id,
        provider_session: owner.session.clone(),
        generation: format!("{GENERATOR_VERSION}:{revision}"),
        revision: Revision::Fingerprint(snapshot.fingerprint.clone()),
        concurrency: Concurrency::BestEffortRevalidate,
    }]);
    base.validate().map_err(composition_error)?;
    Ok(base)
}

fn native_addresses(resource: &ResourceKey, spec: &GodotAuthoringSpec) -> Vec<Address> {
    spec.scenes
        .iter()
        .flat_map(|scene| {
            [
                NATIVE_READBACK_PROPERTY,
                NATIVE_RUNTIME_PROPERTY,
                NATIVE_PERSISTENCE_PROPERTY,
            ]
            .into_iter()
            .map(|property| Address {
                resource: resource.clone(),
                logical_id: scene.id.clone(),
                property: property.into(),
            })
        })
        .collect()
}

fn native_rule_id(scene: &str, property: &str) -> String {
    format!("godot.{property}.{scene}.v1")
}

fn effect_contract(
    profile: &ProfileDescriptor,
    spec: &GodotAuthoringSpec,
    intent_digest: &Digest,
    operation: &TypedOperation<GodotOperation>,
) -> Result<EffectContract> {
    let resource = project_resource(&spec.project);
    let mut rules = vec![
        EffectRule {
            id: RULE_MANAGED_CURRENT.into(),
            version: 1,
            obligation: Obligation::Required,
            operation_id: operation.id.clone(),
            address: Address {
                resource: resource.clone(),
                logical_id: spec.project.clone(),
                property: "managed_files".into(),
            },
            predicate: Predicate::Equals {
                expected: ObservedValue::Bool { value: true },
            },
            method: ObservationMethod {
                name: "godot_managed_source_hash".into(),
                version: 1,
                source: EvidenceSource::FileRead,
            },
            universe: None,
            artifact: None,
            require_causal_attribution: false,
        },
        EffectRule {
            id: RULE_INTENT_MATCH.into(),
            version: 1,
            obligation: Obligation::Required,
            operation_id: operation.id.clone(),
            address: Address {
                resource: resource.clone(),
                logical_id: spec.project.clone(),
                property: "intent_digest".into(),
            },
            predicate: Predicate::DigestEquals {
                expected: intent_digest.clone(),
            },
            method: ObservationMethod {
                name: "godot_derivation_manifest".into(),
                version: 1,
                source: EvidenceSource::FileRead,
            },
            universe: None,
            artifact: None,
            require_causal_attribution: false,
        },
    ];
    for scene in &spec.scenes {
        for (property, predicate, method) in [
            (
                NATIVE_READBACK_PROPERTY,
                Predicate::Equals {
                    expected: ObservedValue::Bool { value: true },
                },
                "godot_native_scene_readback",
            ),
            (
                NATIVE_RUNTIME_PROPERTY,
                Predicate::Equals {
                    expected: ObservedValue::Bool { value: true },
                },
                "godot_native_runtime_readback",
            ),
            (
                NATIVE_PERSISTENCE_PROPERTY,
                Predicate::Reopened,
                "godot_native_fresh_reopen",
            ),
        ] {
            rules.push(EffectRule {
                id: native_rule_id(&scene.id, property),
                version: 1,
                obligation: Obligation::Preference,
                operation_id: operation.id.clone(),
                address: Address {
                    resource: resource.clone(),
                    logical_id: scene.id.clone(),
                    property: property.into(),
                },
                predicate,
                method: ObservationMethod {
                    name: method.into(),
                    version: 1,
                    source: EvidenceSource::NativeApi,
                },
                universe: None,
                artifact: None,
                require_causal_attribution: false,
            });
        }
    }
    let contract = EffectContract {
        version: EFFECT_CONTRACT_VERSION,
        profile: profile.identity.id.clone(),
        allowed: vec![EffectLimit {
            operation_id: operation.id.clone(),
            effects: operation.effects.clone(),
            writes: operation.writes.iter().cloned().collect(),
        }],
        rules,
    };
    contract.validate().map_err(composition_error)?;
    Ok(contract)
}

struct StoreEvidenceAdapter<'a> {
    owner: &'a Owner,
    project: &'a str,
    snapshot: &'a Snapshot,
    post_base: &'a BaseStateSet,
}

impl EvidenceAdapter for StoreEvidenceAdapter<'_> {
    fn identity(&self, resource: &ResourceKey) -> Option<AdapterIdentity> {
        if *resource != project_resource(self.project) {
            return None;
        }
        let state = self
            .post_base
            .0
            .iter()
            .find(|state| &state.key == resource)?;
        Some(AdapterIdentity {
            owner: (*self.owner).clone(),
            provider: resource.provider.clone(),
            provider_session: state.provider_session.clone(),
            generation: state.generation.clone(),
        })
    }

    fn observe(
        &mut self,
        context: &EvaluationContext,
        rule: &EffectRule,
    ) -> semwright_semantic_composition::Result<AdapterObservation> {
        let value = match rule.id.as_str() {
            RULE_MANAGED_CURRENT => ObservedValue::Bool {
                value: self.snapshot.status == "IN_SYNC",
            },
            RULE_INTENT_MATCH => self
                .snapshot
                .record()
                .map(|record| ObservedValue::Digest {
                    value: record.intent_digest.clone(),
                })
                .ok_or_else(|| {
                    ContractError::Unknown("Godot derivation manifest is unavailable".into())
                })?,
            _ => {
                return Err(ContractError::Unknown(
                    "Godot effect rule has no registered store observer".into(),
                ));
            }
        };
        Ok(AdapterObservation {
            binding: EvidenceBinding {
                owner: (*self.owner).clone(),
                request_id: context.request_id.clone(),
                operation_id: rule.operation_id.clone(),
                plan_digest: context.plan_digest.clone(),
                contract_digest: context.contract_digest.clone(),
            },
            observation: semwright_semantic_composition::ObservationRef {
                id: format!("godot_effect_obs_{}", unique_id()),
                base: self.post_base.clone(),
                source: rule.method.source,
                method: rule.method.name.clone(),
                method_version: rule.method.version,
                scope: context.observation_scope.iter().cloned().collect(),
                artifact: rule.artifact.clone(),
                exhaustive: true,
            },
            readback: ReadbackState::Observed,
            value: Some(value),
            coverage: ObservationCoverage {
                consistent: true,
                missing: vec![],
                attribution: Attribution::Ordered,
                enumeration: None,
            },
        })
    }
}

struct NativeEffectAdapter<'a> {
    store: StoreEvidenceAdapter<'a>,
    scene: &'a str,
    result: &'a NativeVerifyResult,
}

impl NativeEffectAdapter<'_> {
    fn native_observation(&self) -> &super::native_observation::NativeObservation {
        match self.result {
            NativeVerifyResult::Inspect { observation, .. }
            | NativeVerifyResult::Play { observation, .. } => observation,
            NativeVerifyResult::Persistence { reader, .. } => reader,
        }
    }

    fn native_value(
        &self,
        property: &str,
    ) -> semwright_semantic_composition::Result<(ObservedValue, bool)> {
        match property {
            NATIVE_READBACK_PROPERTY => {
                let observation = self.native_observation();
                let live_complete = observation
                    .live
                    .as_ref()
                    .is_none_or(|projection| projection.unknown.is_empty());
                let complete = observation.dependency_complete
                    && observation.authored.unknown.is_empty()
                    && live_complete;
                Ok((ObservedValue::Bool { value: complete }, complete))
            }
            NATIVE_RUNTIME_PROPERTY => match self.result {
                NativeVerifyResult::Play { observation, .. } => {
                    let complete = observation.live.is_some() && !observation.frames.is_empty();
                    let fault_free =
                        complete && observation.frames.iter().all(|frame| frame.fault.is_none());
                    Ok((ObservedValue::Bool { value: fault_free }, complete))
                }
                _ => Err(ContractError::Unknown(
                    "native runtime rule requires a play observation".into(),
                )),
            },
            NATIVE_PERSISTENCE_PROPERTY => match self.result {
                NativeVerifyResult::Persistence { evidence, .. } => Ok((evidence.clone(), true)),
                _ => Err(ContractError::Unknown(
                    "native persistence rule requires save/reopen evidence".into(),
                )),
            },
            _ => Err(ContractError::Unknown(
                "native rule property has no registered observer".into(),
            )),
        }
    }
}

impl EvidenceAdapter for NativeEffectAdapter<'_> {
    fn identity(&self, resource: &ResourceKey) -> Option<AdapterIdentity> {
        self.store.identity(resource)
    }

    fn observe(
        &mut self,
        context: &EvaluationContext,
        rule: &EffectRule,
    ) -> semwright_semantic_composition::Result<AdapterObservation> {
        if matches!(rule.id.as_str(), RULE_MANAGED_CURRENT | RULE_INTENT_MATCH) {
            return self.store.observe(context, rule);
        }
        if rule.address.logical_id != self.scene {
            return Err(ContractError::Unknown(
                "native scene was not observed in this invocation".into(),
            ));
        }
        let (value, exhaustive) = self.native_value(&rule.address.property)?;
        Ok(AdapterObservation {
            binding: EvidenceBinding {
                owner: context.owner.clone(),
                request_id: context.request_id.clone(),
                operation_id: rule.operation_id.clone(),
                plan_digest: context.plan_digest.clone(),
                contract_digest: context.contract_digest.clone(),
            },
            observation: semwright_semantic_composition::ObservationRef {
                id: format!("godot_native_effect_obs_{}", unique_id()),
                base: context.after.clone(),
                source: rule.method.source,
                method: rule.method.name.clone(),
                method_version: rule.method.version,
                scope: context.observation_scope.iter().cloned().collect(),
                artifact: rule.artifact.clone(),
                exhaustive,
            },
            readback: ReadbackState::Observed,
            value: Some(value),
            coverage: ObservationCoverage {
                consistent: exhaustive,
                missing: if exhaustive {
                    vec![]
                } else {
                    vec![rule.address.clone()]
                },
                attribution: Attribution::Isolated,
                enumeration: None,
            },
        })
    }
}

fn observation(
    owner: &Owner,
    project: &str,
    snapshot: &Snapshot,
    scope: Vec<Address>,
) -> Result<semwright_semantic_composition::ObservationRef> {
    Ok(semwright_semantic_composition::ObservationRef {
        id: format!("godot_obs_{}", unique_id()),
        base: base_state(owner, project, snapshot)?,
        source: EvidenceSource::FileRead,
        method: "godot_managed_source_readback".into(),
        method_version: 1,
        scope,
        artifact: Some(snapshot.fingerprint.clone()),
        exhaustive: true,
    })
}

fn snapshot_view(project: &str, snapshot: &Snapshot) -> SnapshotView {
    let record = snapshot.record();
    SnapshotView {
        project: project.into(),
        project_id: record.map(|record| record.project.as_str().to_owned()),
        exists: snapshot.exists,
        status: snapshot.status.clone(),
        revision: record.map(|record| record.revision),
        fingerprint: snapshot.fingerprint.clone(),
        intent_digest: record.map(|record| record.intent_digest.clone()),
        bindings: record
            .map(|record| {
                record
                    .bindings
                    .iter()
                    .map(|(key, value)| (key.clone(), value.as_str().to_owned()))
                    .collect()
            })
            .unwrap_or_default(),
        files: snapshot
            .files
            .iter()
            .map(|file| {
                let managed = record.and_then(|record| record.files.get(&file.path));
                FileObservationView {
                    path: file.path.clone(),
                    expected: file.expected.clone(),
                    actual: file.actual.clone(),
                    state: file.state.clone(),
                    asset: managed.map(|file| file.asset.as_str().to_owned()),
                    revision: managed.map(|file| file.revision.as_str().to_owned()),
                    kind: managed.map(|file| file.kind.clone()),
                    logical_key: managed.map(|file| file.logical_key.clone()),
                    active: managed.map(|file| file.active),
                }
            })
            .collect(),
    }
}

fn runtime_identity() -> Digest {
    Digest::of_bytes(
        format!(
            "semwright-driver-godot:{}:{GENERATOR_VERSION}",
            env!("CARGO_PKG_VERSION")
        )
        .as_bytes(),
    )
}

fn now_unix_ms() -> Result<u64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::new(ErrorCode::Internal, "System clock predates Unix epoch"))?
        .as_millis();
    u64::try_from(millis)
        .map_err(|_| Error::new(ErrorCode::Internal, "System time exceeds receipt range"))
}

fn status_for_error(error: &Error) -> ExecutionStatus {
    if !error.outcome_known {
        ExecutionStatus::Unknown
    } else {
        match error.code {
            ErrorCode::Cancelled => ExecutionStatus::Cancelled,
            ErrorCode::PermissionDenied
            | ErrorCode::PolicyDenied
            | ErrorCode::ConsentRequired
            | ErrorCode::SandboxDenied => ExecutionStatus::Denied,
            _ => ExecutionStatus::Failed,
        }
    }
}

fn composition_error(error: ContractError) -> Error {
    match error {
        ContractError::Invalid(message) => Error::invalid(message),
        ContractError::Stale(message) => Error::new(ErrorCode::StaleReference, message),
        ContractError::Limit(message) => Error::new(ErrorCode::ResourceExhausted, message),
        ContractError::Unknown(message) => Error::new(ErrorCode::Unavailable, message),
        ContractError::Denied(message) => Error::new(ErrorCode::PermissionDenied, message),
    }
}

fn graph_error(error: GraphError) -> Error {
    match error {
        GraphError::Storage => Error::new(
            ErrorCode::Unavailable,
            "Project Graph storage unavailable; original evidence preserved",
        ),
        GraphError::Invalid(message) => Error::invalid(message),
        GraphError::Denied => {
            Error::new(ErrorCode::PermissionDenied, "Project Graph denied receipt")
        }
        GraphError::Conflict => Error::new(ErrorCode::Conflict, "Project Graph receipt conflict"),
        GraphError::Limit(message) => Error::new(ErrorCode::ResourceExhausted, message),
        GraphError::Corrupt => Error::new(ErrorCode::Conflict, "Project Graph evidence is corrupt"),
        GraphError::Cancelled => {
            Error::new(ErrorCode::Cancelled, "Project Graph operation cancelled")
        }
        GraphError::Contract(error) => composition_error(error),
        GraphError::Io(error) => error.into(),
    }
}
