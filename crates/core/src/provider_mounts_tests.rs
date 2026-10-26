//! Broker integration with test-only platform capsules. This does not attest a real sandbox.
use super::*;
use semwright_backend_api::{ProvidedCapability, Provider, ProviderInterfaces, feature};
use semwright_platform_api::launch::Mount;
use semwright_policy::{FilesystemGrant, PolicyConfig};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::Notify;

struct TestRoot {
    mount: Mount,
    valid: AtomicBool,
}
impl WorkspaceMountBinding for TestRoot {
    fn mount(&self) -> &Mount {
        &self.mount
    }
    fn validate_current(&self) -> Result<()> {
        if self.valid.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(Error::new(ErrorCode::StaleReference, "test root replaced"))
        }
    }
}
struct TestProvider {
    identity: ProviderIdentity,
    descriptor: CommandDescriptor,
    calls: AtomicUsize,
    blocked: AtomicBool,
    entered: Notify,
    release: Notify,
    closed: CancellationToken,
}
#[async_trait]
impl Provider for TestProvider {
    fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }
    fn supports(&self, name: &str) -> bool {
        name == self.descriptor.name
    }
    async fn capabilities(&self) -> Result<Vec<ProvidedCapability>> {
        Ok(vec![self.descriptor.clone().into()])
    }
    async fn probe(&self) -> Vec<Feature> {
        vec![feature(
            &self.identity.id,
            &self.descriptor.name,
            true,
            "test",
            "test",
        )]
    }
    fn interfaces(&self) -> ProviderInterfaces {
        ProviderInterfaces {
            dynamic_capabilities: true,
            cooperative_cancellation: true,
            ..Default::default()
        }
    }
    fn closed(&self) -> Option<CancellationToken> {
        Some(self.closed.clone())
    }
    async fn execute(&self, context: &Context, _: &CommandDescriptor, _: &Value) -> Result<Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        if self.blocked.load(Ordering::SeqCst) {
            tokio::select! {
                _=context.cancellation.cancelled()=>return Err(Error::new(ErrorCode::Cancelled,"test cancelled")),
                _=self.release.notified()=>(),
            }
        }
        Ok(json!({"done":true}))
    }
}
struct TestApprover {
    accept: bool,
    gated: bool,
    calls: AtomicUsize,
    entered: Notify,
    release: Notify,
}
impl TestApprover {
    fn new(accept: bool, gated: bool) -> Arc<Self> {
        Arc::new(Self {
            accept,
            gated,
            calls: AtomicUsize::new(0),
            entered: Notify::new(),
            release: Notify::new(),
        })
    }
}
#[async_trait]
impl Approver for TestApprover {
    async fn approve(&self, _: Approval, cancel: CancellationToken) -> Result<bool> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        if self.gated {
            tokio::select! {
                _=cancel.cancelled()=>return Err(Error::new(ErrorCode::Cancelled,"test approval cancelled")),
                _=self.release.notified()=>(),
            }
        }
        Ok(self.accept)
    }
}
struct Fixture {
    broker: Arc<Broker>,
    provider: Arc<TestProvider>,
    root: Arc<TestRoot>,
    audit: Arc<audit::Audit>,
    _dir: tempfile::TempDir,
}
impl Fixture {
    async fn new(configure: impl FnOnce(&mut PolicyConfig), approver: Arc<dyn Approver>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let identity = ProviderIdentity::external(SourceKind::Driver, "mount-test", "1").unwrap();
        let descriptor = CommandDescriptor {
            name: format!("{}apply", identity.namespace),
            version: "1".into(),
            description: "Test effect".into(),
            input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
            output_schema: json!({"type":"object","properties":{"done":{"type":"boolean"}},"required":["done"],"additionalProperties":false}),
            requires: vec![identity.id.clone()],
            risk: Risk::MutatingReversible,
            idempotency: Idempotency::NonIdempotent,
            timeout_ms: 5000,
            dry_run: true,
            interactive_consent: false,
            backends: vec![identity.id.clone()],
        };
        let provider = Arc::new(TestProvider {
            identity,
            descriptor,
            calls: AtomicUsize::new(0),
            blocked: AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
            closed: CancellationToken::new(),
        });
        let root = Arc::new(TestRoot {
            mount: Mount {
                source: dir.path().to_path_buf(),
                class: MountClass::Workspace,
                logical_name: "manim-runtime".into(),
                read_only: true,
                execute: false,
            },
            valid: AtomicBool::new(true),
        });
        let mut config = PolicyConfig {
            allow: [
                provider.identity.id.clone(),
                "providers.execute_bound".into(),
            ]
            .into(),
            filesystem: vec![FilesystemGrant {
                name: "manim-runtime".into(),
                path: dir.path().into(),
                read: true,
                write: false,
            }],
            ..Default::default()
        };
        configure(&mut config);
        let audit = audit::Audit::open(&dir.path().join("audit"), 65536, 2).unwrap();
        let broker = Broker::new(
            Policy::new(config).unwrap(),
            vec![],
            audit.clone(),
            approver,
            None,
            json!({}),
            false,
        )
        .unwrap();
        broker
            .mount_provider_with_workspace(provider.clone(), Ok(vec![root.clone()]))
            .await
            .unwrap();
        Self {
            broker,
            provider,
            root,
            audit,
            _dir: dir,
        }
    }
    async fn execute(&self, session: &str, command: &str, args: Value, dry_run: bool) -> Envelope {
        self.broker
            .clone()
            .execute(
                session.into(),
                unique_id(),
                ExecuteRequest {
                    command: command.into(),
                    args,
                    dry_run,
                    backend: None,
                },
                CancellationToken::new(),
            )
            .await
    }
    async fn issue(&self) -> String {
        let result = self
            .execute(
                "s",
                "providers.mount.observe",
                json!({
                    "provider":self.provider.identity.id,"root":"manim-runtime",
                }),
                false,
            )
            .await;
        assert!(result.ok, "{:?}", result.error);
        result.data.unwrap()["binding_ref"].as_str().unwrap().into()
    }
    fn bound(&self, binding: &str, dry_run: bool) -> ExecuteRequest {
        ExecuteRequest {
            command: "providers.execute_bound".into(),
            args: json!({
                "binding_ref":binding,"request":{"command":self.provider.descriptor.name,"args":{},"dry_run":dry_run}
            }),
            dry_run,
            backend: None,
        }
    }
    async fn call(&self, session: &str, request: ExecuteRequest) -> Envelope {
        self.broker
            .clone()
            .execute(
                session.into(),
                unique_id(),
                request,
                CancellationToken::new(),
            )
            .await
    }
}
#[tokio::test]
async fn live_binding_preserves_original_provenance_and_canonical_audit_parent() {
    let f = Fixture::new(|_| {}, Arc::new(NoApprover)).await;
    let token = f.issue().await;
    let result = tokio::time::timeout(Duration::from_secs(5), f.call("s", f.bound(&token, false)))
        .await
        .unwrap();
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.data.as_ref().unwrap()["admission_current"], true);
    assert_eq!(result.data.as_ref().unwrap()["admission_state"], "current");
    let inner: &Value = &result.data.as_ref().unwrap()["result"];
    assert_eq!(inner["ok"], true);
    assert_eq!(inner["command"], f.provider.descriptor.name);
    assert_eq!(
        inner["execution"]["provenance"]["provider"],
        f.provider.identity.id
    );
    let id = inner["request_id"].as_str().unwrap();
    let records = f.audit.tail(32).unwrap();
    let row = records
        .iter()
        .find(|r| r.request_id == id && r.phase == "finish")
        .unwrap();
    assert_eq!(
        row.parent_request_id.as_deref(),
        Some(result.request_id.as_str())
    );
    assert_eq!(
        row.provenance.as_ref().unwrap().provider,
        f.provider.identity.id
    );
}
#[tokio::test]
async fn explicit_wrapper_and_nested_policy_denials_prevent_dispatch() {
    for wrapper in [true, false] {
        let f = Fixture::new(
            |c| {
                c.deny.insert(if wrapper {
                    "providers.execute_bound".into()
                } else {
                    c.allow
                        .iter()
                        .find(|v| v.starts_with("driver:"))
                        .unwrap()
                        .clone()
                });
            },
            Arc::new(NoApprover),
        )
        .await;
        let token = f.issue().await;
        let result = f.call("s", f.bound(&token, false)).await;
        assert!(!result.ok);
        assert_eq!(result.error.unwrap().code, ErrorCode::PolicyDenied);
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
    }
    let f = Fixture::new(
        |c| {
            c.deny.insert("filesystem.read".into());
        },
        Arc::new(NoApprover),
    )
    .await;
    let denied = f
        .execute(
            "s",
            "providers.mount.observe",
            json!({"provider":f.provider.identity.id,"root":"manim-runtime"}),
            false,
        )
        .await;
    assert_eq!(denied.error.unwrap().code, ErrorCode::PolicyDenied);
}
#[tokio::test]
async fn unobservable_mount_does_not_disable_original_provider() {
    let f = Fixture::new(
        |c| {
            c.filesystem[0].read = false;
            c.filesystem[0].write = true;
        },
        Arc::new(NoApprover),
    )
    .await;
    let direct = f
        .execute("s", &f.provider.descriptor.name, json!({}), false)
        .await;
    assert!(direct.ok, "{:?}", direct.error);
    let observed = f
        .execute(
            "s",
            "providers.mount.observe",
            json!({"provider":f.provider.identity.id,"root":"manim-runtime"}),
            false,
        )
        .await;
    assert!(!observed.ok);
}
#[tokio::test]
async fn revoked_cross_session_forged_and_replaced_root_bindings_never_dispatch() {
    let f = Fixture::new(|_| {}, Arc::new(NoApprover)).await;
    let token = f.issue().await;
    assert!(!f.call("other", f.bound(&token, false)).await.ok);
    assert!(
        !f.call(
            "s",
            f.bound("provider-mount:00000000000000000000000000000000", false)
        )
        .await
        .ok
    );
    let native = f
        .broker
        .references
        .lock()
        .unwrap()
        .insert(
            "s",
            &f.provider.identity.id,
            NativeTarget {
                kind: "native".into(),
                identity: "forged".into(),
                revision: 1,
                fingerprint: "forged".into(),
                app: "fixture".into(),
            },
        )
        .unwrap();
    assert!(!f.call("s", f.bound(&native, false)).await.ok);
    f.root.valid.store(false, Ordering::SeqCst);
    let replaced = f.call("s", f.bound(&token, false)).await;
    assert!(replaced.error.unwrap().outcome_known);
    f.root.valid.store(true, Ordering::SeqCst);
    f.broker.revoke_session("s");
    assert!(!f.call("s", f.bound(&token, false)).await.ok);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn core_recursion_route_and_dry_run_substitution_never_dispatch() {
    let f = Fixture::new(|_| {}, Arc::new(NoApprover)).await;
    let token = f.issue().await;
    for command in ["doctor", "jobs.start", "providers.execute_bound"] {
        let mut request = f.bound(&token, false);
        request.args["request"]["command"] = json!(command);
        assert!(!f.call("s", request).await.ok);
    }
    let mut route = f.bound(&token, false);
    route.args["request"]["backend"] = json!("driver:other");
    assert!(!f.call("s", route).await.ok);
    let mut dry = f.bound(&token, false);
    dry.dry_run = true;
    assert!(!f.call("s", dry).await.ok);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn ordinary_refresh_invalidates_binding_but_preserves_unbound_epoch() {
    let f = Fixture::new(|_| {}, Arc::new(NoApprover)).await;
    let token = f.issue().await;
    let old = f.broker.provider(&f.provider.identity.id).unwrap();
    f.broker
        .refresh_provider(&f.provider.identity.id)
        .await
        .unwrap();
    assert!(old.active());
    assert!(old.binding_epoch.is_cancelled());
    assert!(!Arc::ptr_eq(
        &old,
        &f.broker.provider(&f.provider.identity.id).unwrap()
    ));
    let stale = f.call("s", f.bound(&token, false)).await;
    assert!(stale.error.unwrap().outcome_known);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
    let fresh = f.issue().await;
    assert!(f.call("s", f.bound(&fresh, false)).await.ok);
}
#[tokio::test]
async fn refresh_during_effect_is_unknown_once_with_original_audit_custody() {
    let f = Fixture::new(|_| {}, Arc::new(NoApprover)).await;
    let token = f.issue().await;
    f.provider.blocked.store(true, Ordering::SeqCst);
    let request = f.bound(&token, false);
    let outer_id = unique_id();
    let task = tokio::spawn(f.broker.clone().execute(
        "s".into(),
        outer_id.clone(),
        request,
        CancellationToken::new(),
    ));
    tokio::time::timeout(Duration::from_secs(5), f.provider.entered.notified())
        .await
        .unwrap();
    f.broker
        .refresh_provider(&f.provider.identity.id)
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert!(!result.ok);
    assert!(!result.error.unwrap().outcome_known);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
    let records = f.audit.tail(32).unwrap();
    let inner = records
        .iter()
        .find(|r| r.command == f.provider.descriptor.name && r.phase == "finish")
        .unwrap();
    assert_eq!(inner.parent_request_id.as_deref(), Some(outer_id.as_str()));
    assert!(!inner.outcome_known);
    assert_eq!(
        inner.provenance.as_ref().unwrap().provider,
        f.provider.identity.id
    );
    assert_eq!(
        inner
            .provenance
            .as_ref()
            .unwrap()
            .execution_provider
            .as_deref(),
        Some(f.provider.identity.id.as_str())
    );
}
#[tokio::test]
async fn decline_and_revocation_during_one_real_prompt_prevent_dispatch() {
    let decline = TestApprover::new(false, false);
    let f = Fixture::new(|c| c.confirm_mutations = true, decline.clone()).await;
    let token = f.issue().await;
    assert!(!f.call("s", f.bound(&token, false)).await.ok);
    assert_eq!(decline.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
    let pause = TestApprover::new(true, true);
    let f = Fixture::new(|c| c.confirm_mutations = true, pause.clone()).await;
    let token = f.issue().await;
    let task = tokio::spawn(f.broker.clone().execute(
        "s".into(),
        unique_id(),
        f.bound(&token, false),
        CancellationToken::new(),
    ));
    tokio::time::timeout(Duration::from_secs(5), pause.entered.notified())
        .await
        .unwrap();
    f.broker.revoke_session("s");
    pause.release.notify_one();
    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert!(!result.ok);
    assert!(result.error.unwrap().outcome_known);
    assert_eq!(pause.calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn dry_run_does_not_claim_confirmation_or_dispatch() {
    let approver = TestApprover::new(true, false);
    let f = Fixture::new(|c| c.confirm_mutations = true, approver.clone()).await;
    let token = f.issue().await;
    let result = f.call("s", f.bound(&token, true)).await;
    assert!(result.ok, "{:?}", result.error);
    assert_ne!(result.execution.policy_decision, "allow_after_confirmation");
    assert!(
        result.data.unwrap()["result"]["execution"]["dry_run"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(approver.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn expiry_alone_preserves_known_success_but_revocation_remains_unknown() {
    for revoke_after_expiry in [false, true] {
        let f = Fixture::new(|_| {}, Arc::new(NoApprover)).await;
        *f.broker.references.lock().unwrap() = RefStore::new(Duration::from_secs(2), 4);
        let token = f.issue().await;
        let deadline = f
            .broker
            .references
            .lock()
            .unwrap()
            .get_provider_mount(&token, "s")
            .unwrap()
            .expires;
        f.provider.blocked.store(true, Ordering::SeqCst);
        let outer_id = unique_id();
        let task = tokio::spawn(f.broker.clone().execute(
            "s".into(),
            outer_id.clone(),
            f.bound(&token, false),
            CancellationToken::new(),
        ));
        tokio::time::timeout(Duration::from_secs(5), f.provider.entered.notified())
            .await
            .unwrap();
        tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
        assert!(Instant::now() >= deadline);
        {
            let mut store = f.broker.references.lock().unwrap();
            assert!(store.get_provider_mount(&token, "s").is_err());
            store
                .insert(
                    "other",
                    &f.provider.identity.id,
                    NativeTarget {
                        kind: "native".into(),
                        identity: "prune-probe".into(),
                        revision: 1,
                        fingerprint: "probe".into(),
                        app: "fixture".into(),
                    },
                )
                .unwrap();
            assert!(store.get_provider_mount_for_completion(&token, "s").is_ok());
        }
        if revoke_after_expiry {
            f.broker.revoke_session("s");
        }
        f.provider.release.notify_one();
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
        let records = f.audit.tail(32).unwrap();
        let inner = records
            .iter()
            .find(|r| r.command == f.provider.descriptor.name && r.phase == "finish")
            .unwrap();
        assert_eq!(inner.parent_request_id.as_deref(), Some(outer_id.as_str()));
        assert_eq!(
            inner.provenance.as_ref().unwrap().provider,
            f.provider.identity.id
        );
        if revoke_after_expiry {
            assert!(!result.ok);
            assert!(!result.error.as_ref().unwrap().outcome_known);
            assert!(!inner.outcome_known);
        } else {
            assert!(result.ok, "{:?}", result.error);
            assert!(result.error.is_none());
            let data = result.data.as_ref().unwrap();
            assert_eq!(data["admission_current"], false);
            assert_eq!(data["admission_state"], "expired");
            assert_eq!(data["result"]["ok"], true);
            assert_eq!(data["result"]["request_id"], inner.request_id);
            assert_eq!(
                data["result"]["execution"]["provenance"]["provider"],
                f.provider.identity.id
            );
            assert!(inner.outcome_known);
            assert_eq!(inner.ok, Some(true));
        }
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn bound_execution_invalidates_recording_without_losing_original_audit() {
    for record_before_dispatch in [true, false] {
        let f = Fixture::new(|_| {}, Arc::new(NoApprover)).await;
        let mut trace_id = None;
        if record_before_dispatch {
            let started = f
                .broker
                .workflow_record_start(
                    "s",
                    "bound-operation-recording",
                    "Capture a provider effect without losing its authority",
                    true,
                )
                .unwrap();
            trace_id = Some(started["trace_id"].as_str().unwrap().to_owned());
        }
        let token = f.issue().await;
        f.provider.blocked.store(true, Ordering::SeqCst);
        let outer_id = unique_id();
        let task = tokio::spawn(f.broker.clone().execute(
            "s".into(),
            outer_id.clone(),
            f.bound(&token, false),
            CancellationToken::new(),
        ));
        tokio::time::timeout(Duration::from_secs(5), f.provider.entered.notified())
            .await
            .unwrap();
        if !record_before_dispatch {
            let started = f
                .broker
                .workflow_record_start(
                    "s",
                    "recording-during-bound-operation",
                    "Do not learn an in-flight effect as an unbound command",
                    true,
                )
                .unwrap();
            trace_id = Some(started["trace_id"].as_str().unwrap().to_owned());
        }
        f.provider.release.notify_one();
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
        let child_id = result.data.as_ref().unwrap()["result"]["request_id"]
            .as_str()
            .unwrap();
        let records = f.audit.tail(32).unwrap();
        let outer = records
            .iter()
            .find(|row| row.phase == "finish" && row.request_id == outer_id)
            .unwrap();
        let child = records
            .iter()
            .find(|row| row.phase == "finish" && row.request_id == child_id)
            .unwrap();
        assert_eq!(outer.command, "providers.execute_bound");
        assert_eq!(outer.ok, Some(true));
        assert!(outer.outcome_known);
        assert_eq!(child.command, f.provider.descriptor.name);
        assert_eq!(child.parent_request_id.as_deref(), Some(outer_id.as_str()));
        assert_eq!(child.ok, Some(true));
        assert!(child.outcome_known);
        assert_eq!(
            child.provenance.as_ref().unwrap().provider,
            f.provider.identity.id
        );
        assert_eq!(
            records
                .iter()
                .filter(|row| {
                    row.phase == "finish"
                        && (row.request_id == outer_id || row.request_id == child_id)
                })
                .count(),
            2
        );
        let stopped = f.broker.workflow_record_stop("s", true);
        assert_eq!(stopped.unwrap_err().code, ErrorCode::Conflict);
        let trace_id = trace_id.unwrap();
        assert!(f.broker.workflow_trace(&trace_id).is_err());
        assert!(
            f.broker.workflow_traces().unwrap()["traces"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn ordinary_unbound_execution_still_records_one_original_step() {
    let f = Fixture::new(|_| {}, Arc::new(NoApprover)).await;
    let started = f
        .broker
        .workflow_record_start(
            "s",
            "ordinary-provider-operation",
            "Preserve ordinary workflow recording",
            true,
        )
        .unwrap();
    let trace_id = started["trace_id"].as_str().unwrap();
    let result = f
        .execute("s", &f.provider.descriptor.name, json!({}), false)
        .await;
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 1);
    let stopped = f.broker.workflow_record_stop("s", true).unwrap();
    assert_eq!(stopped["trace"]["id"], trace_id);
    let steps = stopped["trace"]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0]["command"], f.provider.descriptor.name);
    assert_eq!(steps[0]["ok"], true);
    assert_eq!(steps[0]["outcome_known"], true);
    assert_eq!(
        f.broker.workflow_trace(trace_id).unwrap()["trace"]["id"],
        trace_id
    );
}

// Real Broker/Core issuer with test-only Provider/capsule, no physical Host/SQLite claim.
mod owner_operation_issuer {
    use super::*;
    use semwright_backend_api::provider::OwnerOperationAuthority;
    use std::sync::Mutex as StdMutex;
    struct PermitProvider {
        identity: ProviderIdentity,
        observe: CommandDescriptor,
        apply: CommandDescriptor,
        target: NativeTarget,
        protocol: u32,
        ordinary_calls: AtomicUsize,
        owner_dispatches: AtomicUsize,
        guarded_actions: AtomicUsize,
        before_acquire: AtomicBool,
        after_acquire: AtomicBool,
        dispatched: Notify,
        acquired: Notify,
        proceed: Notify,
        last_error: StdMutex<Option<Error>>,
        last_mount: StdMutex<Option<String>>,
        pause_shutdown: AtomicBool,
        shutdown_entered: Notify,
        shutdown_proceed: Notify,
    }
    impl PermitProvider {
        async fn pause(&self, context: &Context) -> Result<()> {
            tokio::select! {
                biased;
                _ = context.cancellation.cancelled() => Err(Error::new(
                    ErrorCode::Cancelled, "issuer fixture cancelled")),
                _ = self.proceed.notified() => Ok(()),
                _ = tokio::time::sleep(Duration::from_secs(5)) => Err(Error::new(
                    ErrorCode::Timeout, "issuer fixture control expired")),
            }
        }
        fn rejected(&self, error: Error) -> Error {
            *self.last_error.lock().unwrap() = Some(error.clone());
            error
        }
    }
    #[async_trait]
    impl Provider for PermitProvider {
        fn identity(&self) -> &ProviderIdentity {
            &self.identity
        }
        fn supports(&self, name: &str) -> bool {
            name == self.observe.name || name == self.apply.name
        }
        async fn capabilities(&self) -> Result<Vec<ProvidedCapability>> {
            Ok(vec![self.observe.clone().into(), self.apply.clone().into()])
        }
        async fn probe(&self) -> Vec<Feature> {
            [&self.observe, &self.apply]
                .iter()
                .map(|descriptor| {
                    feature(&self.identity.id, &descriptor.name, true, "test", "test")
                })
                .collect()
        }
        fn interfaces(&self) -> ProviderInterfaces {
            ProviderInterfaces {
                cooperative_cancellation: true,
                native_refs: true,
                ..Default::default()
            }
        }
        fn supports_owner_operation_permits(&self) -> bool {
            self.protocol >= 9
        }
        async fn shutdown(&self) -> Result<()> {
            if self.pause_shutdown.load(Ordering::SeqCst) {
                self.shutdown_entered.notify_one();
                tokio::time::timeout(Duration::from_secs(5), self.shutdown_proceed.notified())
                    .await
                    .map_err(|_| {
                        Error::new(
                            ErrorCode::Timeout,
                            "issuer fixture shutdown control expired",
                        )
                    })?;
            }
            Ok(())
        }
        async fn validate(&self, target: &NativeTarget) -> Result<()> {
            let expected = &self.target;
            if target.kind == expected.kind
                && target.identity == expected.identity
                && target.revision == expected.revision
                && target.fingerprint == expected.fingerprint
                && target.app == expected.app
            {
                Ok(())
            } else {
                Err(Error::new(
                    ErrorCode::StaleReference,
                    "fixture observation changed",
                ))
            }
        }
        async fn execute(
            &self,
            _: &Context,
            descriptor: &CommandDescriptor,
            _: &Value,
        ) -> Result<Value> {
            if descriptor.name == self.observe.name {
                return Ok(json!({"ref":{"$ref":self.target}}));
            }
            self.ordinary_calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"done":true,"owner_permit":false}))
        }
        async fn execute_with_owner_scope(
            &self,
            context: &Context,
            descriptor: &CommandDescriptor,
            args: &Value,
            authority: Option<Arc<dyn OwnerOperationAuthority>>,
        ) -> Result<Value> {
            let Some(authority) = authority else {
                return self.execute(context, descriptor, args).await;
            };
            if descriptor.name != self.apply.name {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "Read-only observation unexpectedly received owner authority",
                ));
            }
            self.owner_dispatches.fetch_add(1, Ordering::SeqCst);
            self.dispatched.notify_one();
            if self.before_acquire.load(Ordering::SeqCst) {
                self.pause(context).await?;
            }
            let lease = authority.acquire().map_err(|error| self.rejected(error))?;
            *self.last_mount.lock().unwrap() = Some(lease.logical_mount().to_owned());
            if lease.logical_mount() != "motionwright-data" {
                return Err(self.rejected(Error::new(
                    ErrorCode::PermissionDenied,
                    "Owner permit targets a different logical workspace",
                )));
            }
            lease
                .check_current()
                .map_err(|error| self.rejected(error))?;
            self.acquired.notify_one();
            if self.after_acquire.load(Ordering::SeqCst) {
                self.pause(context).await?;
            }
            lease
                .check_current()
                .map_err(|error| self.rejected(error))?;
            self.guarded_actions.fetch_add(1, Ordering::SeqCst);
            drop(lease);
            Ok(json!({"done":true,"owner_permit":true}))
        }
    }
    struct PermitFixture {
        broker: Arc<Broker>,
        provider: Arc<PermitProvider>,
        root: Arc<TestRoot>,
        _dir: tempfile::TempDir,
    }
    impl PermitFixture {
        async fn new(protocol: u32) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let identity =
                ProviderIdentity::external(SourceKind::Driver, "owner-permit-test", "1").unwrap();
            let observe = CommandDescriptor {
                name: format!("{}observe", identity.namespace),
                version: "1".into(),
                description: "Test native observation".into(),
                input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
                output_schema: json!({"type":"object","properties":{"ref":{"type":"string"}},
                    "required":["ref"],"additionalProperties":false}),
                requires: vec![identity.id.clone()],
                risk: Risk::ReadOnly,
                idempotency: Idempotency::ReadOnly,
                timeout_ms: 10000,
                dry_run: false,
                interactive_consent: false,
                backends: vec![identity.id.clone()],
            };
            let apply = CommandDescriptor {
                name: format!("{}apply", identity.namespace),
                version: "1".into(),
                description: "Test owner-guarded action".into(),
                input_schema: json!({"type":"object","properties":{"ref":{"type":"string"}},
                    "required":["ref"],"additionalProperties":false}),
                output_schema: json!({"type":"object","properties":{
                    "done":{"type":"boolean"},"owner_permit":{"type":"boolean"}},
                    "required":["done","owner_permit"],"additionalProperties":false}),
                requires: vec![identity.id.clone()],
                risk: Risk::MutatingReversible,
                idempotency: Idempotency::NonIdempotent,
                timeout_ms: 10000,
                dry_run: false,
                interactive_consent: false,
                backends: vec![identity.id.clone()],
            };
            let provider = Arc::new(PermitProvider {
                identity,
                observe,
                apply,
                target: NativeTarget {
                    kind: "native".into(),
                    identity: "fixture-project:1".into(),
                    revision: 1,
                    fingerprint: "a".repeat(64),
                    app: "owner-permit-test".into(),
                },
                protocol,
                ordinary_calls: AtomicUsize::new(0),
                owner_dispatches: AtomicUsize::new(0),
                guarded_actions: AtomicUsize::new(0),
                before_acquire: AtomicBool::new(false),
                after_acquire: AtomicBool::new(false),
                dispatched: Notify::new(),
                acquired: Notify::new(),
                proceed: Notify::new(),
                last_error: StdMutex::new(None),
                last_mount: StdMutex::new(None),
                pause_shutdown: AtomicBool::new(false),
                shutdown_entered: Notify::new(),
                shutdown_proceed: Notify::new(),
            });
            let root = Arc::new(TestRoot {
                mount: Mount {
                    source: dir.path().to_path_buf(),
                    class: MountClass::Workspace,
                    logical_name: "motionwright-data".into(),
                    read_only: false,
                    execute: false,
                },
                valid: AtomicBool::new(true),
            });
            let other_dir = dir.path().join("other-scope");
            std::fs::create_dir(&other_dir).unwrap();
            let other_root = Arc::new(TestRoot {
                mount: Mount {
                    source: other_dir.clone(),
                    class: MountClass::Workspace,
                    logical_name: "other-data".into(),
                    read_only: false,
                    execute: false,
                },
                valid: AtomicBool::new(true),
            });
            let config = PolicyConfig {
                allow: [
                    provider.identity.id.clone(),
                    "providers.execute_bound".into(),
                ]
                .into(),
                filesystem: vec![
                    FilesystemGrant {
                        name: "motionwright-data".into(),
                        path: dir.path().into(),
                        read: true,
                        write: true,
                    },
                    FilesystemGrant {
                        name: "other-data".into(),
                        path: other_dir,
                        read: true,
                        write: true,
                    },
                ],
                ..Default::default()
            };
            let audit = audit::Audit::open(&dir.path().join("audit"), 65536, 2).unwrap();
            let broker = Broker::new(
                Policy::new(config).unwrap(),
                vec![],
                audit,
                Arc::new(NoApprover),
                None,
                json!({}),
                false,
            )
            .unwrap();
            broker
                .mount_provider_with_workspace(provider.clone(), Ok(vec![root.clone(), other_root]))
                .await
                .unwrap();
            Self {
                broker,
                provider,
                root,
                _dir: dir,
            }
        }
        async fn call(&self, request: ExecuteRequest) -> Envelope {
            self.broker
                .clone()
                .execute("s".into(), unique_id(), request, CancellationToken::new())
                .await
        }
        fn direct(&self, command: &str, args: Value) -> ExecuteRequest {
            ExecuteRequest {
                command: command.into(),
                args,
                dry_run: false,
                backend: None,
            }
        }
        async fn observed_refs(&self) -> (String, String) {
            let binding = self
                .call(self.direct(
                    "providers.mount.observe",
                    json!({
                        "provider":self.provider.identity.id,"root":"motionwright-data",
                    }),
                ))
                .await;
            assert!(binding.ok, "{:?}", binding.error);
            let binding = binding.data.unwrap()["binding_ref"]
                .as_str()
                .unwrap()
                .to_owned();
            let native = self
                .call(self.direct(&self.provider.observe.name, json!({})))
                .await;
            assert!(native.ok, "{:?}", native.error);
            let native = native.data.unwrap()["ref"].as_str().unwrap().to_owned();
            assert!(native.starts_with("native:"));
            (binding, native)
        }
        fn request(&self, binding: &str, native: &str, bound: bool) -> ExecuteRequest {
            if !bound {
                return self.direct(&self.provider.apply.name, json!({"ref":native}));
            }
            self.direct(
                "providers.execute_bound",
                json!({
                    "binding_ref":binding,"request":{
                        "command":self.provider.apply.name,"args":{"ref":native},"dry_run":false,
                    },
                }),
            )
        }
        fn spawn(&self, request: ExecuteRequest) -> tokio::task::JoinHandle<Envelope> {
            tokio::spawn(self.broker.clone().execute(
                "s".into(),
                unique_id(),
                request,
                CancellationToken::new(),
            ))
        }
    }
    async fn terminal(mut task: tokio::task::JoinHandle<Envelope>) -> Envelope {
        match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
            Ok(Ok(envelope)) => envelope,
            Ok(Err(error)) => panic!("issuer fixture task failed: {error}"),
            Err(_) => {
                task.abort();
                let _ = task.await;
                panic!("issuer fixture terminal deadline exceeded; owned task aborted");
            }
        }
    }
    #[tokio::test]
    async fn bound_native_dispatch_receives_real_core_authority_and_retains_its_gate() {
        let f = PermitFixture::new(9).await;
        let (binding, native) = f.observed_refs().await;
        f.provider.after_acquire.store(true, Ordering::SeqCst);
        let task = f.spawn(f.request(&binding, &native, true));
        let arrived = tokio::time::timeout(Duration::from_secs(5), f.provider.acquired.notified())
            .await
            .is_ok();
        let lease = f.broker.provider(&f.provider.identity.id).unwrap();
        let held_before_action = !lease.operation_gate.drained().unwrap();
        let actions_before_release = f.provider.guarded_actions.load(Ordering::SeqCst);
        f.provider.proceed.notify_one();
        let result = terminal(task).await;
        assert!(arrived, "real Core acquire never completed");
        assert!(held_before_action);
        assert_eq!(actions_before_release, 0);
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.data.unwrap()["result"]["data"]["owner_permit"], true);
        assert_eq!(
            f.provider.last_mount.lock().unwrap().as_deref(),
            Some("motionwright-data")
        );
        assert_eq!(f.provider.owner_dispatches.load(Ordering::SeqCst), 1);
        assert_eq!(f.provider.ordinary_calls.load(Ordering::SeqCst), 0);
        assert_eq!(f.provider.guarded_actions.load(Ordering::SeqCst), 1);
        assert!(lease.operation_gate.drained().unwrap());
    }
    #[tokio::test]
    async fn unbound_and_legacy_dispatch_keep_ordinary_behavior_without_an_issuer() {
        for (protocol, bound) in [(9, false), (7, true)] {
            let f = PermitFixture::new(protocol).await;
            let (binding, native) = f.observed_refs().await;
            let result = terminal(f.spawn(f.request(&binding, &native, bound))).await;
            assert!(result.ok, "{:?}", result.error);
            let value = if bound {
                &result.data.as_ref().unwrap()["result"]["data"]
            } else {
                result.data.as_ref().unwrap()
            };
            assert_eq!(value["owner_permit"], false);
            assert_eq!(f.provider.owner_dispatches.load(Ordering::SeqCst), 0);
            assert_eq!(f.provider.ordinary_calls.load(Ordering::SeqCst), 1);
            assert_eq!(f.provider.guarded_actions.load(Ordering::SeqCst), 0);
        }
    }
    #[tokio::test]
    async fn owner_change_after_dispatch_but_before_acquire_prevents_guarded_action() {
        for revoke_session in [true, false] {
            let f = PermitFixture::new(9).await;
            let (binding, native) = f.observed_refs().await;
            f.provider.before_acquire.store(true, Ordering::SeqCst);
            let task = f.spawn(f.request(&binding, &native, true));
            let arrived =
                tokio::time::timeout(Duration::from_secs(5), f.provider.dispatched.notified())
                    .await
                    .is_ok();
            if arrived {
                if revoke_session {
                    f.broker.revoke_session("s");
                } else {
                    f.root.valid.store(false, Ordering::SeqCst);
                }
            }
            f.provider.proceed.notify_one();
            let result = terminal(task).await;
            assert!(
                arrived,
                "provider did not reach the controlled pre-acquire point"
            );
            assert!(!result.ok);
            assert_eq!(f.provider.owner_dispatches.load(Ordering::SeqCst), 1);
            assert_eq!(f.provider.ordinary_calls.load(Ordering::SeqCst), 0);
            assert_eq!(f.provider.guarded_actions.load(Ordering::SeqCst), 0);
            assert_eq!(
                f.provider.last_error.lock().unwrap().as_ref().unwrap().code,
                ErrorCode::StaleReference
            );
            assert!(
                f.broker
                    .provider(&f.provider.identity.id)
                    .unwrap()
                    .operation_gate
                    .drained()
                    .unwrap()
            );
        }
    }

    #[tokio::test]
    async fn another_valid_mount_of_same_provider_is_not_the_data_commit_scope() {
        let f = PermitFixture::new(9).await;
        let (_, native) = f.observed_refs().await;
        let observed = f
            .call(f.direct(
                "providers.mount.observe",
                json!({
                    "provider":f.provider.identity.id,"root":"other-data",
                }),
            ))
            .await;
        assert!(observed.ok, "{:?}", observed.error);
        let other_binding = observed.data.unwrap()["binding_ref"]
            .as_str()
            .unwrap()
            .to_owned();
        let result = terminal(f.spawn(f.request(&other_binding, &native, true))).await;
        assert!(!result.ok);
        assert_eq!(f.provider.owner_dispatches.load(Ordering::SeqCst), 1);
        assert_eq!(f.provider.ordinary_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            f.provider.last_mount.lock().unwrap().as_deref(),
            Some("other-data")
        );
        assert_eq!(
            f.provider.last_error.lock().unwrap().as_ref().unwrap().code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(f.provider.guarded_actions.load(Ordering::SeqCst), 0);
        assert!(
            f.broker
                .provider(&f.provider.identity.id)
                .unwrap()
                .operation_gate
                .drained()
                .unwrap()
        );
    }

    struct RefreshRemoveRelease {
        pause: Arc<crate::operation_permits::RefreshPublishPause>,
        provider: Arc<PermitProvider>,
    }
    impl Drop for RefreshRemoveRelease {
        fn drop(&mut self) {
            self.pause.proceed.notify_one();
            self.provider.shutdown_proceed.notify_one();
        }
    }
    async fn collect_lifecycle_task(
        mut task: tokio::task::JoinHandle<Result<u64>>,
    ) -> Option<Result<u64>> {
        match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
            Ok(Ok(result)) => Some(result),
            Ok(Err(_)) => None,
            Err(_) => {
                task.abort();
                let _ = task.await;
                None
            }
        }
    }
    #[tokio::test]
    async fn cancelled_remove_tombstone_is_not_replaced_by_refresh() {
        // Real Broker refresh/remove; provider shutdown and physical capsule are
        // controlled test doubles. No actual process/SQLite closure is claimed.
        let f = PermitFixture::new(9).await;
        let lease = f.broker.provider(&f.provider.identity.id).unwrap();
        let pause = Arc::new(crate::operation_permits::RefreshPublishPause::default());
        lease
            .operation_gate
            .pause_next_refresh_publish(pause.clone());
        f.provider.pause_shutdown.store(true, Ordering::SeqCst);
        let _release = RefreshRemoveRelease {
            pause: pause.clone(),
            provider: f.provider.clone(),
        };
        let broker = f.broker.clone();
        let id = f.provider.identity.id.clone();
        let refreshing = tokio::spawn(async move { broker.refresh_provider(&id).await });
        let reached_publish =
            tokio::time::timeout(Duration::from_secs(5), pause.entered.notified())
                .await
                .is_ok();
        let mut removing = None;
        let mut reached_shutdown = false;
        let mut retained_cancelled_lease = false;
        if reached_publish {
            let broker = f.broker.clone();
            let id = f.provider.identity.id.clone();
            removing = Some(tokio::spawn(
                async move { broker.remove_provider(&id).await },
            ));
            reached_shutdown = tokio::time::timeout(
                Duration::from_secs(5),
                f.provider.shutdown_entered.notified(),
            )
            .await
            .is_ok();
            retained_cancelled_lease = lease.lifetime.is_cancelled()
                && f.broker
                    .provider(&f.provider.identity.id)
                    .is_some_and(|current| Arc::ptr_eq(&current, &lease));
        }

        // Release/collect BOTH owned tasks before any test assertion.
        pause.proceed.notify_one();
        let refreshed = collect_lifecycle_task(refreshing).await;
        f.provider.shutdown_proceed.notify_one();
        let removed = match removing {
            Some(task) => collect_lifecycle_task(task).await,
            None => None,
        };
        assert!(
            reached_publish,
            "refresh did not reach the controlled publication window"
        );
        assert!(
            reached_shutdown,
            "remove did not reach its controlled shutdown window"
        );
        assert!(
            retained_cancelled_lease,
            "the intended cancelled same-Arc tombstone was not retained"
        );
        assert!(
            matches!(&refreshed, Some(Err(error)) if error.code == ErrorCode::Conflict),
            "refresh published or failed outside the intended conflict: {refreshed:?}"
        );
        assert!(
            matches!(&removed, Some(Ok(_))),
            "remove did not complete after the original lease drained: {removed:?}"
        );
        assert!(f.broker.provider(&f.provider.identity.id).is_none());
    }
}
