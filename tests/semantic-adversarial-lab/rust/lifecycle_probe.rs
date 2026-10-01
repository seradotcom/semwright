//! Independent G lifecycle/fault-injection probes for A's Broker/provider runtime.
//! Synthetic in-memory provider only: no external network, UI, secrets or user data.
use async_trait::async_trait;
use semwright_backend_api::{
    Context, ProvidedCapability, Provider, ProviderInterfaces, ProviderSignal, feature,
};
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_policy::{Policy, PolicyConfig};
use semwright_registry::{CatalogQuery, catalog::descriptor_digest};
use semwright_types::{
    CommandDescriptor, Envelope, Error, ErrorCode, ExecuteRequest, Feature, Idempotency,
    JobArtifact, JobProgress, ProviderIdentity, Result, Risk, SourceKind, unique_id,
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Notify, broadcast};
use tokio_util::sync::CancellationToken;

const SOURCE: &str = env!("G_LAB_COMPILED_SOURCE_SHA");
type ProbeResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn descriptor(identity: &ProviderIdentity, suffix: &str) -> CommandDescriptor {
    CommandDescriptor {
        name: format!("{}{suffix}", identity.namespace),
        version: "1".into(),
        description: "G synthetic lifecycle fixture".into(),
        input_schema: json!({"type":"object","properties":{},"additionalProperties":false}),
        output_schema: json!({
            "type":"object",
            "properties":{"value":{"type":"integer"}},
            "required":["value"],
            "additionalProperties":false
        }),
        requires: vec![identity.id.clone()],
        risk: Risk::ReadOnly,
        idempotency: Idempotency::ReadOnly,
        timeout_ms: 1_000,
        dry_run: true,
        interactive_consent: false,
        backends: vec![identity.id.clone()],
    }
}

struct SyntheticProvider {
    identity: ProviderIdentity,
    commands: Mutex<Vec<CommandDescriptor>>,
    signal: broadcast::Sender<ProviderSignal>,
    closed: CancellationToken,
    blocked: AtomicBool,
    malformed: AtomicBool,
    cancelled: AtomicBool,
    calls: AtomicUsize,
    shutdowns: AtomicUsize,
    entered: Notify,
    release: Notify,
}

impl SyntheticProvider {
    fn new() -> Arc<Self> {
        let identity =
            ProviderIdentity::external(SourceKind::Driver, "g-lifecycle", "1").expect("identity");
        let commands = vec![
            descriptor(&identity, "count"),
            descriptor(&identity, "progress"),
        ];
        Arc::new(Self {
            identity,
            commands: Mutex::new(commands),
            signal: broadcast::channel(32).0,
            closed: CancellationToken::new(),
            blocked: AtomicBool::new(false),
            malformed: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            shutdowns: AtomicUsize::new(0),
            entered: Notify::new(),
            release: Notify::new(),
        })
    }
}

struct CancelEvidence<'a>(&'a AtomicBool, CancellationToken);
impl Drop for CancelEvidence<'_> {
    fn drop(&mut self) {
        self.0.store(self.1.is_cancelled(), Ordering::SeqCst);
    }
}

#[async_trait]
impl Provider for SyntheticProvider {
    fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }

    fn supports(&self, name: &str) -> bool {
        self.commands
            .lock()
            .expect("commands")
            .iter()
            .any(|command| command.name == name)
    }

    async fn capabilities(&self) -> Result<Vec<ProvidedCapability>> {
        Ok(self
            .commands
            .lock()
            .expect("commands")
            .iter()
            .cloned()
            .map(Into::into)
            .collect())
    }

    async fn probe(&self) -> Vec<Feature> {
        self.commands
            .lock()
            .expect("commands")
            .iter()
            .map(|command| {
                feature(
                    &self.identity.id,
                    &command.name,
                    true,
                    "G synthetic provider",
                    "none",
                )
            })
            .collect()
    }

    fn interfaces(&self) -> ProviderInterfaces {
        ProviderInterfaces {
            dynamic_capabilities: true,
            cooperative_cancellation: true,
            events: true,
            progress: true,
            artifacts: true,
            health: true,
            native_refs: false,
        }
    }

    fn events(&self) -> Option<broadcast::Receiver<ProviderSignal>> {
        Some(self.signal.subscribe())
    }

    fn closed(&self) -> Option<CancellationToken> {
        Some(self.closed.clone())
    }

    async fn execute(
        &self,
        context: &Context,
        requested: &CommandDescriptor,
        _args: &Value,
    ) -> Result<Value> {
        {
            let commands = self.commands.lock().expect("commands");
            let current = commands
                .iter()
                .find(|command| command.name == requested.name)
                .ok_or_else(|| Error::new(ErrorCode::Conflict, "descriptor disappeared"))?;
            if descriptor_digest(current)? != descriptor_digest(requested)? {
                return Err(Error::new(ErrorCode::Conflict, "descriptor changed"));
            }
        }

        self.calls.fetch_add(1, Ordering::SeqCst);
        let _cancel = CancelEvidence(&self.cancelled, context.cancellation.clone());
        self.entered.notify_one();

        if self.blocked.load(Ordering::SeqCst) {
            tokio::select! {
                _ = context.cancellation.cancelled() => {
                    return Err(Error::new(ErrorCode::Cancelled, "G fixture cancelled"));
                }
                _ = self.release.notified() => {}
            }
        }

        if self.malformed.load(Ordering::SeqCst) {
            return Ok(json!({"value":"not-an-integer"}));
        }

        if requested.name.ends_with("progress") {
            let _ = self.signal.send(ProviderSignal::Progress {
                request_id: context.request_id.clone(),
                progress: JobProgress {
                    completed: 2,
                    total: Some(2),
                    message: Some("G fixture complete".into()),
                },
                artifacts: vec![JobArtifact {
                    name: "preview".into(),
                    reference: "artifact:g-lifecycle-preview".into(),
                    media_type: Some("application/octet-stream".into()),
                    sha256: Some("b".repeat(64)),
                    bytes: Some(16),
                }],
            });
            tokio::time::sleep(Duration::from_millis(80)).await;
        }

        Ok(json!({"value":1}))
    }

    async fn shutdown(&self) -> Result<()> {
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
        self.closed.cancel();
        Ok(())
    }
}

struct Fixture {
    broker: Arc<Broker>,
    provider: Arc<SyntheticProvider>,
    root: PathBuf,
}

impl Fixture {
    fn new() -> ProbeResult<Self> {
        let root = PathBuf::from(format!("/out/g-lifecycle-{}", unique_id()));
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        let provider = SyntheticProvider::new();
        let policy = Policy::new(PolicyConfig {
            allow: [provider.identity.id.clone()].into(),
            ..Default::default()
        })?;
        let audit = Audit::open(&root.join("audit"), 65_536, 2)?;
        let broker = Broker::new(
            policy,
            vec![],
            audit,
            Arc::new(NoApprover),
            None,
            json!({"g_lifecycle_adversarial":true}),
            false,
        )?;
        Ok(Self {
            broker,
            provider,
            root,
        })
    }

    async fn mount(&self) -> ProbeResult<u64> {
        Ok(self.broker.mount_provider(self.provider.clone()).await?)
    }

    async fn execute(
        &self,
        session: &str,
        command: &str,
        args: Value,
        cancellation: CancellationToken,
    ) -> Envelope {
        self.broker
            .clone()
            .execute(
                session.into(),
                unique_id(),
                ExecuteRequest {
                    command: command.into(),
                    args,
                    dry_run: false,
                    backend: None,
                },
                cancellation,
            )
            .await
    }

    async fn call(&self, session: &str, suffix: &str) -> Envelope {
        self.execute(
            session,
            &format!("driver.g-lifecycle.{suffix}"),
            json!({}),
            CancellationToken::new(),
        )
        .await
    }

    async fn job_start(&self, session: &str, suffix: &str) -> Envelope {
        self.execute(
            session,
            "jobs.start",
            json!({"request":{"command":format!("driver.g-lifecycle.{suffix}"),"args":{}}}),
            CancellationToken::new(),
        )
        .await
    }

    async fn job_get(&self, session: &str, id: &str) -> Envelope {
        self.execute(
            session,
            "jobs.get",
            json!({"job_id":id}),
            CancellationToken::new(),
        )
        .await
    }

    async fn job_cancel(&self, session: &str, id: &str) -> Envelope {
        self.execute(
            session,
            "jobs.cancel",
            json!({"job_id":id}),
            CancellationToken::new(),
        )
        .await
    }

    async fn wait_inactive(&self) -> ProbeResult<()> {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let providers = self.broker.provider_status().expect("status");
                let inactive = providers["providers"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|row| row["route"] == "driver:g-lifecycle"))
                    .is_some_and(|row| row["connected"] == false);
                if inactive {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await?;
        Ok(())
    }

    async fn wait_job_terminal(&self, session: &str, id: &str) -> ProbeResult<Value> {
        Ok(tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let out = self.job_get(session, id).await;
                if out.ok {
                    let job = out.data.as_ref().expect("data")["job"].clone();
                    if matches!(
                        job["state"].as_str(),
                        Some("succeeded" | "failed" | "cancelled")
                    ) {
                        break job;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await?)
    }

    async fn close(self) -> ProbeResult<()> {
        self.broker.shutdown().await;
        let root = self.root.clone();
        drop(self);
        if root.exists() {
            fs::remove_dir_all(root)?;
        }
        Ok(())
    }
}

fn error_code(envelope: &Envelope) -> Option<ErrorCode> {
    envelope.error.as_ref().map(|error| error.code)
}

async fn probe(id: &str) -> ProbeResult<Value> {
    Ok(match id {
        "G-LIFE-001" => {
            let fixture = Fixture::new()?;
            let before = fixture.broker.catalog_revision()?;
            let (a, b) = tokio::join!(
                fixture.broker.mount_provider(fixture.provider.clone()),
                fixture.broker.mount_provider(fixture.provider.clone())
            );
            let after = fixture.broker.catalog_revision()?;
            let count = fixture.broker.provider_status()?["providers"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .filter(|row| row["route"] == "driver:g-lifecycle")
                        .count()
                })
                .unwrap_or(0);
            let observed = json!({
                "exactly_one_mount":a.is_ok()!=b.is_ok(),
                "single_revision_advance":after==before+1,
                "single_provider":count==1
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-002" => {
            let fixture = Fixture::new()?;
            fixture.mount().await?;
            let before = fixture.broker.catalog_revision()?;
            fixture.provider.commands.lock().expect("commands")[0].version = "2".into();
            let after = fixture
                .broker
                .refresh_provider("driver:g-lifecycle")
                .await?;
            let call = fixture.call("g-session", "count").await;
            let observed = json!({
                "revision_advanced":after>before,
                "new_descriptor_visible":fixture.broker.describe("driver.g-lifecycle.count")?.version=="2",
                "future_call_succeeds":call.ok
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-003" => {
            let fixture = Fixture::new()?;
            fixture.mount().await?;
            fixture.provider.commands.lock().expect("commands")[0].requires =
                vec!["desktop.observe".into()];
            let refresh_rejected = fixture
                .broker
                .refresh_provider("driver:g-lifecycle")
                .await
                .is_err();
            let descriptor_preserved = fixture
                .broker
                .describe("driver.g-lifecycle.count")?
                .requires
                == vec!["driver:g-lifecycle".to_string()];
            let call = fixture.call("g-session", "count").await;
            let observed = json!({
                "invalid_refresh_rejected":refresh_rejected,
                "old_descriptor_preserved":descriptor_preserved,
                "execution_revoked":error_code(&call)==Some(ErrorCode::Unavailable)
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-004" => {
            let fixture = Fixture::new()?;
            fixture.mount().await?;
            fixture.provider.signal.send(ProviderSignal::Disconnected)?;
            fixture.wait_inactive().await?;
            let refresh_rejected = fixture
                .broker
                .refresh_provider("driver:g-lifecycle")
                .await
                .is_err();
            let call = fixture.call("g-session", "count").await;
            let observed = json!({
                "terminal_disconnect_inactive":error_code(&call)==Some(ErrorCode::Unavailable),
                "refresh_cannot_revive":refresh_rejected,
                "provider_not_dispatched":fixture.provider.calls.load(Ordering::SeqCst)==0
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-005" => {
            let fixture = Fixture::new()?;
            fixture.provider.blocked.store(true, Ordering::SeqCst);
            fixture.mount().await?;
            let broker = fixture.broker.clone();
            let running = tokio::spawn(async move {
                broker
                    .execute(
                        "g-session".into(),
                        unique_id(),
                        ExecuteRequest {
                            command: "driver.g-lifecycle.count".into(),
                            args: json!({}),
                            dry_run: false,
                            backend: None,
                        },
                        CancellationToken::new(),
                    )
                    .await
            });
            tokio::time::timeout(Duration::from_secs(2), fixture.provider.entered.notified())
                .await?;
            fixture.provider.closed.cancel();
            let result = tokio::time::timeout(Duration::from_secs(2), running).await??;
            let observed = json!({
                "inflight_returns_unavailable":error_code(&result)==Some(ErrorCode::Unavailable),
                "outcome_unknown":result.error.as_ref().is_some_and(|e|!e.outcome_known),
                "provider_cancelled":fixture.provider.cancelled.load(Ordering::SeqCst),
                "no_retry":fixture.provider.calls.load(Ordering::SeqCst)==1
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-006" => {
            let fixture = Fixture::new()?;
            fixture.provider.blocked.store(true, Ordering::SeqCst);
            fixture.mount().await?;
            let stop = CancellationToken::new();
            let broker = fixture.broker.clone();
            let child_stop = stop.clone();
            let running = tokio::spawn(async move {
                broker
                    .execute(
                        "g-session".into(),
                        unique_id(),
                        ExecuteRequest {
                            command: "driver.g-lifecycle.count".into(),
                            args: json!({}),
                            dry_run: false,
                            backend: None,
                        },
                        child_stop,
                    )
                    .await
            });
            tokio::time::timeout(Duration::from_secs(2), fixture.provider.entered.notified())
                .await?;
            stop.cancel();
            let result = tokio::time::timeout(Duration::from_secs(2), running).await??;
            let observed = json!({
                "cancelled":error_code(&result)==Some(ErrorCode::Cancelled),
                "outcome_unknown":result.error.as_ref().is_some_and(|e|!e.outcome_known),
                "provider_cancelled":fixture.provider.cancelled.load(Ordering::SeqCst),
                "no_retry":fixture.provider.calls.load(Ordering::SeqCst)==1
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-007" => {
            let fixture = Fixture::new()?;
            fixture.provider.blocked.store(true, Ordering::SeqCst);
            fixture.provider.commands.lock().expect("commands")[0].timeout_ms = 40;
            fixture.mount().await?;
            let result = fixture.call("g-session", "count").await;
            let observed = json!({
                "timed_out":error_code(&result)==Some(ErrorCode::Timeout),
                "outcome_unknown":result.error.as_ref().is_some_and(|e|!e.outcome_known),
                "provider_cancelled":fixture.provider.cancelled.load(Ordering::SeqCst),
                "no_retry":fixture.provider.calls.load(Ordering::SeqCst)==1
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-008" => {
            let fixture = Fixture::new()?;
            fixture.provider.malformed.store(true, Ordering::SeqCst);
            fixture.mount().await?;
            let result = fixture.call("g-session", "count").await;
            let observed = json!({
                "schema_failure":error_code(&result)==Some(ErrorCode::BackendFailed),
                "no_data":result.data.is_none(),
                "one_dispatch":fixture.provider.calls.load(Ordering::SeqCst)==1
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-009" => {
            let fixture = Fixture::new()?;
            fixture.provider.blocked.store(true, Ordering::SeqCst);
            fixture.mount().await?;
            let session = "g-session-a";
            let started = fixture.job_start(session, "count").await;
            let id = started.data.as_ref().expect("data")["job"]["id"]
                .as_str()
                .expect("job id")
                .to_owned();
            tokio::time::timeout(Duration::from_secs(2), fixture.provider.entered.notified())
                .await?;
            let foreign = fixture.job_get("g-session-b", &id).await;
            let _ = fixture.job_cancel(session, &id).await;
            let _ = fixture.wait_job_terminal(session, &id).await?;
            let observed = json!({
                "job_started":started.ok,
                "foreign_lookup_not_found":error_code(&foreign)==Some(ErrorCode::NotFound)
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-010" => {
            let fixture = Fixture::new()?;
            fixture.provider.blocked.store(true, Ordering::SeqCst);
            fixture.mount().await?;
            let session = "g-session";
            let started = fixture.job_start(session, "count").await;
            let id = started.data.as_ref().expect("data")["job"]["id"]
                .as_str()
                .expect("id")
                .to_owned();
            tokio::time::timeout(Duration::from_secs(2), fixture.provider.entered.notified())
                .await?;
            let first = fixture.job_cancel(session, &id).await;
            let second = fixture.job_cancel(session, &id).await;
            let terminal = fixture.wait_job_terminal(session, &id).await?;
            let cancel_events = fixture
                .broker
                .replay_for(session, 0)?
                .iter()
                .filter(|event| {
                    event.event.kind == "job.cancel_requested"
                        && serde_json::to_string(event).is_ok_and(|text| text.contains(&id))
                })
                .count();
            let observed = json!({
                "first_requests_cancel":first.data.as_ref().is_some_and(|d|d["job"]["cancellation_requested"]==true),
                "second_is_idempotent":second.ok,
                "terminal_cancelled":terminal["state"]=="cancelled",
                "single_cancel_event":cancel_events==1,
                "provider_cancelled":fixture.provider.cancelled.load(Ordering::SeqCst)
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-011" => {
            let fixture = Fixture::new()?;
            fixture.mount().await?;
            let session = "g-session";
            let started = fixture.job_start(session, "progress").await;
            let id = started.data.as_ref().expect("data")["job"]["id"]
                .as_str()
                .expect("id")
                .to_owned();
            let terminal = fixture.wait_job_terminal(session, &id).await?;
            let foreign = fixture.job_get("foreign", &id).await;
            let observed = json!({
                "succeeded":terminal["state"]=="succeeded",
                "progress_complete":terminal["progress"]["completed"]==2&&terminal["progress"]["total"]==2,
                "one_artifact":terminal["artifacts"].as_array().is_some_and(|a|a.len()==1),
                "artifact_bound":terminal["artifacts"][0]["reference"]=="artifact:g-lifecycle-preview"&&terminal["artifacts"][0]["bytes"]==16,
                "foreign_hidden":error_code(&foreign)==Some(ErrorCode::NotFound)
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-012" => {
            let fixture = Fixture::new()?;
            fixture.provider.blocked.store(true, Ordering::SeqCst);
            fixture.mount().await?;
            let session = "g-revoked";
            let started = fixture.job_start(session, "count").await;
            let id = started.data.as_ref().expect("data")["job"]["id"]
                .as_str()
                .expect("id")
                .to_owned();
            tokio::time::timeout(Duration::from_secs(2), fixture.provider.entered.notified())
                .await?;
            fixture.broker.revoke_session(session);
            tokio::time::timeout(Duration::from_secs(2), async {
                while !fixture.provider.cancelled.load(Ordering::SeqCst) {
                    tokio::task::yield_now().await;
                }
            })
            .await?;
            let lookup = fixture.job_get(session, &id).await;
            let observed = json!({
                "provider_cancelled":fixture.provider.cancelled.load(Ordering::SeqCst),
                "job_forgotten":error_code(&lookup)==Some(ErrorCode::NotFound)
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-013" => {
            let fixture = Fixture::new()?;
            fixture.mount().await?;
            let result = fixture
                .execute(
                    "g-session",
                    "jobs.start",
                    json!({"request":{"command":"jobs.list","args":{}}}),
                    CancellationToken::new(),
                )
                .await;
            let observed = json!({
                "nested_job_control_rejected":error_code(&result)==Some(ErrorCode::InvalidArgument)
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-014" => {
            let fixture = Fixture::new()?;
            let result = fixture.broker.replay(u64::MAX);
            let observed = json!({
                "future_cursor_rejected":result.as_ref().err().map(|e|e.code)==Some(ErrorCode::Conflict)
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-015" => {
            let fixture = Fixture::new()?;
            fixture.mount().await?;
            let session = "g-audience-a";
            let started = fixture.job_start(session, "count").await;
            let id = started.data.as_ref().expect("data")["job"]["id"]
                .as_str()
                .expect("id")
                .to_owned();
            let _ = fixture.wait_job_terminal(session, &id).await?;
            let a = fixture.broker.replay_for(session, 0)?;
            let b = fixture.broker.replay_for("g-audience-b", 0)?;
            let a_text = serde_json::to_string(&a)?;
            let b_text = serde_json::to_string(&b)?;
            let ordered = a.windows(2).all(|w| w[0].sequence < w[1].sequence);
            let observed = json!({
                "owner_sees_job_events":a_text.contains(&id),
                "foreign_does_not":!b_text.contains(&id),
                "sequence_monotonic":ordered
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-016" => {
            let fixture = Fixture::new()?;
            fixture.mount().await?;
            let before = fixture.broker.catalog_revision()?;
            fixture.provider.commands.lock().expect("commands")[0].version = "2".into();
            fixture
                .provider
                .signal
                .send(ProviderSignal::CapabilitiesChanged)?;
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if fixture
                        .broker
                        .describe("driver.g-lifecycle.count")
                        .is_ok_and(|d| d.version == "2")
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await?;
            let mut events = fixture.broker.subscribe();
            fixture.provider.signal.send(ProviderSignal::Event {
                kind: "object.created".into(),
                payload: json!({"source":"semwright-core","id":"g-object"}),
            })?;
            let event = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let event = events.recv().await.expect("event");
                    if event.event.kind == "object.created" {
                        break event;
                    }
                }
            })
            .await?;
            let observed = json!({
                "signal_refresh_advanced":fixture.broker.catalog_revision()? > before,
                "refreshed_descriptor_visible":fixture.broker.describe("driver.g-lifecycle.count")?.version=="2",
                "source_bound_to_provider":event.event.source=="driver:g-lifecycle",
                "payload_marked_untrusted":event.event.untrusted_payload,
                "forged_payload_preserved_as_data":event.event.payload.as_ref().is_some_and(|p|p["source"]=="semwright-core")
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-017" => {
            let fixture = Fixture::new()?;
            fixture.mount().await?;
            let before = fixture.broker.catalog_revision()?;
            let after = fixture.broker.invalidate_provider("driver:g-lifecycle")?;
            let stale = fixture
                .broker
                .catalog_search(CatalogQuery {
                    revision: Some(before),
                    ..Default::default()
                })
                .await;
            let call = fixture.call("g-session", "count").await;
            let observed = json!({
                "revision_advanced":after>before,
                "stale_catalog_rejected":stale.as_ref().err().map(|e|e.code)==Some(ErrorCode::Conflict),
                "invalidated_execution_unavailable":error_code(&call)==Some(ErrorCode::Unavailable)
            });
            fixture.close().await?;
            observed
        }
        "G-LIFE-018" => {
            let fixture = Fixture::new()?;
            fixture.provider.blocked.store(true, Ordering::SeqCst);
            fixture.mount().await?;
            let broker = fixture.broker.clone();
            let running = tokio::spawn(async move {
                broker
                    .execute(
                        "g-session".into(),
                        unique_id(),
                        ExecuteRequest {
                            command: "driver.g-lifecycle.count".into(),
                            args: json!({}),
                            dry_run: false,
                            backend: None,
                        },
                        CancellationToken::new(),
                    )
                    .await
            });
            tokio::time::timeout(Duration::from_secs(2), fixture.provider.entered.notified())
                .await?;
            let before = fixture.broker.catalog_revision()?;
            fixture.provider.commands.lock().expect("commands")[0].version = "2".into();
            fixture
                .provider
                .signal
                .send(ProviderSignal::CapabilitiesChanged)?;
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if fixture
                        .broker
                        .describe("driver.g-lifecycle.count")
                        .is_ok_and(|d| d.version == "2")
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await?;
            fixture.provider.release.notify_waiters();
            let completed = tokio::time::timeout(Duration::from_secs(2), running).await??;
            let observed = json!({
                "refresh_advanced":fixture.broker.catalog_revision()? > before,
                "inflight_not_cancelled":completed.ok,
                "single_dispatch":fixture.provider.calls.load(Ordering::SeqCst)==1
            });
            fixture.close().await?;
            observed
        }
        _ => {
            return Err(format!("unregistered lifecycle selector: {id}").into());
        }
    })
}

fn cases() -> Vec<String> {
    (1..=18).map(|i| format!("G-LIFE-{i:03}")).collect()
}

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 || std::env::var("G_LAB_TARGET_SHA").as_deref() != Ok(SOURCE) {
        std::process::exit(2)
    }
    if args[0] == "--list" {
        println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"cases":cases()})
        );
        return;
    }
    if !cases().contains(&args[0]) {
        std::process::exit(2)
    }
    match probe(&args[0]).await {
        Ok(observed) => println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"case_id":args[0],"observed":observed})
        ),
        Err(error) => {
            eprintln!("lifecycle probe contract/setup error: {error:?}");
            std::process::exit(1)
        }
    }
}
