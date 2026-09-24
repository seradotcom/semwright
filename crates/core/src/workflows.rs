use super::*;
use semwright_registry::Metadata;
use semwright_workflow::{
    Candidate, DescriptorLookup, ParameterHint, Promotion, TraceStep, WorkflowManager,
    WorkflowTrace, compile as compile_trace, promoted_descriptor, sanitize, verify_drift,
};
use sha2::{Digest, Sha256};
use std::path::Path;

impl DescriptorLookup for Broker {
    fn describe(&self, command: &str) -> Result<CommandDescriptor> {
        Broker::describe(self, command)
    }
}

fn descriptor_digest(descriptor: &CommandDescriptor) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(descriptor)?)
    ))
}

fn learned_recipe_metadata(identity: &ProviderIdentity) -> Metadata {
    let mut metadata = Metadata::for_provider(identity);
    metadata.tags = vec!["learned".into(), "recipe".into(), "workflow".into()];
    metadata.object_types = vec!["workflow".into()];
    metadata
}

fn recording_command(command: &str) -> bool {
    ![
        "workflow.",
        "recipe.",
        "audit.",
        "capabilities.",
        "commands.",
        "jobs.",
        "events.",
        "plugin.",
    ]
    .iter()
    .any(|prefix| command.starts_with(prefix))
        && command != "doctor"
}
impl Broker {
    pub fn configure_workflows(&self, directory: &Path) -> Result<Value> {
        let promotions = {
            let mut workflows = self
                .workflows
                .lock()
                .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?;
            workflows.configure_persistence(directory)?;
            workflows.promotions()
        };
        let mut prepared = vec![];
        let mut stale = vec![];
        for promotion in promotions {
            let descriptor = match verify_drift(&promotion.candidate, self)
                .and_then(|_| promoted_descriptor(&promotion.slug, &promotion.candidate, self))
            {
                Ok(descriptor) if descriptor.name == promotion.capability => descriptor,
                Ok(_) => {
                    stale.push(
                        json!({"slug":promotion.slug,"reason":"capability identity mismatch"}),
                    );
                    continue;
                }
                Err(error) => {
                    stale.push(json!({"slug":promotion.slug,"reason":error.message}));
                    continue;
                }
            };
            prepared.push((promotion, descriptor));
        }
        let mut catalog = self
            .registry
            .write()
            .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?;
        let mut candidate_catalog = catalog.clone();
        let mut restored = vec![];
        for (promotion, descriptor) in prepared {
            let identity = ProviderIdentity::external(
                SourceKind::Recipe,
                &promotion.slug,
                &descriptor.version,
            )?;
            candidate_catalog
                .register_with_metadata(descriptor.clone(), learned_recipe_metadata(&identity))?;
            restored.push(descriptor.name);
        }
        *catalog = candidate_catalog;
        Ok(json!({"restored":restored,"stale":stale}))
    }

    pub(super) fn workflow_record_start(
        &self,
        session: &str,
        name: &str,
        intent: &str,
        capture_values: bool,
    ) -> Result<Value> {
        let id = self
            .workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .start(session, name, intent, capture_values)?;
        Ok(json!({
            "recording":true,
            "trace_id":id,
            "capture_values":capture_values,
            "privacy":if capture_values {
                "structured values are stored locally; sensitive-looking fields are still redacted"
            } else {
                "metadata only; non-reference values are redacted"
            }
        }))
    }
    pub(super) fn workflow_record_stop(&self, session: &str, successful: bool) -> Result<Value> {
        let trace = self
            .workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .stop(session, successful)?;
        Ok(json!({"recording":false,"trace":trace}))
    }

    pub(super) fn workflow_traces(&self) -> Result<Value> {
        let traces = self
            .workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .list_traces();
        Ok(json!({"traces":traces}))
    }

    pub(super) fn workflow_trace(&self, id: &str) -> Result<Value> {
        let trace = self
            .workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .trace(id)?;
        Ok(json!({"trace":trace}))
    }

    pub(super) fn workflow_trace_delete(&self, id: &str) -> Result<Value> {
        let trace = self
            .workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .delete_trace(id)?;
        Ok(json!({"deleted":true,"trace_id":trace.id}))
    }

    pub(super) fn workflow_compile(
        &self,
        trace_ids: &[String],
        name: &str,
        description: &str,
        hints: Vec<ParameterHint>,
    ) -> Result<Value> {
        if trace_ids.is_empty() || trace_ids.len() > 8 {
            return Err(Error::invalid("Compile requires 1..8 explicit trace IDs"));
        }
        let traces = {
            let workflows = self
                .workflows
                .lock()
                .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?;
            trace_ids
                .iter()
                .map(|id| workflows.trace(id))
                .collect::<Result<Vec<_>>>()?
        };
        let candidate = compile_trace(&traces, name, description, &hints, self)?;
        self.workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .store_candidate(candidate.clone())?;
        Ok(json!({"candidate":candidate}))
    }

    pub(super) fn workflow_candidate(&self, id: &str) -> Result<Candidate> {
        self.workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .candidate(id)
    }

    pub(super) fn workflow_candidate_delete(&self, id: &str) -> Result<Value> {
        let candidate = self
            .workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .delete_candidate(id)?;
        Ok(json!({"deleted":true,"candidate_id":candidate.id}))
    }

    pub(super) fn workflow_verify(self: &Arc<Self>, session: &str, id: &str) -> Result<Value> {
        let candidate = self.workflow_candidate(id)?;
        let drift = verify_drift(&candidate, self.as_ref())?;
        let validation = candidate.recipe.validate(&RecipeBridge {
            broker: self.clone(),
            session: session.into(),
        })?;
        let candidate = self
            .workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .mark_static_verified(id)?;
        Ok(json!({
            "verified":true,
            "candidate":candidate,
            "drift":drift,
            "recipe_validation":validation
        }))
    }

    pub(super) async fn workflow_replay(
        self: &Arc<Self>,
        session: &str,
        id: &str,
        inputs: Value,
        dry_run: bool,
        cancellation: CancellationToken,
    ) -> Result<Value> {
        let candidate = self.workflow_candidate(id)?;
        verify_drift(&candidate, self.as_ref())?;
        if !dry_run && !candidate.static_verified {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Workflow candidate must pass workflow.verify before a live replay",
            ));
        }
        candidate.recipe.validate(&RecipeBridge {
            broker: self.clone(),
            session: session.into(),
        })?;
        let output = candidate
            .recipe
            .run(
                &RecipeBridge {
                    broker: self.clone(),
                    session: session.into(),
                },
                inputs,
                dry_run,
                cancellation,
            )
            .await?;
        if !dry_run {
            self.workflows
                .lock()
                .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
                .mark_replay(id)?;
        }
        Ok(output)
    }
    pub(super) fn workflow_promote(&self, candidate_id: &str, slug: &str) -> Result<Value> {
        let candidate = self.workflow_candidate(candidate_id)?;
        verify_drift(&candidate, self)?;
        let descriptor = promoted_descriptor(slug, &candidate, self)?;
        let identity = ProviderIdentity::external(SourceKind::Recipe, slug, &descriptor.version)?;

        let mut catalog = self
            .registry
            .write()
            .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?;
        let mut candidate_catalog = catalog.clone();
        candidate_catalog
            .register_with_metadata(descriptor.clone(), learned_recipe_metadata(&identity))?;

        let promotion = self
            .workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .promote(slug, candidate_id, &descriptor.name)?;
        *catalog = candidate_catalog;
        drop(catalog);
        self.event(self.core_event("registry_changed"));
        Ok(json!({"promoted":true,"promotion":promotion,"capability":descriptor}))
    }
    pub(super) fn workflow_promotions(&self) -> Result<Value> {
        let promotions = self
            .workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .promotions();
        let rows = promotions
            .into_iter()
            .map(|promotion| {
                let status = verify_drift(&promotion.candidate, self)
                    .map(|_| "valid")
                    .unwrap_or("stale");
                json!({
                    "slug":promotion.slug,
                    "capability":promotion.capability,
                    "candidate_id":promotion.candidate.id,
                    "status":status,
                    "promoted_unix_ms":promotion.promoted_unix_ms
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({"promotions":rows}))
    }

    pub(super) fn workflow_demote(&self, slug: &str) -> Result<Value> {
        let promotion = {
            let workflows = self
                .workflows
                .lock()
                .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?;
            workflows
                .promotions()
                .into_iter()
                .find(|value| value.slug == slug)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "Promoted workflow not found"))?
        };
        let mut catalog = self
            .registry
            .write()
            .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?;
        let mut candidate_catalog = catalog.clone();
        if let Err(error) = candidate_catalog.remove_recipe_command(&promotion.capability)
            && error.code != ErrorCode::NotFound
        {
            return Err(error);
        }
        let removed = self
            .workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .demote(slug)?;
        *catalog = candidate_catalog;
        drop(catalog);
        self.event(self.core_event("registry_changed"));
        Ok(json!({"demoted":true,"promotion":removed}))
    }

    pub(super) async fn execute_promoted_workflow(
        self: &Arc<Self>,
        session: &str,
        capability: &str,
        inputs: Value,
        dry_run: bool,
        cancellation: CancellationToken,
    ) -> Result<Value> {
        let promotion = self
            .workflows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Workflow store lock poisoned"))?
            .promotion_by_capability(capability)?;
        verify_drift(&promotion.candidate, self.as_ref())?;
        promotion
            .candidate
            .recipe
            .run(
                &RecipeBridge {
                    broker: self.clone(),
                    session: session.into(),
                },
                inputs,
                dry_run,
                cancellation,
            )
            .await
    }

    pub(super) fn record_workflow_execution(
        &self,
        session: &str,
        request: &ExecuteRequest,
        descriptor: &CommandDescriptor,
        metadata: &Metadata,
        selected: &str,
        duration_ms: u64,
        result: &Result<Value>,
    ) {
        if !recording_command(&request.command) {
            return;
        }
        let capture_values = match self.workflows.lock() {
            Ok(workflows) => workflows.active_capture_values(session),
            Err(_) => return,
        };
        let Some(capture_values) = capture_values else {
            return;
        };
        let effective_capture = capture_values && descriptor.risk != Risk::SecretAccess;
        let (args, args_redacted) = match sanitize(&request.args, effective_capture) {
            Ok(value) => value,
            Err(error) => {
                if let Ok(mut workflows) = self.workflows.lock() {
                    workflows.invalidate(
                        session,
                        &format!(
                            "Workflow step arguments could not be recorded: {}",
                            error.message
                        ),
                    );
                }
                return;
            }
        };
        let (recorded_result, result_redacted) = match result {
            Ok(value) => match sanitize(value, effective_capture) {
                Ok((value, redacted)) => (Some(value), redacted),
                Err(error) => {
                    if let Ok(mut workflows) = self.workflows.lock() {
                        workflows.invalidate(
                            session,
                            &format!(
                                "Workflow step result could not be recorded: {}",
                                error.message
                            ),
                        );
                    }
                    return;
                }
            },
            Err(_) => (None, false),
        };
        let step = TraceStep {
            index: 0,
            command: request.command.clone(),
            args,
            result: recorded_result,
            ok: result.is_ok(),
            error_code: result.as_ref().err().map(|error| error.code),
            outcome_known: result
                .as_ref()
                .err()
                .is_none_or(|error| error.outcome_known),
            risk: descriptor.risk,
            idempotency: descriptor.idempotency,
            capability_version: descriptor.version.clone(),
            descriptor_sha256: metadata.descriptor_sha256.clone(),
            backend: selected.into(),
            provider: Some(metadata.provider.clone()),
            duration_ms,
            redacted: args_redacted || result_redacted || descriptor.risk == Risk::SecretAccess,
        };
        if let Ok(mut workflows) = self.workflows.lock() {
            let _ = workflows.record(session, step);
        }
    }
}
