//! Adapt cooperation to the canonical Driver Host protocol; never authorize a call here.
use crate::cooperation::{
    Application, CallContext, ObservationPage, Query, RequestIdentity, ResourceVersion,
    TargetRequirement, validate_value, version_fingerprint,
};
use crate::{
    Capability, CommandDescriptor, Driver, DriverExecutionContext, DriverInterfaces, Error,
    ErrorCode, Idempotency, NativeTarget, Result, Risk, Value, async_trait, descriptor_digest,
    json,
};
use std::collections::BTreeMap;

const MAX_OBSERVED_RESOURCES: usize = 1024;
struct Observed {
    target: NativeTarget,
    version: ResourceVersion,
    scope: String,
}
struct Compiled {
    capability: Capability,
    input: jsonschema::Validator,
    output: jsonschema::Validator,
}

/// A real canonical Driver implementation. NativeTarget's integer is a bounded
/// observation serial; the application revision is retained and compared in full.
pub struct NativeDriver {
    app: Application,
    catalog: BTreeMap<String, Compiled>,
    observed: BTreeMap<String, Observed>,
    instance: String,
    serial: u64,
}
impl NativeDriver {
    pub fn new(app: Application) -> Result<Self> {
        let identity = crate::types::ProviderIdentity::external(
            crate::types::SourceKind::Driver,
            app.id(),
            app.version(),
        )?;
        let mut capabilities = Vec::new();
        for registered in app.operations.values() {
            registered.contract.validate(app.id())?;
            if registered.contract.target == TargetRequirement::ObservedResource
                && !app.has_observer()
            {
                return Err(Error::invalid(
                    "Observed operation requires an observation provider",
                ));
            }
            capabilities.push(Capability {
                descriptor: registered.contract.descriptor.clone(),
                aliases: vec![],
                tags: vec![],
                object_types: vec![],
            });
        }
        if app.has_observer() {
            capabilities.push(read_capability(&app, "observe", "Observe one revision-bound page", query_schema(), json!({"type":"object","properties":{"page":{"type":"object"},"ref":{"type":["object","string"]}},"required":["page","ref"],"additionalProperties":false})));
        }
        if app.has_recovery() {
            let mut required = vec!["request"];
            if app.has_observer() {
                required.push("ref");
            }
            capabilities.push(read_capability(&app, "operation.get", "Read an exact historical result; never replay", json!({"type":"object","properties":{"request":{"type":"object","properties":{"resource":{"type":"string","minLength":1,"maxLength":512},"epoch":{"type":"integer","minimum":0,"maximum":9007199254740991u64},"key":{"type":"string","minLength":1,"maxLength":128},"request_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"}},"required":["resource","epoch","key","request_sha256"],"additionalProperties":false},"ref":{"type":"string"}},"required":required,"additionalProperties":false}), json!({"type":"object","properties":{"record":{"type":"object"},"historical_only":{"const":true},"current_authority":{"const":false},"replay_allowed":{"const":false}},"required":["record","historical_only","current_authority","replay_allowed"],"additionalProperties":false})));
        }
        if capabilities.is_empty() {
            return Err(Error::invalid(
                "Application exposes no cooperation interface",
            ));
        }
        let mut catalog = BTreeMap::new();
        for capability in capabilities {
            capability.validate_for(&identity)?;
            let input = jsonschema::validator_for(&capability.descriptor.input_schema)
                .map_err(|_| Error::invalid("Invalid operation input schema"))?;
            let output = jsonschema::validator_for(&capability.descriptor.output_schema)
                .map_err(|_| Error::invalid("Invalid operation output schema"))?;
            catalog.insert(
                capability.descriptor.name.clone(),
                Compiled {
                    capability,
                    input,
                    output,
                },
            );
        }
        Ok(Self {
            app,
            catalog,
            observed: BTreeMap::new(),
            instance: crate::types::unique_id(),
            serial: 0,
        })
    }

    fn remember(&mut self, page: &ObservationPage) -> Result<NativeTarget> {
        if let Some(old) = self
            .observed
            .values()
            .find(|old| old.version == page.version && old.scope == page.scope)
        {
            return Ok(old.target.clone());
        }
        // Old versions of this resource are stale already. Do not accumulate them forever.
        self.observed.retain(|_, old| {
            old.version.resource != page.version.resource || old.version == page.version
        });
        if self.observed.len() >= MAX_OBSERVED_RESOURCES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Observed resource budget exhausted; start a new driver session",
            ));
        }
        self.serial = self.serial.checked_add(1).ok_or_else(|| {
            Error::new(ErrorCode::ResourceExhausted, "Observation serial exhausted")
        })?;
        let target = NativeTarget {
            kind: "native".into(),
            identity: format!("{}:{}", self.instance, self.serial),
            revision: self.serial,
            fingerprint: version_fingerprint(&page.version)?,
            app: self.app.id().into(),
        };
        self.observed.insert(
            target.identity.clone(),
            Observed {
                target: target.clone(),
                version: page.version.clone(),
                scope: page.scope.clone(),
            },
        );
        Ok(target)
    }

    fn cached(&self, target: &NativeTarget) -> Result<&Observed> {
        let old = self.observed.get(&target.identity).ok_or_else(|| {
            Error::new(
                ErrorCode::StaleReference,
                "Reference was not observed in this driver instance",
            )
        })?;
        let expected = &old.target;
        if target.kind != expected.kind
            || target.app != expected.app
            || target.revision != expected.revision
            || target.fingerprint != expected.fingerprint
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Reference binding differs from the observation",
            ));
        }
        Ok(old)
    }

    async fn fresh(&self, target: &NativeTarget, context: &CallContext) -> Result<ResourceVersion> {
        let old = self.cached(target)?;
        let query = Query {
            resource: old.version.resource.clone(),
            scope: old.scope.clone(),
            limit: 1,
            cursor: None,
        };
        let observer = self
            .app
            .observer
            .as_ref()
            .ok_or_else(|| Error::new(ErrorCode::Unsupported, "Observation is unavailable"))?;
        let page = observer.observe(&query, context).await?;
        page.validate_for(&query)?;
        if page.version != old.version {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Application changed since the observation",
            ));
        }
        Ok(page.version)
    }

    fn reject_unregistered_markers(&self, value: &Value) -> Result<()> {
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            match value {
                Value::Object(map) => {
                    if let Some(marker) = map.get("$ref").filter(|v| v.is_object()) {
                        let target: NativeTarget = serde_json::from_value(marker.clone())
                            .map_err(|_| Error::invalid("Malformed native reference marker"))?;
                        self.cached(&target)?;
                    }
                    pending.extend(map.values());
                }
                Value::Array(values) => pending.extend(values),
                _ => {}
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Driver for NativeDriver {
    fn id(&self) -> &str {
        self.app.id()
    }
    fn version(&self) -> &str {
        self.app.version()
    }
    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            health: true,
            native_refs: self.app.has_observer(),
            cooperative_cancellation: true,
            host_tools: self.app.host_tools_required(),
            ..DriverInterfaces::default()
        }
    }
    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        Ok(self
            .catalog
            .values()
            .map(|entry| entry.capability.clone())
            .collect())
    }
    async fn execute(&mut self, _: &str, _: &str, _: Value) -> Result<Value> {
        Err(Error::new(
            ErrorCode::PermissionDenied,
            "Native operations require canonical Driver Host execution context",
        ))
    }
    async fn validate_native_ref(&mut self, target: &NativeTarget) -> Result<()> {
        // Driver Host lifecycle validation has no execution authority by design.
        // Prove only that this exact opaque target was emitted by this live driver
        // instance. Fresh application state is checked again in execute_with_context,
        // where the Host-mediated observer has the grants needed for a real read.
        self.cached(target).map(|_| ())
    }
    async fn execute_with_context(
        &mut self,
        command: &str,
        digest: &str,
        args: Value,
        host: DriverExecutionContext,
    ) -> Result<Value> {
        host.check_cancelled()?;
        validate_value(&args)?;
        let registered = self
            .catalog
            .get(command)
            .ok_or_else(|| Error::new(ErrorCode::PermissionDenied, "Operation is not exposed"))?;
        if descriptor_digest(&registered.capability.descriptor)? != digest {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Operation descriptor changed",
            ));
        }
        registered
            .input
            .validate(&args)
            .map_err(|_| Error::invalid("Operation arguments do not match its schema"))?;
        let target = host.native_target().cloned();
        if args.get("ref").is_some() && target.is_none() {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Caller reference has no Host binding",
            ));
        }
        let mut context = CallContext::from_host(host, None);
        let expected = if let Some(target) = &target {
            Some(self.fresh(target, &context).await?)
        } else {
            None
        };
        context.set_expected(expected.clone());
        let mut mutates = false;
        let result = if command == format!("driver.{}.observe", self.app.id()) {
            let mut query_args = args.clone();
            query_args
                .as_object_mut()
                .expect("validated object")
                .remove("ref");
            let query: Query = serde_json::from_value(query_args)?;
            query.validate()?;
            if expected
                .as_ref()
                .is_some_and(|version| version.resource != query.resource)
            {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "Reference and query resource differ",
                ));
            }
            let page = self
                .app
                .observer
                .as_ref()
                .expect("catalog observer")
                .observe(&query, &context)
                .await?;
            page.validate_for(&query)?;
            let target = self.remember(&page)?;
            json!({"page": page, "ref": crate::types::target_marker(target)})
        } else if command == format!("driver.{}.operation.get", self.app.id()) {
            let identity: RequestIdentity = serde_json::from_value(args["request"].clone())?;
            identity.validate()?;
            if self.app.has_observer()
                && expected
                    .as_ref()
                    .is_none_or(|version| version.resource != identity.resource)
            {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "Recovery requires a current resource observation",
                ));
            }
            let record = self
                .app
                .recovery
                .as_ref()
                .expect("catalog recovery")
                .lookup(&identity, &context)
                .await?;
            record.validate_for(&identity)?;
            json!({"record":record,"historical_only":true,"current_authority":false,"replay_allowed":false})
        } else {
            let operation = self.app.operations.get(command).expect("catalog operation");
            if operation.contract.target == TargetRequirement::ObservedResource
                && expected.is_none()
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Operation requires an observed resource",
                ));
            }
            context.check_cancelled()?;
            mutates = operation.contract.descriptor.risk.mutates();
            operation
                .handler
                .invoke(args, &context)
                .await
                .into_result()?
        };
        // A malformed result after an app commit is uncertain, not a known rejection.
        let validate_result = || -> Result<()> {
            validate_value(&result)?;
            self.reject_unregistered_markers(&result)?;
            self.catalog[command].output.validate(&result).map_err(|_| {
                Error::new(
                    ErrorCode::PluginProtocolError,
                    "Application result violates its declared schema",
                )
            })
        };
        validate_result().map_err(|error| if mutates { error.uncertain() } else { error })?;
        Ok(result)
    }
}

fn read_capability(
    app: &Application,
    suffix: &str,
    description: &str,
    input: Value,
    output: Value,
) -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: format!("driver.{}.{suffix}", app.id()),
            version: app.version().into(),
            description: description.into(),
            input_schema: input,
            output_schema: output,
            requires: vec![format!("driver:{}", app.id())],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 30_000,
            dry_run: false,
            interactive_consent: false,
            backends: vec![format!("driver:{}", app.id())],
        },
        aliases: vec![],
        tags: vec![],
        object_types: vec![],
    }
}
fn query_schema() -> Value {
    json!({"type":"object","properties":{"resource":{"type":"string","minLength":1,"maxLength":512},"scope":{"type":"string","minLength":1,"maxLength":128},"limit":{"type":"integer","minimum":1,"maximum":256},"cursor":{"type":["object","null"]},"ref":{"type":"string"}},"required":["resource","scope","limit"],"additionalProperties":false})
}

#[cfg(test)]
mod native_reference_lifecycle_tests {
    use super::*;
    use crate::cooperation::{ObservationProvider, RevisionToken};
    use std::sync::Arc;

    struct LifecycleMustNotObserve;

    #[async_trait]
    impl ObservationProvider for LifecycleMustNotObserve {
        async fn observe(&self, _: &Query, _: &CallContext) -> Result<ObservationPage> {
            panic!("native-reference lifecycle validation must not acquire observation authority")
        }
    }

    #[tokio::test]
    async fn lifecycle_validation_is_cache_only_and_rejects_tampering() {
        let app = Application::new("lifecycle-app", "1.0.0")
            .unwrap()
            .require_host_tools()
            .with_observer(Arc::new(LifecycleMustNotObserve));
        let mut driver = NativeDriver::new(app).unwrap();
        let page = ObservationPage {
            version: ResourceVersion {
                resource: "inventory".into(),
                generation: "generation-a".into(),
                revision: RevisionToken::new("revision-a").unwrap(),
            },
            scope: "stock".into(),
            items: vec![json!({"sku":"alpha"})],
            next: None,
            complete: true,
        };
        let target = driver.remember(&page).unwrap();

        driver.validate_native_ref(&target).await.unwrap();

        let mut tampered = target.clone();
        tampered.revision += 1;
        let error = driver.validate_native_ref(&tampered).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleReference);
    }
}
