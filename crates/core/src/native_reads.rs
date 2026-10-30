//! Native reads reuse owner Policy, the real private Project Graph boundary,
//! and the existing RefStore / ProviderLease / OperationGate lifetime.
//! No request actor, provider marker, or pathname can create this authority.
use super::*;
use crate::operation_permits::NativeReadHold;
use semwright_policy::NativeResourceReadPolicy;
use semwright_registry::Metadata;

fn denied() -> Error {
    Error::new(ErrorCode::PolicyDenied, "Native resource access denied")
}

pub(super) fn validate_provider_contract(
    policy: &Policy,
    identity: &ProviderIdentity,
    rows: &[(CommandDescriptor, Metadata)],
    previously_required: bool,
) -> Result<bool> {
    let classified = rows.iter().any(|(descriptor, _)| {
        descriptor
            .requires
            .iter()
            .any(|required| required == NATIVE_RESOURCE_READ_REQUIREMENT)
    });
    let protected =
        previously_required || classified || policy.native_read_provider(&identity.id).is_some();
    if !protected {
        return Ok(false);
    }
    // Owner policy keeps protection across removal/reconnection. An empty
    // entry is a valid default-deny policy; an absent entry cannot opt in by
    // trusting a provider marker, then later fall back to a legacy driver.
    if policy.native_read_provider(&identity.id).is_none() {
        return Err(denied());
    }
    let slug = identity.id.strip_prefix("driver:").ok_or_else(denied)?;
    if identity.kind != SourceKind::Driver
        || !rows.iter().any(|(descriptor, _)| {
            descriptor.name == format!("driver.{slug}.observe")
                && descriptor.risk == Risk::ReadOnly
                && descriptor
                    .requires
                    .iter()
                    .any(|required| required == NATIVE_RESOURCE_READ_REQUIREMENT)
        })
    {
        return Err(denied());
    }
    for (descriptor, _) in rows {
        let read_marker = descriptor
            .requires
            .iter()
            .any(|required| required == NATIVE_RESOURCE_READ_REQUIREMENT);
        if (read_marker && descriptor.risk != Risk::ReadOnly)
            || (descriptor.name == format!("driver.{slug}.operation.get") && !read_marker)
        {
            return Err(denied());
        }
    }
    Ok(true)
}

fn requested_resource<'a>(
    descriptor: &CommandDescriptor,
    provider_id: &str,
    args: &'a Value,
) -> Result<Option<&'a str>> {
    let slug = provider_id.strip_prefix("driver:").ok_or_else(denied)?;
    if descriptor.name == format!("driver.{slug}.observe") {
        return args
            .get("resource")
            .and_then(Value::as_str)
            .map(Some)
            .ok_or_else(denied);
    }
    if descriptor.name == format!("driver.{slug}.operation.get") {
        return args
            .pointer("/request/resource")
            .and_then(Value::as_str)
            .map(Some)
            .ok_or_else(denied);
    }
    if !descriptor.risk.mutates() {
        // A marked observed read selects its resource only through the genuine
        // RefStore entry, exactly as an observed mutation does. Payload resource
        // fields cannot grant authority; admission below requires native_read
        // and rechecks the policy, primary/secondary refs, and provider lifetime.
        let prefix = format!("driver.{slug}.");
        if descriptor.risk == Risk::ReadOnly
            && descriptor.name.starts_with(&prefix)
            && descriptor.name.len() > prefix.len()
            && descriptor.backends.len() == 1
            && descriptor.backends[0] == provider_id
            && descriptor
                .requires
                .iter()
                .any(|required| required == provider_id)
            && descriptor
                .requires
                .iter()
                .any(|required| required == NATIVE_RESOURCE_READ_REQUIREMENT)
            && descriptor.input_schema.get("type").and_then(Value::as_str) == Some("object")
            && descriptor
                .input_schema
                .get("required")
                .and_then(Value::as_array)
                .is_some_and(|keys| keys.iter().any(|key| key == "ref"))
            && descriptor
                .input_schema
                .pointer("/properties/ref/type")
                .and_then(Value::as_str)
                == Some("string")
            && args
                .get("ref")
                .and_then(Value::as_str)
                .is_some_and(|reference| !reference.is_empty())
        {
            return Ok(None);
        }
        // Unclassified read routes still have no canonical selector contract.
        return Err(denied());
    }
    // Ordinary observed mutations carry their project in the genuine ref.
    // A top-level `resource` may be payload (review.add / lock.set scene key).
    Ok(None)
}

fn mapping_for(broker: &Broker, origin: &NativeReadReference) -> Result<NativeResourceReadPolicy> {
    let mapping = broker
        .policy
        .native_read_mapping(&origin.binding.backend, &origin.resource)
        .ok_or_else(denied)?;
    if origin.grants != broker.policy.grant_fingerprint()
        || mapping.project != origin.project
        || mapping.root != origin.root
        || origin.binding.mount != origin.root
        || broker
            .policy
            .config()
            .deny
            .contains(NATIVE_RESOURCE_READ_REQUIREMENT)
    {
        return Err(denied());
    }
    Ok(mapping.clone())
}

fn check_origin(
    broker: &Broker,
    context: &Context,
    origin: &NativeReadReference,
    lease: &Arc<ProviderLease>,
    completion: bool,
) -> Result<()> {
    context.check_cancelled()?;
    if !lease.native_read_scope_required
        || origin.binding.backend != lease.identity.id
        || origin.binding.session != context.session
        || origin.binding.is_revoked()
        || origin.binding.connection != lease.connection
        || origin.binding.generation != lease.generation
    {
        return Err(denied());
    }
    let bound = BoundProvider {
        reference_id: origin.binding_reference.clone(),
        lease: lease.clone(),
        admission: origin.binding.clone(),
        parent_request_id: None,
    };
    let current = if completion {
        broker
            .check_bound_completion(&context.session, &bound, Some(lease))
            .map_err(|_| denied())?
    } else {
        broker
            .check_bound_provider(&context.session, &bound, Some(lease))
            .map_err(|_| denied())?;
        true
    };
    if !current {
        return Err(denied());
    }
    let mapping = mapping_for(broker, origin)?;
    {
        // This is the existing real GraphStore owner/principal/root check.
        // Never hold this lock across native validation or provider await.
        let mut graphs = broker.project_graphs.lock().map_err(|_| denied())?;
        graphs
            .as_mut()
            .ok_or_else(denied)?
            .authorize_native_read(&broker.policy, context, &mapping)
            .map_err(|_| denied())?;
    }
    context.check_cancelled()?;
    let current = if completion {
        broker
            .check_bound_completion(&context.session, &bound, Some(lease))
            .map_err(|_| denied())?
    } else {
        broker
            .check_bound_provider(&context.session, &bound, Some(lease))
            .map_err(|_| denied())?;
        true
    };
    if !current {
        return Err(denied());
    }
    Ok(())
}

/// Called immediately after RefStore lookup, before backend.validate can reach
/// any provider preflight. Both primary and secondary references use this path.
pub(super) fn validate_reference(
    broker: &Broker,
    session: &str,
    reference: &Reference,
) -> Result<()> {
    let lease = broker.provider(&reference.backend).ok_or_else(denied)?;
    if !lease.native_read_scope_required && reference.native_read.is_none() {
        return Ok(());
    }
    let origin = reference.native_read.as_ref().ok_or_else(denied)?;
    if reference.target.kind != "native"
        || reference.backend != origin.binding.backend
        || reference.session != session
    {
        return Err(denied());
    }
    let context = Context {
        session: session.into(),
        request_id: "native-read-reference-check".into(),
        cancellation: lease.epoch.child_token(),
    };
    check_origin(broker, &context, origin, &lease, false)
}

pub(super) struct NativeReadAdmission {
    origin: NativeReadReference,
    bound: BoundProvider,
    references: Vec<(String, Reference)>,
    hold: Option<NativeReadHold>,
}

impl NativeReadAdmission {
    pub(super) fn admit(
        broker: &Arc<Broker>,
        context: &Context,
        descriptor: &CommandDescriptor,
        args: &Value,
        provider: Option<&Arc<ProviderLease>>,
        bound: Option<&BoundProvider>,
        reference_pair: (Option<&Reference>, Option<&Reference>),
    ) -> Result<Option<Self>> {
        let (reference, secondary) = reference_pair;
        let marker = descriptor
            .requires
            .iter()
            .any(|required| required == NATIVE_RESOURCE_READ_REQUIREMENT);
        let Some(lease) = provider else {
            return if marker { Err(denied()) } else { Ok(None) };
        };
        if !lease.native_read_scope_required && !marker {
            return Ok(None);
        }
        let bound = bound.ok_or_else(denied)?;
        broker
            .check_bound_provider(&context.session, bound, Some(lease))
            .map_err(|_| denied())?;
        let requested = requested_resource(descriptor, &lease.identity.id, args)?;
        let resource = requested
            .or_else(|| {
                reference.and_then(|reference| {
                    reference
                        .native_read
                        .as_ref()
                        .map(|origin| origin.resource.as_str())
                })
            })
            .ok_or_else(denied)?;
        let mapping = broker
            .policy
            .native_read_mapping(&lease.identity.id, resource)
            .ok_or_else(denied)?;
        if mapping.root != bound.admission.mount {
            return Err(denied());
        }
        let origin = NativeReadReference {
            resource: resource.into(),
            project: mapping.project.clone(),
            root: mapping.root.clone(),
            grants: broker.policy.grant_fingerprint(),
            binding_reference: bound.reference_id.clone(),
            binding: bound.admission.clone(),
        };
        let mut references = Vec::new();
        for (field, reference) in [("ref", reference), ("target_ref", secondary)] {
            if let Some(reference) = reference {
                if reference.target.kind != "native"
                    || reference.backend != lease.identity.id
                    || reference.session != context.session
                    || reference
                        .native_read
                        .as_ref()
                        .is_none_or(|old| !old.same_scope(&origin))
                {
                    return Err(denied());
                }
                references.push((
                    args.get(field)
                        .and_then(Value::as_str)
                        .ok_or_else(denied)?
                        .into(),
                    reference.clone(),
                ));
            }
        }
        check_origin(broker, context, &origin, lease, false)?;
        let hold = if descriptor.risk == Risk::ReadOnly {
            Some(lease.operation_gate.hold_native_read(
                &context.session,
                &context.cancellation,
                &origin.root,
            )?)
        } else {
            // The real Host write permit owns mutation cancellation/drain. A
            // second hold here would change its original before-enter protocol.
            None
        };
        let admitted = Self {
            origin,
            bound: bound.clone(),
            references,
            hold,
        };
        // revoke_session cancels existing scopes, not future session IDs. Re-read
        // the ORIGINAL mount after Gate registration to close that race.
        admitted.check(broker, context, false)?;
        Ok(Some(admitted))
    }

    pub(super) fn origin(&self) -> &NativeReadReference {
        &self.origin
    }
    pub(super) fn closing(&self) -> CancellationToken {
        self.hold
            .as_ref()
            .map(NativeReadHold::closing)
            .unwrap_or_default()
    }

    pub(super) fn check(&self, broker: &Broker, context: &Context, completion: bool) -> Result<()> {
        if let Some(hold) = &self.hold {
            hold.check_current().map_err(|_| denied())?;
        }
        check_origin(broker, context, &self.origin, &self.bound.lease, completion)?;
        {
            let references = broker.references.lock().map_err(|_| denied())?;
            for (id, expected) in &self.references {
                let current = references.get(id, &context.session).map_err(|_| denied())?;
                let a = &current.target;
                let b = &expected.target;
                if current.backend != expected.backend
                    || current.session != expected.session
                    || current.expires != expected.expires
                    || a.kind != b.kind
                    || a.app != b.app
                    || a.identity != b.identity
                    || a.revision != b.revision
                    || a.fingerprint != b.fingerprint
                    || current
                        .native_read
                        .as_ref()
                        .is_none_or(|origin| !origin.same_scope(&self.origin))
                {
                    return Err(denied());
                }
            }
        }
        if let Some(hold) = &self.hold {
            hold.check_current().map_err(|_| denied())?;
        }
        context.check_cancelled()
    }
}

#[cfg(all(test, target_os = "linux"))]
#[derive(Default)]
pub(super) struct ReadCompletionPause {
    pub(super) entered: tokio::sync::Notify,
    pub(super) proceed: tokio::sync::Notify,
}
#[cfg(all(test, target_os = "linux"))]
impl Broker {
    pub(super) fn pause_next_native_read_completion(&self) -> Result<Arc<ReadCompletionPause>> {
        let mut slot = self
            .native_read_completion_pause
            .lock()
            .map_err(|_| denied())?;
        if slot.is_some() {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Native read completion pause is already set",
            ));
        }
        let pause = Arc::new(ReadCompletionPause::default());
        *slot = Some(pause.clone());
        Ok(pause)
    }
    pub(super) fn pause_next_native_bound_read_completion(
        &self,
    ) -> Result<Arc<ReadCompletionPause>> {
        let mut slot = self
            .native_bound_read_completion_pause
            .lock()
            .map_err(|_| denied())?;
        if slot.is_some() {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Native bound read completion pause is already set",
            ));
        }
        let pause = Arc::new(ReadCompletionPause::default());
        *slot = Some(pause.clone());
        Ok(pause)
    }
    pub(super) async fn native_bound_read_completion_checkpoint(&self) -> Result<()> {
        let pause = self
            .native_bound_read_completion_pause
            .lock()
            .map_err(|_| denied())?
            .take();
        if let Some(pause) = pause {
            pause.entered.notify_one();
            pause.proceed.notified().await;
        }
        Ok(())
    }
    pub(super) async fn native_read_completion_checkpoint(&self) -> Result<()> {
        let pause = self
            .native_read_completion_pause
            .lock()
            .map_err(|_| denied())?
            .take();
        if let Some(pause) = pause {
            pause.entered.notify_one();
            pause.proceed.notified().await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod resource_selector_tests {
    use super::*;
    use semwright_types::Idempotency;
    use serde_json::json;

    const PROVIDER: &str = "driver:motionwright";

    fn observed_read() -> CommandDescriptor {
        CommandDescriptor {
            name: "driver.motionwright.project.preview".into(),
            version: "1.0.0".into(),
            description: "Preview an observed project".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"ref": {"type": "string"}},
                "required": ["ref"]
            }),
            output_schema: json!({"type": "object"}),
            requires: vec![PROVIDER.into(), NATIVE_RESOURCE_READ_REQUIREMENT.into()],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 1_000,
            dry_run: false,
            interactive_consent: false,
            backends: vec![PROVIDER.into()],
        }
    }

    #[test]
    fn observed_read_never_selects_resource_from_payload() {
        let descriptor = observed_read();
        for args in [
            json!({"ref": "opaque-broker-ref"}),
            json!({"ref": "opaque-broker-ref", "resource": "project:foreign"}),
            json!({"ref": "opaque-broker-ref", "request": {"resource": "project:foreign"}}),
        ] {
            // None is a request for the genuine RefStore origin, not admission.
            assert_eq!(
                requested_resource(&descriptor, PROVIDER, &args).unwrap(),
                None
            );
        }
    }

    #[test]
    fn observed_read_requires_nonempty_request_ref() {
        let descriptor = observed_read();
        for args in [
            json!({"resource": "project:authorized"}),
            json!({"ref": ""}),
            json!({"ref": null}),
            json!({"ref": {"native_read": {"resource": "project:authorized"}}}),
        ] {
            assert!(requested_resource(&descriptor, PROVIDER, &args).is_err());
        }
    }

    #[test]
    fn observed_read_requires_explicit_string_ref_schema() {
        let args = json!({"ref": "opaque-broker-ref", "resource": "project:authorized"});
        for schema in [
            json!({"type": "object", "properties": {"ref": {"type": "string"}}}),
            json!({"type": "object", "required": ["ref"]}),
            json!({"type": "object", "required": ["ref"], "properties": {"ref": {"type": "object"}}}),
            json!({"required": ["ref"], "properties": {"ref": {"type": "string"}}}),
        ] {
            let mut descriptor = observed_read();
            descriptor.input_schema = schema;
            assert!(requested_resource(&descriptor, PROVIDER, &args).is_err());
        }
    }

    #[test]
    fn observed_read_requires_marker_and_canonical_provider_scope() {
        let args = json!({"ref": "opaque-broker-ref", "resource": "project:authorized"});
        let original = observed_read();
        let mut unmarked = original.clone();
        unmarked
            .requires
            .retain(|required| required != NATIVE_RESOURCE_READ_REQUIREMENT);
        let mut no_provider_requirement = original.clone();
        no_provider_requirement
            .requires
            .retain(|required| required != PROVIDER);
        let mut foreign_backend = original.clone();
        foreign_backend.backends = vec!["driver:foreign".into()];
        let mut multiple_backends = original.clone();
        multiple_backends.backends.push("driver:foreign".into());
        let mut foreign_name = original.clone();
        foreign_name.name = "driver.foreign.project.preview".into();
        for descriptor in [
            unmarked,
            no_provider_requirement,
            foreign_backend,
            multiple_backends,
            foreign_name,
        ] {
            assert!(requested_resource(&descriptor, PROVIDER, &args).is_err());
        }
        assert!(requested_resource(&original, "mcp:motionwright", &args).is_err());
    }

    #[test]
    fn canonical_reads_keep_their_explicit_resource_selectors() {
        let mut descriptor = observed_read();
        descriptor.name = "driver.motionwright.observe".into();
        let args =
            json!({"resource": "project:observed", "request": {"resource": "project:other"}});
        assert_eq!(
            requested_resource(&descriptor, PROVIDER, &args).unwrap(),
            Some("project:observed")
        );
        assert!(
            requested_resource(&descriptor, PROVIDER, &json!({"ref": "opaque-broker-ref"}))
                .is_err()
        );
        descriptor.name = "driver.motionwright.operation.get".into();
        assert_eq!(
            requested_resource(&descriptor, PROVIDER, &args).unwrap(),
            Some("project:other")
        );
        assert!(
            requested_resource(
                &descriptor,
                PROVIDER,
                &json!({"ref": "opaque-broker-ref", "resource": "project:observed"}),
            )
            .is_err()
        );
    }

    #[test]
    fn observed_mutation_keeps_the_genuine_reference_selector() {
        let mut descriptor = observed_read();
        descriptor.name = "driver.motionwright.lock.set".into();
        descriptor.risk = Risk::Mutating;
        descriptor.requires = vec![PROVIDER.into()];
        let args = json!({"ref": "opaque-broker-ref", "resource": "scene:payload"});
        assert_eq!(
            requested_resource(&descriptor, PROVIDER, &args).unwrap(),
            None
        );
    }
}
