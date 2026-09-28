//! Typed authoring operations through the existing Driver Host.
use super::*;
use semwright_motion_authoring as a;
use semwright_semantic_composition as c;
use std::{collections::BTreeSet, io::BufRead};
fn contract_error(e: c::ContractError) -> Error {
    Error::new(ErrorCode::Conflict, e.to_string())
}
fn host_owner(context: &DriverExecutionContext) -> c::Owner {
    c::Owner {
        session: context.session().into(),
        principal: c::PrincipalBinding::HostSession,
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanArgs {
    pub film: a::Film,
    pub budget: c::ConvergenceBudget,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyPlanArgs {
    pub plan_ref: String,
    #[serde(default)]
    pub dry_run: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObserveArgs {
    pub plan_ref: String,
    pub job_ref: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MotionMutation {
    Realize {
        intent_digest: c::Digest,
        logical_subjects: Vec<String>,
    },
}
type Plan = c::PreparedPlan<a::Film, MotionMutation>;
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Planned {
    pub plan_ref: String,
    pub plan: Plan,
    pub resulting_fingerprint: String,
    pub changes: u32,
    pub repair: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Applied {
    pub plan_ref: String,
    pub applied: bool,
    pub fingerprint: String,
    pub revision: u64,
    pub intent_digest: c::Digest,
    pub execution_status: c::ExecutionStatus,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inspection {
    pub fingerprint: Option<String>,
    pub film: Option<a::Film>,
    pub low_level_project: bool,
    pub authoring_version: u32,
    pub native_acceptance: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RepairChange {
    RestoreRatio {
        finding: String,
        subject: String,
    },
    Reflow {
        finding: String,
        profile: a::OutputProfile,
    },
    ResizeBox {
        finding: String,
        subject: String,
        size: a::Size,
        max_delta: f64,
    },
    MoveAnnotation {
        finding: String,
        annotation: String,
        offset: a::Point,
        max_distance: f64,
    },
    ExtendHold {
        finding: String,
        span: String,
        duration: semwright_media_time::Rational,
    },
    SetGap {
        finding: String,
        subject: String,
        gap: f64,
        max_delta: f64,
    },
}
impl RepairChange {
    fn finding(&self) -> &str {
        match self {
            Self::RestoreRatio { finding, .. }
            | Self::Reflow { finding, .. }
            | Self::ResizeBox { finding, .. }
            | Self::MoveAnnotation { finding, .. }
            | Self::ExtendHold { finding, .. }
            | Self::SetGap { finding, .. } => finding,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepairArgs {
    pub source_plan_ref: String,
    pub job_ref: String,
    pub changes: Vec<RepairChange>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Verified {
    pub measurement: a::MotionMeasurement,
    pub report: c::VerificationReport,
}
struct Stored {
    public: Planned,
    project: Project,
    base_fingerprint: Option<String>,
    applied_fingerprint: Option<String>,
    root: String,
}
pub struct CompositionRuntime {
    vault: c::PlanVault,
    plans: BTreeMap<(c::Owner, String), Stored>,
    renders: BTreeMap<(c::Owner, String), String>,
    controllers: BTreeMap<(c::Owner, String), c::Controller>,
    started: BTreeMap<(c::Owner, String), std::time::Instant>,
}
impl Default for CompositionRuntime {
    fn default() -> Self {
        Self {
            vault: c::PlanVault::bounded(64, 16, 512),
            plans: BTreeMap::new(),
            renders: BTreeMap::new(),
            controllers: BTreeMap::new(),
            started: BTreeMap::new(),
        }
    }
}
fn required_rules(film: &a::Film) -> BTreeSet<String> {
    let mut values = film
        .shots()
        .flat_map(|s| (0..s.constraints.len()).map(move |i| format!("{}:constraint:{i}", s.id)))
        .collect::<BTreeSet<_>>();
    values.extend(
        [
            "native-frame-coverage",
            "resolved-cues",
            "caption-timing",
            "transition-completion",
        ]
        .map(str::to_owned),
    );
    values
}
fn observed_base(
    snapshot: Option<&Snapshot>,
    context: &DriverExecutionContext,
) -> Result<c::BaseStateSet> {
    Ok(c::BaseStateSet(vec![c::BaseState {
        key: c::ResourceKey {
            provider: SCOPE.into(),
            resource: "project".into(),
        },
        document_id: snapshot
            .map_or("absent-project", |s| s.project.id.as_str())
            .into(),
        provider_session: context.session().into(),
        generation: snapshot
            .map_or("absent", |s| s.project.generation.as_str())
            .into(),
        revision: c::Revision::Fingerprint(
            c::Digest::parse(
                snapshot.map_or_else(|| security::sha256(b"absent"), |s| s.source_sha256.clone()),
            )
            .map_err(contract_error)?,
        ),
        concurrency: c::Concurrency::BestEffortRevalidate,
    }]))
}
pub fn catalog() -> Result<Vec<Capability>> {
    let mut caps = vec![
        MotionDriver::cap::<EmptyArgs, Inspection>(
            "driver.motion-canvas.composition.inspect",
            "Inspect the sealed authoring source and current managed project",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        )?,
        MotionDriver::cap::<PlanArgs, Planned>(
            "driver.motion-canvas.composition.plan",
            "Plan a typed Film without native mutation or rendering",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        )?,
        MotionDriver::cap::<ApplyPlanArgs, Applied>(
            "driver.motion-canvas.composition.apply",
            "Apply a server-issued authoring plan",
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
            true,
        )?,
        MotionDriver::cap::<ObserveArgs, a::MotionMeasurement>(
            "driver.motion-canvas.composition.measure",
            "Measure a source-bound native render stream",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        )?,
        MotionDriver::cap::<ObserveArgs, a::MotionMeasurement>(
            "driver.motion-canvas.composition.validate",
            "Validate required constraints against actual native frame evidence",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        )?,
        MotionDriver::cap::<RepairArgs, Planned>(
            "driver.motion-canvas.composition.repair.plan",
            "Plan bounded repairs from fresh native findings",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        )?,
        MotionDriver::cap::<ApplyPlanArgs, Applied>(
            "driver.motion-canvas.composition.repair.apply",
            "Apply a revision-bound server-issued repair plan",
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
            true,
        )?,
        MotionDriver::cap::<ObserveArgs, Verified>(
            "driver.motion-canvas.composition.verify",
            "Reverify current authoring using render-side observations and artifact digests",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        )?,
    ];
    for cap in &mut caps {
        cap.tags.push("composition".into());
        cap.object_types.push("motion-film".into());
    }
    Ok(caps)
}
fn trusted_profile(film: &a::Film) -> Result<c::ProfileDescriptor> {
    let effects = BTreeSet::from([
        c::EffectClass::CreateOwnedObject,
        c::EffectClass::UpdateOwnedObject,
    ]);
    let capability = MotionDriver::cap::<ApplyPlanArgs, Applied>(
        "driver.motion-canvas.composition.apply",
        "Apply a server-issued authoring plan",
        Risk::MutatingReversible,
        Idempotency::NonIdempotent,
        true,
    )?;
    Ok(c::ProfileDescriptor {
        identity: c::ProfileIdentity {
            id: "semwright-motion-authoring".into(),
            version: 1,
            intent_schema: c::schema_digest::<a::Film>().map_err(contract_error)?,
            operation_schema: c::schema_digest::<MotionMutation>().map_err(contract_error)?,
        },
        capabilities: vec![c::CapabilityBinding {
            phase: c::Phase::Apply,
            command: capability.descriptor.name.clone(),
            descriptor: c::Digest::parse(descriptor_digest(&capability.descriptor)?)
                .map_err(contract_error)?,
            effects: effects.clone(),
        }],
        required_rules: required_rules(film),
        allowed_effects: effects,
    })
}
impl MotionDriver {
    fn composition_snapshot(&self) -> Result<Option<Snapshot>> {
        if self.store()?.semantic_path().try_exists()? {
            self.load().map(Some)
        } else {
            Ok(None)
        }
    }
    fn prepare_authoring(
        &mut self,
        input: PlanArgs,
        context: &DriverExecutionContext,
        parent: Option<&str>,
    ) -> Result<Planned> {
        context.check_cancelled()?;
        self.ensure_island_runtime_compatible()?;
        if matches!(self.detect_project()?.mode, ProjectMode::External) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "An external project requires an explicitly created managed island first",
            ));
        }
        input.film.validate().map_err(contract_error)?;
        input.budget.validate().map_err(contract_error)?;
        let current = self.composition_snapshot()?;
        let (project, _) =
            crate::authoring::project(&input.film, current.as_ref().map(|s| &s.project))?;
        let actual = observed_base(current.as_ref(), context)?;
        let owner = host_owner(context);
        let profile = trusted_profile(&input.film)?;
        let ids = input
            .film
            .shots()
            .flat_map(|s| s.subjects.iter().map(|s| s.id.clone()))
            .collect::<Vec<_>>();
        let changes =
            u32::try_from(ids.len() + 1).map_err(|_| Error::invalid("change count overflow"))?;
        let intent_digest = c::canonical_digest(&input.film).map_err(contract_error)?;
        let dependency_digest = c::canonical_digest(&(
            input.film.editorial.clone(),
            input.film.assets.clone(),
            input.film.cues.clone(),
            crate::authoring::COMPILER_EXTENSION_VERSION,
        ))
        .map_err(contract_error)?;
        let address = c::Address {
            resource: actual.0[0].key.clone(),
            logical_id: input.film.id.clone(),
            property: "authoring-project".into(),
        };
        let body = c::PlanBody {
            contract_version: c::CONTRACT_VERSION,
            profile: profile.identity.clone(),
            owner: owner.clone(),
            base: actual,
            intent: input.film,
            intent_digest: intent_digest.clone(),
            dependencies: BTreeMap::from([("render-dependencies".into(), dependency_digest)]),
            changes: c::ChangeSet {
                atomicity: c::Atomicity::AtomicFileReplacement,
                operations: vec![c::TypedOperation {
                    id: "realize-film".into(),
                    payload: MotionMutation::Realize {
                        intent_digest,
                        logical_subjects: ids,
                    },
                    reads: vec![address.clone()],
                    writes: vec![address.clone()],
                    effects: profile.allowed_effects.clone(),
                    depends_on: vec![],
                    postconditions: BTreeSet::from(["fresh-native-render-required".into()]),
                }],
            },
            required_rules: profile.required_rules.clone(),
            observation_scope: vec![address],
            budget: input.budget.clone(),
            require_compare_and_swap: false,
        };
        let plan = c::PreparedPlan::prepare(body, &profile).map_err(contract_error)?;
        let plan_ref = plan.digest.as_str().to_owned();
        let resulting_fingerprint = security::sha256(&serde_json::to_vec_pretty(&project)?);
        let public = Planned {
            plan_ref: plan_ref.clone(),
            plan,
            resulting_fingerprint,
            changes,
            repair: parent.is_some(),
        };
        if self.composition.plans.len() >= 64
            && !self
                .composition
                .plans
                .contains_key(&(owner.clone(), plan_ref.clone()))
        {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "authoring plan capacity",
            ));
        }
        let root = if let Some(parent) = parent {
            let old = self
                .composition
                .plans
                .get(&(owner.clone(), parent.into()))
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::PolicyDenied,
                        "repair source belongs to a different host session",
                    )
                })?;
            old.root.clone()
        } else {
            plan_ref.clone()
        };
        self.composition
            .vault
            .issue(
                &owner,
                &plan_ref,
                &public,
                input.budget.clone(),
                changes,
                parent,
                parent.is_some(),
            )
            .map_err(contract_error)?;
        if parent.is_some() {
            self.composition
                .controllers
                .get_mut(&(owner.clone(), root.clone()))
                .ok_or_else(|| Error::new(ErrorCode::Conflict, "repair controller missing"))?
                .bind_repair(public.plan.digest.clone())
                .map_err(contract_error)?;
        } else {
            self.composition
                .controllers
                .entry((owner.clone(), root.clone()))
                .or_insert(
                    c::Controller::new(
                        public.plan.digest.clone(),
                        input.budget,
                        profile.required_rules,
                    )
                    .map_err(contract_error)?,
                );
            self.composition
                .started
                .entry((owner.clone(), root.clone()))
                .or_insert_with(std::time::Instant::now);
        }
        self.composition
            .plans
            .entry((owner, plan_ref))
            .or_insert(Stored {
                public: public.clone(),
                project,
                base_fingerprint: current.map(|s| s.source_sha256),
                applied_fingerprint: None,
                root,
            });
        Ok(public)
    }
    pub(super) fn record_authoring_render(
        &mut self,
        context: &DriverExecutionContext,
        job: &JobView,
        source: &str,
    ) -> Result<()> {
        if self.composition.renders.len() >= 128 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "authoring render receipt capacity",
            ));
        }
        self.composition
            .renders
            .insert((host_owner(context), job.job_ref.clone()), source.into());
        Ok(())
    }
    async fn apply_authoring(
        &mut self,
        args: ApplyPlanArgs,
        repair: bool,
        context: &DriverExecutionContext,
    ) -> Result<Applied> {
        context.check_cancelled()?;
        let owner = host_owner(context);
        let stored = self
            .composition
            .plans
            .get(&(owner.clone(), args.plan_ref.clone()))
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PolicyDenied,
                    "plan was not issued to this host owner",
                )
            })?;
        self.composition
            .vault
            .matches(&owner, &args.plan_ref, &stored.public)
            .map_err(contract_error)?;
        if stored.public.repair != repair {
            return Err(Error::new(
                ErrorCode::Conflict,
                "plan purpose does not match apply operation",
            ));
        }
        let current = self.composition_snapshot()?;
        let expected = stored.base_fingerprint.clone();
        if current.as_ref().map(|s| s.source_sha256.as_str()) != expected.as_deref() {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "authoring plan source changed",
            ));
        }
        stored
            .public
            .plan
            .body
            .base
            .check_fresh(&observed_base(current.as_ref(), context)?, false)
            .map_err(contract_error)?;
        let project = stored.project.clone();
        let public = stored.public.clone();
        let root = stored.root.clone();
        let mut receipt = Applied {
            plan_ref: args.plan_ref.clone(),
            applied: false,
            fingerprint: public.resulting_fingerprint.clone(),
            revision: project.revision,
            intent_digest: public.plan.body.intent_digest.clone(),
            execution_status: c::ExecutionStatus::Prepared,
        };
        if args.dry_run {
            return Ok(receipt);
        }
        let controller_key = (owner.clone(), root);
        let controller = self
            .composition
            .controllers
            .get(&controller_key)
            .ok_or_else(|| Error::new(ErrorCode::Conflict, "controller absent"))?;
        if controller.state
            != if repair {
                c::State::RepairPlanned
            } else {
                c::State::Prepared
            }
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "illegal composition apply transition",
            ));
        }
        let permit = self
            .composition
            .vault
            .begin(&owner, &args.plan_ref, &public, context.request_id())
            .map_err(contract_error)?;
        self.composition
            .controllers
            .get_mut(&controller_key)
            .expect("checked controller")
            .applying(repair)
            .map_err(contract_error)?;
        if let Err(cancel) = context.check_cancelled() {
            self.composition
                .vault
                .finish(permit, c::ExecutionStatus::Cancelled, vec![])
                .map_err(contract_error)?;
            self.composition
                .controllers
                .get_mut(&controller_key)
                .expect("controller")
                .cancel();
            return Err(cancel);
        }
        let result = if let Some(expected) = expected {
            self.store()?.commit(&expected, &project)
        } else {
            self.store()?.create(&project)
        };
        match result {
            Ok(snapshot) => {
                self.composition
                    .vault
                    .finish(
                        permit,
                        c::ExecutionStatus::Completed,
                        vec!["authoring-project-committed".into()],
                    )
                    .map_err(contract_error)?;
                self.composition
                    .controllers
                    .get_mut(&controller_key)
                    .expect("controller")
                    .executed(c::ExecutionStatus::Completed)
                    .map_err(contract_error)?;
                self.composition
                    .plans
                    .get_mut(&(owner, args.plan_ref))
                    .expect("issued plan")
                    .applied_fingerprint = Some(snapshot.source_sha256.clone());
                receipt.applied = true;
                receipt.fingerprint = snapshot.source_sha256;
                receipt.execution_status = c::ExecutionStatus::Completed;
                Ok(receipt)
            }
            Err(e) => {
                self.composition
                    .vault
                    .finish(permit, c::ExecutionStatus::Unknown, vec![])
                    .map_err(contract_error)?;
                self.composition
                    .controllers
                    .get_mut(&controller_key)
                    .expect("controller")
                    .executed(c::ExecutionStatus::Unknown)
                    .map_err(contract_error)?;
                Err(e)
            }
        }
    }
    async fn measure_authoring(
        &mut self,
        args: ObserveArgs,
        context: &DriverExecutionContext,
    ) -> Result<a::MotionMeasurement> {
        context.check_cancelled()?;
        let owner = host_owner(context);
        let stored = self
            .composition
            .plans
            .get(&(owner.clone(), args.plan_ref.clone()))
            .ok_or_else(|| Error::new(ErrorCode::PolicyDenied, "unknown host-owned plan"))?;
        self.composition
            .vault
            .matches(&owner, &args.plan_ref, &stored.public)
            .map_err(contract_error)?;
        let expected = stored
            .applied_fingerprint
            .clone()
            .ok_or_else(|| Error::new(ErrorCode::Conflict, "plan has not been applied"))?;
        if self
            .composition
            .renders
            .get(&(owner.clone(), args.job_ref.clone()))
            != Some(&expected)
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "render is not associated with this host session and applied source",
            ));
        }
        let snapshot = self.load()?;
        if snapshot.source_sha256 != expected {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "project changed after rendering",
            ));
        }
        let film = stored.public.plan.body.intent.clone();
        let plan_digest = stored.public.plan.digest.clone();
        let root = stored.root.clone();
        let bundle = self
            .renderer
            .native_observations(&args.job_ref, &expected)
            .await?;
        let coverage = a::RangeCoverage {
            first_frame: bundle.plan.first_frame,
            end_frame_exclusive: bundle.plan.end_frame_exclusive,
            observed_frames: 0,
            exhaustive: false,
            fps: semwright_media_time::Rate::new(bundle.plan.fps, bundle.plan.fps_denominator)
                .map_err(contract_error)?,
            method: "motion-canvas-3.17.2-native-frame-probe-v1".into(),
            observation_digest: c::Digest::parse(bundle.observation_sha256)
                .map_err(contract_error)?,
            render_input_digest: c::Digest::parse(bundle.render_input_digest)
                .map_err(contract_error)?,
            artifact_digest: c::Digest::parse(bundle.artifact_sha256).map_err(contract_error)?,
        };
        let reader = std::io::BufReader::new(std::io::Cursor::new(bundle.bytes));
        let frames = reader.split(b'\n').filter_map(|line| match line {
            Ok(v) if v.is_empty() => None,
            Ok(v) => Some(c::strict_decode::<a::FrameObservation>(&v)),
            Err(e) => Some(Err(c::ContractError::Invalid(e.to_string()))),
        });
        let measured = a::measure(
            &film,
            plan_digest,
            observed_base(Some(&snapshot), context)?,
            coverage,
            frames,
        )
        .map_err(contract_error)?;
        self.composition
            .vault
            .record_observation(&owner, &args.plan_ref, measured.findings.len() as u32)
            .map_err(contract_error)?;
        let key = (owner, root);
        let elapsed = self.composition.started.get(&key).map_or(u64::MAX, |t| {
            u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX)
        });
        let controller = self
            .composition
            .controllers
            .get_mut(&key)
            .ok_or_else(|| Error::new(ErrorCode::Conflict, "authoring controller absent"))?;
        let verdict = measured.validation.verdict().map_err(contract_error)?;
        if controller.state == c::State::Observing && verdict != c::Verdict::Fail {
            let unknown = measured
                .validation
                .checks
                .iter()
                .filter(|r| r.verdict == c::Verdict::Unknown)
                .count() as u64;
            controller
                .observed(&measured.validation, vec![0, unknown], 0, elapsed)
                .map_err(contract_error)?;
        }
        // A failed report is returned as data. Repair selection is a separate,
        // explicit typed request; measurements never invent or dispatch writes.
        Ok(measured)
    }
    async fn repair_authoring(
        &mut self,
        args: RepairArgs,
        context: &DriverExecutionContext,
    ) -> Result<Planned> {
        c::ensure(
            !args.changes.is_empty() && args.changes.len() <= 32,
            "repair change budget",
        )
        .map_err(contract_error)?;
        let measured = self
            .measure_authoring(
                ObserveArgs {
                    plan_ref: args.source_plan_ref.clone(),
                    job_ref: args.job_ref,
                },
                context,
            )
            .await?;
        let owner = host_owner(context);
        let stored = self
            .composition
            .plans
            .get(&(owner.clone(), args.source_plan_ref.clone()))
            .ok_or_else(|| Error::new(ErrorCode::PolicyDenied, "repair source absent"))?;
        let mut film = stored.public.plan.body.intent.clone();
        let budget = stored.public.plan.body.budget.clone();
        let root = stored.root.clone();
        let mut seen = BTreeSet::new();
        for change in &args.changes {
            c::ensure(
                seen.insert(change.finding().to_owned()),
                "duplicate repair finding",
            )
            .map_err(contract_error)?;
            let finding = measured
                .findings
                .iter()
                .find(|f| {
                    f.id == change.finding()
                        && f.verdict == c::Verdict::Fail
                        && f.evidence_class == c::EvidenceClass::Deterministic
                })
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::PolicyDenied,
                        "repair requires a fresh deterministic failed native finding",
                    )
                })?;
            apply_repair(&mut film, change, finding)?;
        }
        film.validate().map_err(contract_error)?;
        a::realize(&film).map_err(contract_error)?;
        let key = (owner, root);
        let elapsed = self.composition.started.get(&key).map_or(u64::MAX, |t| {
            u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX)
        });
        let failures = measured
            .validation
            .checks
            .iter()
            .filter(|r| r.verdict == c::Verdict::Fail)
            .count() as u64;
        let unknown = measured
            .validation
            .checks
            .iter()
            .filter(|r| r.verdict == c::Verdict::Unknown)
            .count() as u64;
        let decision = self
            .composition
            .controllers
            .get_mut(&key)
            .ok_or_else(|| Error::new(ErrorCode::Conflict, "repair controller missing"))?
            .observed(&measured.validation, vec![failures, unknown], 1, elapsed)
            .map_err(contract_error)?;
        if decision != c::Decision::PlanRepair {
            return Err(Error::new(
                ErrorCode::Conflict,
                format!("composition stopped before repair: {decision:?}"),
            ));
        }
        self.prepare_authoring(
            PlanArgs { film, budget },
            context,
            Some(&args.source_plan_ref),
        )
    }
    pub(super) async fn execute_composition(
        &mut self,
        command: &str,
        digest: &str,
        args: Value,
        context: &DriverExecutionContext,
    ) -> Result<Value> {
        context.check_cancelled()?;
        let capability = catalog()?
            .into_iter()
            .find(|c| c.descriptor.name == command)
            .ok_or_else(|| {
                Error::new(ErrorCode::Unsupported, "authoring capability unavailable")
            })?;
        if descriptor_digest(&capability.descriptor)? != digest {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "authoring descriptor changed",
            ));
        }
        let input = jsonschema::validator_for(&capability.descriptor.input_schema)
            .map_err(|_| Error::new(ErrorCode::Internal, "invalid authoring input schema"))?;
        if !input.is_valid(&args) {
            return Err(Error::invalid("authoring arguments do not match schema"));
        }
        let value = match command {
            "driver.motion-canvas.composition.inspect" => {
                let _: EmptyArgs = Self::parse(args)?;
                let snapshot = self.composition_snapshot()?;
                let film = snapshot
                    .as_ref()
                    .and_then(|s| s.project.authoring.as_ref().map(|a| a.intent.clone()));
                serde_json::to_value(Inspection{fingerprint:snapshot.as_ref().map(|s|s.source_sha256.clone()),low_level_project:snapshot.is_some()&&film.is_none(),film,authoring_version:1,native_acceptance:"Inspection is not native acceptance; source-bound render and verification are required".into()})?
            }
            "driver.motion-canvas.composition.plan" => {
                serde_json::to_value(self.prepare_authoring(Self::parse(args)?, context, None)?)?
            }
            "driver.motion-canvas.composition.apply"
            | "driver.motion-canvas.composition.repair.apply" => serde_json::to_value(
                self.apply_authoring(
                    Self::parse(args)?,
                    command.ends_with("repair.apply"),
                    context,
                )
                .await?,
            )?,
            "driver.motion-canvas.composition.measure"
            | "driver.motion-canvas.composition.validate" => {
                serde_json::to_value(self.measure_authoring(Self::parse(args)?, context).await?)?
            }
            "driver.motion-canvas.composition.repair.plan" => {
                serde_json::to_value(self.repair_authoring(Self::parse(args)?, context).await?)?
            }
            "driver.motion-canvas.composition.verify" => {
                let measurement = self.measure_authoring(Self::parse(args)?, context).await?;
                let report = c::VerificationReport {
                    execution_status: c::ExecutionStatus::Completed,
                    validation: measurement.validation.clone(),
                    support_level: c::SupportLevel::Native,
                    effects_observed: vec![],
                    effects_unobservable: vec![],
                };
                serde_json::to_value(Verified {
                    measurement,
                    report,
                })?
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "unknown authoring operation",
                ));
            }
        };
        let output = jsonschema::validator_for(&capability.descriptor.output_schema)
            .map_err(|_| Error::new(ErrorCode::Internal, "invalid authoring output schema"))?;
        if !output.is_valid(&value) {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "authoring response violates its descriptor",
            ));
        }
        if serde_json::to_vec(&value)?.len() > semwright_types::MAX_FRAME - 4096 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "authoring response exceeds protocol frame budget",
            ));
        }
        Ok(value)
    }
}
fn subject_mut<'a>(film: &'a mut a::Film, id: &str) -> Result<&'a mut a::Subject> {
    film.sequences
        .iter_mut()
        .flat_map(|s| &mut s.beats)
        .flat_map(|b| &mut b.shots)
        .flat_map(|s| &mut s.subjects)
        .find(|s| s.id == id)
        .ok_or_else(|| Error::invalid("repair subject absent"))
}
fn apply_repair(
    film: &mut a::Film,
    change: &RepairChange,
    finding: &a::MotionFinding,
) -> Result<()> {
    let (shot_id, rule) = film
        .shots()
        .find_map(|shot| {
            shot.constraints
                .iter()
                .enumerate()
                .find(|(i, _)| format!("{}:constraint:{i}", shot.id) == finding.rule)
                .map(|(_, r)| (shot.id.clone(), r.clone()))
        })
        .ok_or_else(|| {
            Error::new(
                ErrorCode::PolicyDenied,
                "finding does not refer to a declared repairable constraint",
            )
        })?;
    let reject = || {
        Error::new(
            ErrorCode::PolicyDenied,
            "repair kind or subject does not match its native finding",
        )
    };
    match change {
        RepairChange::RestoreRatio { subject, .. } => {
            let a::VisualConstraint::AspectRatio {
                subject: expected,
                ratio,
                ..
            } = rule
            else {
                return Err(reject());
            };
            if expected != *subject || finding.subject != *subject {
                return Err(reject());
            }
            let s = subject_mut(film, subject)?;
            let a::SpatialIntent::Fixed { size, .. } = &mut s.layout else {
                return Err(Error::invalid("ratio repair requires a fixed box"));
            };
            size.height = size.width / ratio;
        }
        RepairChange::Reflow { profile, .. } => {
            if !matches!(
                rule,
                a::VisualConstraint::SafeArea { .. } | a::VisualConstraint::NoOverlap { .. }
            ) {
                return Err(reject());
            }
            film.output = profile.clone();
        }
        RepairChange::ResizeBox {
            subject,
            size,
            max_delta,
            ..
        } => {
            let expected = match rule {
                a::VisualConstraint::NoTruncation { subject }
                | a::VisualConstraint::SafeArea { subject, .. } => subject,
                _ => return Err(reject()),
            };
            if expected != *subject || finding.subject != *subject {
                return Err(reject());
            }
            c::ensure(
                max_delta.is_finite() && (0.0..=512.0).contains(max_delta),
                "resize allowance must be finite and bounded",
            )
            .map_err(contract_error)?;
            let s = subject_mut(film, subject)?;
            let a::SpatialIntent::Fixed { size: old, .. } = &mut s.layout else {
                return Err(Error::invalid("resize repair requires a fixed box"));
            };
            c::ensure(
                (old.width - size.width).abs() <= *max_delta
                    && (old.height - size.height).abs() <= *max_delta,
                "resize exceeds declared delta",
            )
            .map_err(contract_error)?;
            *old = *size;
        }
        RepairChange::MoveAnnotation {
            annotation,
            offset,
            max_distance,
            ..
        } => {
            let a::VisualConstraint::SafeArea { subject, .. } = rule else {
                return Err(reject());
            };
            c::ensure(
                max_distance.is_finite() && (0.0..=512.0).contains(max_distance),
                "annotation distance allowance",
            )
            .map_err(contract_error)?;
            let item = film
                .sequences
                .iter_mut()
                .flat_map(|s| &mut s.beats)
                .flat_map(|b| &mut b.shots)
                .filter(|s| s.id == shot_id)
                .flat_map(|s| &mut s.annotations)
                .find(|v| v.id == *annotation && v.subject == subject)
                .ok_or_else(reject)?;
            c::ensure(
                ((item.offset.x - offset.x).powi(2) + (item.offset.y - offset.y).powi(2)).sqrt()
                    <= *max_distance,
                "annotation move exceeds allowed distance",
            )
            .map_err(contract_error)?;
            item.offset = *offset;
        }
        RepairChange::ExtendHold { span, duration, .. } => {
            let a::VisualConstraint::MinimumVisible { subject, .. } = rule else {
                return Err(reject());
            };
            let shot = film.shots().find(|s| s.id == shot_id).ok_or_else(reject)?;
            let allowed=shot.span_id==*span||shot.motion.iter().any(|m|m.span_id==*span&&matches!(&m.primitive,a::Primitive::Hold{targets}if targets.contains(&subject)));
            if !allowed {
                return Err(reject());
            }
            let target = film
                .timing
                .spans
                .iter_mut()
                .find(|s| s.id == *span)
                .ok_or_else(|| Error::invalid("hold span missing"))?;
            c::ensure(
                *duration >= target.preferred && *duration <= target.maximum,
                "hold extension exceeds declared slack",
            )
            .map_err(contract_error)?;
            target.preferred = *duration;
        }
        RepairChange::SetGap {
            subject,
            gap,
            max_delta,
            ..
        } => {
            let a::VisualConstraint::NoOverlap {
                subject: left,
                other: right,
                ..
            } = rule
            else {
                return Err(reject());
            };
            let shot = film.shots().find(|s| s.id == shot_id).ok_or_else(reject)?;
            let shares_parent = [left, right].iter().all(|id| {
                shot.subjects
                    .iter()
                    .any(|s| s.id == *id && s.parent.as_deref() == Some(subject.as_str()))
            });
            if !shares_parent {
                return Err(reject());
            }
            c::ensure(
                gap.is_finite() && max_delta.is_finite() && (0.0..=512.0).contains(max_delta),
                "finite gap allowance required",
            )
            .map_err(contract_error)?;
            let s = subject_mut(film, subject)?;
            let old = match &mut s.layout {
                a::SpatialIntent::Stack { gap, .. }
                | a::SpatialIntent::Split { gap, .. }
                | a::SpatialIntent::Grid { gap, .. } => gap,
                _ => return Err(Error::invalid("gap repair requires native layout")),
            };
            c::ensure(
                (*old - gap).abs() <= *max_delta,
                "gap delta exceeds allowance",
            )
            .map_err(contract_error)?;
            *old = *gap;
        }
    }
    film.validate().map_err(contract_error)?;
    a::realize(film).map_err(contract_error)?;
    Ok(())
}
