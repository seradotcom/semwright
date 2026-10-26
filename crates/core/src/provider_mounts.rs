//! Session-scoped bindings to a real DriverHost mount and the exact Broker lease.
//! The returned path is evidence, never a safe local-write capability or project ACL.
use super::*;
use semwright_platform_api::launch::{MountClass, WorkspaceMountBinding};

pub(super) type MountBindings = Vec<Arc<dyn WorkspaceMountBinding>>;
#[derive(Clone)]
pub(super) struct BoundProvider {
    pub(super) reference_id: String,
    pub(super) lease: Arc<ProviderLease>,
    pub(super) admission: ProviderMountReference,
    pub(super) parent_request_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundRequest {
    binding_ref: String,
    request: ExecuteRequest,
}
impl Broker {
    pub(super) fn validate_owner_workspace_bindings(&self, bindings: &MountBindings) -> Result<()> {
        let mut names = std::collections::BTreeSet::new();
        for binding in bindings {
            let mount = binding.mount();
            mount.validate()?;
            let grant = self
                .policy
                .config()
                .filesystem
                .iter()
                .find(|grant| grant.name == mount.logical_name)
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::PolicyDenied,
                        "Mounted workspace has no matching owner filesystem grant",
                    )
                })?;
            if mount.class != MountClass::Workspace
                || std::path::Path::new(&mount.logical_name)
                    .components()
                    .count()
                    != 1
                || !names.insert(&mount.logical_name)
                || grant.path != mount.source
                || !grant.read
                || (!mount.read_only && !grant.write)
            {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Mounted workspace does not match its owner filesystem grant",
                ));
            }
            binding.validate_current()?;
        }
        Ok(())
    }
    fn require_mount_read_policy(&self, root: &str) -> Result<()> {
        // Reuse the canonical capability/explicit-deny evaluation. A platform fact is not policy.
        let descriptor = self.describe("providers.mount.observe")?;
        self.policy
            .enforce(&descriptor, &json!({"root":root}), None)?;
        Ok(())
    }
    fn mount_binding(
        &self,
        lease: &Arc<ProviderLease>,
        root: &str,
    ) -> Result<Arc<dyn WorkspaceMountBinding>> {
        if !lease.active()
            || lease.binding_epoch.is_cancelled()
            || !self
                .provider(&lease.identity.id)
                .is_some_and(|current| Arc::ptr_eq(&current, lease))
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Provider mount binding belongs to an inactive or replaced generation",
            ));
        }
        self.require_mount_read_policy(root)?;
        let bindings = lease.workspace_mounts.as_ref().map_err(Clone::clone)?;
        let binding = bindings
            .iter()
            .find(|binding| binding.mount().logical_name == root)
            .cloned()
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PermissionDenied,
                    "The selected provider has no authenticated mount for this logical root",
                )
            })?;
        binding.validate_current().map_err(|_| {
            Error::new(
                ErrorCode::StaleReference,
                "Mounted root instance cannot be revalidated",
            )
        })?;
        if !lease.active()
            || lease.binding_epoch.is_cancelled()
            || !self
                .provider(&lease.identity.id)
                .is_some_and(|current| Arc::ptr_eq(&current, lease))
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Provider generation changed while resolving its mount",
            ));
        }
        Ok(binding)
    }
    pub(super) fn observe_provider_mount(&self, session: &str, args: &Value) -> Result<Value> {
        let route = arg_str(args, "provider")?;
        let root = arg_str(args, "root")?;
        let lease = self.provider(route).ok_or_else(|| {
            Error::new(ErrorCode::NotFound, "Requested provider is not connected")
        })?;
        let binding = self.mount_binding(&lease, root)?;
        let reference_id = self
            .references
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Reference store lock poisoned"))?
            .insert_provider_mount(session, route, &lease.connection, lease.generation, root)?;
        let bound = self.resolve_bound_provider(session, &reference_id)?;
        self.check_bound_provider(session, &bound, Some(&lease))?;
        let reference = self
            .references
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Reference store lock poisoned"))?
            .get_provider_mount(&reference_id, session)?;
        let mount = binding.mount();
        Ok(json!({
            "binding_ref": reference_id,
            "provider": lease.identity.id,
            "provider_generation": lease.generation,
            "connection": lease.connection,
            "mount": {"root":mount.logical_name,"path":mount.source,
                "read":true,"write":!mount.read_only,"execute":mount.execute},
            "admission_ttl_ms": reference.expires.saturating_duration_since(Instant::now())
                .as_millis().min(u64::MAX as u128) as u64,
        }))
    }
    fn resolve_bound_provider(&self, session: &str, reference_id: &str) -> Result<BoundProvider> {
        // Drop the store lock before consulting the catalog or validating native root identity.
        let reference = self
            .references
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Reference store lock poisoned"))?
            .get_provider_mount(reference_id, session)?;
        let lease = self
            .provider(&reference.backend)
            .ok_or_else(|| Error::new(ErrorCode::StaleReference, "Bound provider disappeared"))?;
        if lease.connection != reference.connection || lease.generation != reference.generation {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Provider connection or generation no longer matches the binding",
            ));
        }
        self.mount_binding(&lease, &reference.mount)?;
        Ok(BoundProvider {
            reference_id: reference_id.into(),
            lease,
            admission: reference,
            parent_request_id: None,
        })
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn resolve_bound_provider_for_owner_operation_test(
        &self,
        session: &str,
        reference_id: &str,
    ) -> Result<BoundProvider> {
        self.resolve_bound_provider(session, reference_id)
    }

    pub(super) fn check_bound_provider(
        &self,
        session: &str,
        expected: &BoundProvider,
        selected: Option<&Arc<ProviderLease>>,
    ) -> Result<()> {
        // Re-read the typed reference on every fence: revoke_session, TTL and invalidation
        // remain effective even if a provider itself did not disconnect.
        let current = self.resolve_bound_provider(session, &expected.reference_id)?;
        if !current.admission.same_admission(&expected.admission)
            || expected.admission.is_revoked()
            || !Arc::ptr_eq(&current.lease, &expected.lease)
            || selected.is_none_or(|selected| !Arc::ptr_eq(selected, &current.lease))
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Bound operation selected another provider lease",
            ));
        }
        Ok(())
    }
    /// Successful completion may outlive admission TTL; all other authority fences remain.
    pub(super) fn check_bound_completion(
        &self,
        session: &str,
        expected: &BoundProvider,
        selected: Option<&Arc<ProviderLease>>,
    ) -> Result<bool> {
        let record = self
            .references
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Reference store lock poisoned"))?
            .get_provider_mount_for_completion(&expected.reference_id, session)?;
        if expected.admission.is_revoked() || !record.same_admission(&expected.admission) {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Bound admission was revoked or replaced",
            ));
        }
        let current = self
            .provider(&record.backend)
            .ok_or_else(|| Error::new(ErrorCode::StaleReference, "Bound provider disappeared"))?;
        if current.connection != record.connection
            || current.generation != record.generation
            || !Arc::ptr_eq(&current, &expected.lease)
            || selected.is_none_or(|selected| !Arc::ptr_eq(selected, &current))
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Bound completion selected another lease",
            ));
        }
        self.mount_binding(&current, &record.mount)?;
        if expected.admission.is_revoked() {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Bound admission was revoked during completion",
            ));
        }
        Ok(record.expires > Instant::now())
    }
    pub(super) async fn execute_provider_bound(
        self: &Arc<Self>,
        session: &str,
        outer_request_id: &str,
        outer: &ExecuteRequest,
        cancellation: CancellationToken,
        audit: &mut audit::Scope,
    ) -> Result<Value> {
        // Audit's canonical request identity is 32 hexadecimal characters. Require this
        // for the new wrapper so every uncertain child outcome has an exact parent link.
        if outer_request_id.len() != 32 || !outer_request_id.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::invalid(
                "Bound transport requires a canonical request identifier",
            ));
        }
        let args: BoundRequest = serde_json::from_value(outer.args.clone())?;
        if outer
            .backend
            .as_deref()
            .is_some_and(|value| value != "core")
            || outer.dry_run != args.request.dry_run
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Bound transport cannot change route or nested dry-run semantics",
            ));
        }
        let invocation = self.invocation(&args.request.command)?;
        let selected = invocation.dynamic_provider.as_ref().ok_or_else(|| {
            Error::new(
                ErrorCode::PolicyDenied,
                "A mount binding may execute only a direct command of its provider",
            )
        })?;
        if invocation.capability.metadata.source != SourceKind::Driver {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Only direct DriverHost commands can use a workspace binding",
            ));
        }
        invocation.capability.validate_input(&args.request.args)?;
        // Public classification stays conservative Mutating/NonIdempotent. At execution
        // time this trusted dispatch has resolved the actual command, so its actual risk
        // determines confirmation. Wrapper capability denial is still independently enforced.
        let mut transport = self.describe("providers.execute_bound")?;
        transport.risk = invocation.capability.descriptor.risk;
        transport.interactive_consent = invocation.capability.descriptor.interactive_consent;
        let app = invocation.capability.metadata.app.as_deref();
        let outer_confirmation = self.policy.enforce(&transport, &outer.args, app)?;
        let nested_confirmation =
            self.policy
                .enforce(&invocation.capability.descriptor, &args.request.args, app)?;
        if outer_confirmation && !nested_confirmation {
            return Err(Error::new(
                ErrorCode::ConsentRequired,
                "Bound transport confirmation is not covered by the actual command",
            ));
        }
        audit.decision(if outer_confirmation {
            "require_confirmation"
        } else {
            "allow"
        });
        let bound = self.resolve_bound_provider(session, &args.binding_ref)?;
        if args
            .request
            .backend
            .as_deref()
            .is_some_and(|route| route != bound.lease.identity.id)
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Bound command cannot select another execution route",
            ));
        }
        self.check_bound_provider(session, &bound, Some(selected))?;
        let mut child_bound = bound.clone();
        child_bound.parent_request_id = Some(outer_request_id.into());
        let nested = Box::pin(self.clone().execute_inner(
            session.into(),
            unique_id(),
            args.request,
            cancellation,
            Some(child_bound),
        ))
        .await;
        if !nested.ok {
            // Original typed outcome is preserved; canonical child audit retains correlation.
            return Err(nested.error.unwrap_or_else(|| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "Bound command returned failure without a typed outcome",
                )
                .uncertain()
            }));
        }
        let admission_current = self
            .check_bound_completion(session, &bound, Some(&bound.lease))
            .map_err(|_| {
                Error::new(
                    ErrorCode::StaleReference,
                    "Bound result is historical; admission changed during completion",
                )
                .uncertain()
            })?;
        if outer_confirmation && !nested.execution.dry_run {
            audit.decision("allow_after_confirmation");
        }
        let output = json!({"binding_ref":args.binding_ref,"result":nested,
            "admission_current":admission_current,
            "admission_state":if admission_current {"current"} else {"expired"}});
        let bytes = serde_json::to_vec(&output).map_err(|error| Error::from(error).uncertain())?;
        if bytes.len() > MAX_FRAME - 8192 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Bound result exceeds the broker frame budget",
            )
            .uncertain());
        }
        Ok(output)
    }
}

#[cfg(test)]
#[path = "provider_mounts_tests.rs"]
mod tests;
