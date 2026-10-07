//! Reviewer-owned defensive tests; disposable journal and deterministic FakeDesktop only.
use async_trait::async_trait;
use semwright_backend_api::{Backend, Context};
use semwright_backends::fake::FakeDesktop;
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_policy::{Policy, PolicyConfig, Profile};
use semwright_types::{Envelope, ExecuteRequest, Feature, NativeTarget, Result, unique_id};
use serde_json::{Value, json};
use std::{path::{Path, PathBuf}, sync::Arc};
use tokio_util::sync::CancellationToken;

fn force_next_rotation_failure(audit: &Arc<Audit>, directory: &Path) {
    // Fill only this test's journal, stopping immediately at its configured rotation bound.
    for _ in 0..1024 {
        if std::fs::metadata(directory.join("audit.jsonl")).unwrap().len() >= 65536 {
            std::fs::create_dir(directory.join("audit.jsonl.1")).unwrap();
            return;
        }
        let mut record = audit.begin("reviewer.fixture.padding", &unique_id(), "reviewer-fault").unwrap();
        if std::fs::metadata(directory.join("audit.jsonl")).unwrap().len() >= 65536 {
            std::fs::create_dir(directory.join("audit.jsonl.1")).unwrap();
            // Keep this synthetic unfinished scope from triggering rotation during Drop.
            // It is one bounded fixture allocation reclaimed at process exit.
            std::mem::forget(record);
            return;
        }
        record.finish(&Ok(json!({}))).unwrap();
    }
    panic!("bounded padding did not reach rotation threshold");
}

struct PostEffectAuditFault {
    desktop: Arc<FakeDesktop>,
    audit: Arc<Audit>,
    directory: PathBuf,
}
#[async_trait]
impl Backend for PostEffectAuditFault {
    fn name(&self) -> &'static str { "fake" }
    fn supports(&self, command: &str) -> bool { self.desktop.supports(command) }
    fn operation_feature(&self, command: &str) -> Option<String> { self.desktop.operation_feature(command) }
    async fn probe(&self) -> Vec<Feature> { self.desktop.probe().await }
    async fn validate(&self, target: &NativeTarget) -> Result<()> { self.desktop.validate(target).await }
    async fn is_focused(&self, target: &NativeTarget) -> Result<bool> { self.desktop.is_focused(target).await }
    async fn execute(&self, context: &Context, command: &str, args: &Value) -> Result<Value> {
        let result = self.desktop.execute(context, command, args).await;
        if command == "ui.invoke" && result.is_ok() {
            force_next_rotation_failure(&self.audit, &self.directory);
        }
        result
    }
}

async fn call(broker: &Arc<Broker>, session: &str, command: &str, args: Value) -> Envelope {
    broker.clone().execute(session.into(), unique_id(), ExecuteRequest {
        command: command.into(), args, dry_run: false, backend: None,
    }, CancellationToken::new()).await
}

#[tokio::test]
async fn reviewer_audit_failure_before_dispatch_has_zero_effects() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("audit");
    let audit = Audit::open(&directory, 65536, 2).unwrap();
    let desktop = Arc::new(FakeDesktop::new());
    let broker = Broker::new(Policy::new(PolicyConfig { profile: Profile::Desktop, ..Default::default() }).unwrap(),
        vec![desktop.clone()], audit.clone(), Arc::new(NoApprover), None, json!({"fixture":true}), true).unwrap();
    let session = unique_id();
    let found = call(&broker, &session, "ui.find", json!({"selector":{"name":{"op":"exact","value":"Export"}}})).await;
    assert!(found.ok, "{found:?}");
    let reference = found.data.unwrap()["nodes"][0]["ref"].clone();
    force_next_rotation_failure(&audit, &directory);
    let result = call(&broker, &session, "ui.invoke", json!({"ref":reference})).await;
    assert!(!result.ok, "audit failure must block dispatch");
    assert_eq!(desktop.invocations(), 0, "audit begin failed before side effect");
    println!("REVIEWER_ASSERT audit_before_dispatch effects=0 error={:?}", result.error);
}

#[tokio::test]
async fn reviewer_audit_failure_after_effect_returns_uncertain_without_retry() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("audit");
    let audit = Audit::open(&directory, 65536, 2).unwrap();
    let desktop = Arc::new(FakeDesktop::new());
    let backend = Arc::new(PostEffectAuditFault { desktop: desktop.clone(), audit: audit.clone(), directory });
    let broker = Broker::new(Policy::new(PolicyConfig { profile: Profile::Desktop, ..Default::default() }).unwrap(),
        vec![backend], audit, Arc::new(NoApprover), None, json!({"fixture":true}), true).unwrap();
    let session = unique_id();
    let found = call(&broker, &session, "ui.find", json!({"selector":{"name":{"op":"exact","value":"Export"}}})).await;
    assert!(found.ok, "{found:?}");
    let reference = found.data.unwrap()["nodes"][0]["ref"].clone();
    let result = call(&broker, &session, "ui.invoke", json!({"ref":reference})).await;
    assert!(!result.ok, "lost audit completion must not be reported as success");
    let error = result.error.as_ref().expect("uncertain error");
    assert!(!error.outcome_known, "effect already happened before journal failure");
    assert_eq!(desktop.invocations(), 1, "one effect with no retry");
    assert!(result.execution.fallbacks_attempted.is_empty());
    println!("REVIEWER_ASSERT audit_after_effect effects=1 outcome_known=false error={error:?}");
}
