//! Providers enumerate and execute capabilities without knowing caller authorization or MCP.
use crate::{Backend, Context};
use async_trait::async_trait;
use semwright_types::{
    CommandDescriptor, Error, ErrorCode, Feature, JobArtifact, JobProgress, NativeTarget,
    ProviderIdentity, Result, Selector,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct ProvidedCapability {
    pub descriptor: CommandDescriptor,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub object_types: Vec<String>,
}
impl From<CommandDescriptor> for ProvidedCapability {
    fn from(descriptor: CommandDescriptor) -> Self {
        Self {
            descriptor,
            aliases: vec![],
            tags: vec![],
            object_types: vec![],
        }
    }
}
/// A provider cannot supply the authority/source of an event. The runtime binds that itself.
#[derive(Clone, Debug)]
pub enum ProviderSignal {
    CapabilitiesChanged,
    Disconnected,
    Event {
        kind: String,
        payload: Value,
    },
    Progress {
        request_id: String,
        progress: JobProgress,
        artifacts: Vec<JobArtifact>,
    },
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ProviderInterfaces {
    pub dynamic_capabilities: bool,
    pub cooperative_cancellation: bool,
    pub events: bool,
    pub progress: bool,
    pub artifacts: bool,
    pub health: bool,
    /// Provider can emit broker-internal NativeTarget markers and validate them before reuse.
    pub native_refs: bool,
}
#[async_trait]
pub trait Provider: Send + Sync {
    fn identity(&self) -> &ProviderIdentity;
    /// Dynamic routes are exactly the owner-assigned ID. Native adapters retain legacy route aliases.
    fn name(&self) -> &str {
        &self.identity().id
    }
    fn supports(&self, command: &str) -> bool;
    async fn capabilities(&self) -> Result<Vec<ProvidedCapability>>;
    async fn probe(&self) -> Vec<Feature>;
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| command.to_owned())
    }
    fn interfaces(&self) -> ProviderInterfaces {
        ProviderInterfaces::default()
    }
    fn events(&self) -> Option<broadcast::Receiver<ProviderSignal>> {
        None
    }
    /// Cancelled when the underlying transport is irrecoverably closed. No polling is required.
    fn closed(&self) -> Option<CancellationToken> {
        None
    }
    /// Optional backend-owned reduction for `ui.find`. The broker remains authoritative:
    /// it materializes references and reapplies the portable selector to every candidate.
    async fn find_ui_candidates(
        &self,
        _context: &Context,
        _selector: &Selector,
        _max_nodes: usize,
        _max_depth: usize,
    ) -> Result<Option<Value>> {
        Ok(None)
    }
    /// Implementations must execute this pinned descriptor or reject it as stale, never reinterpret it.
    async fn execute(
        &self,
        context: &Context,
        descriptor: &CommandDescriptor,
        args: &Value,
    ) -> Result<Value>;
    fn emits_native_refs(&self) -> bool {
        self.interfaces().native_refs
    }
    async fn validate(&self, _target: &NativeTarget) -> Result<()> {
        Err(Error::new(
            ErrorCode::Unsupported,
            "Provider does not validate native references",
        ))
    }
    async fn is_focused(&self, _target: &NativeTarget) -> Result<bool> {
        Ok(false)
    }
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}
/// Compatibility boundary: existing Linux and application backends are providers, not rewritten drivers.
pub struct NativeProvider {
    backend: Arc<dyn Backend>,
    identity: ProviderIdentity,
    capabilities: Vec<ProvidedCapability>,
}
impl NativeProvider {
    pub fn new(
        backend: Arc<dyn Backend>,
        identity: ProviderIdentity,
        capabilities: Vec<ProvidedCapability>,
    ) -> Self {
        Self {
            backend,
            identity,
            capabilities,
        }
    }
}
#[async_trait]
impl Provider for NativeProvider {
    fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }
    fn name(&self) -> &str {
        self.backend.name()
    }
    fn supports(&self, command: &str) -> bool {
        self.backend.supports(command)
    }
    async fn capabilities(&self) -> Result<Vec<ProvidedCapability>> {
        Ok(self.capabilities.clone())
    }
    async fn probe(&self) -> Vec<Feature> {
        self.backend.probe().await
    }
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.backend.operation_feature(command)
    }
    fn interfaces(&self) -> ProviderInterfaces {
        ProviderInterfaces {
            health: true,
            cooperative_cancellation: true,
            events: self.backend.events().is_some(),
            native_refs: self.backend.emits_native_refs(),
            ..Default::default()
        }
    }
    fn events(&self) -> Option<broadcast::Receiver<ProviderSignal>> {
        self.backend.events()
    }
    async fn find_ui_candidates(
        &self,
        context: &Context,
        selector: &Selector,
        max_nodes: usize,
        max_depth: usize,
    ) -> Result<Option<Value>> {
        self.backend
            .find_ui_candidates(context, selector, max_nodes, max_depth)
            .await
    }
    async fn execute(
        &self,
        context: &Context,
        descriptor: &CommandDescriptor,
        args: &Value,
    ) -> Result<Value> {
        self.backend.execute(context, &descriptor.name, args).await
    }
    fn emits_native_refs(&self) -> bool {
        self.backend.emits_native_refs()
    }
    async fn validate(&self, target: &NativeTarget) -> Result<()> {
        self.backend.validate(target).await
    }
    async fn is_focused(&self, target: &NativeTarget) -> Result<bool> {
        self.backend.is_focused(target).await
    }
    async fn shutdown(&self) -> Result<()> {
        self.backend.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semwright_types::{ProviderIdentity, SourceKind};
    use serde_json::json;

    struct EventBackend {
        tx: broadcast::Sender<ProviderSignal>,
    }

    #[async_trait]
    impl Backend for EventBackend {
        fn name(&self) -> &'static str {
            "event-fixture"
        }
        fn supports(&self, _: &str) -> bool {
            false
        }
        async fn probe(&self) -> Vec<Feature> {
            vec![]
        }
        fn events(&self) -> Option<broadcast::Receiver<ProviderSignal>> {
            Some(self.tx.subscribe())
        }
        async fn execute(&self, _: &Context, _: &str, _: &Value) -> Result<Value> {
            Err(Error::new(ErrorCode::Unsupported, "fixture"))
        }
    }

    #[tokio::test]
    async fn native_provider_forwards_backend_events() {
        let (tx, _) = broadcast::channel(8);
        let backend = Arc::new(EventBackend { tx: tx.clone() });
        let identity =
            ProviderIdentity::external(SourceKind::Driver, "event-fixture", "1").unwrap();
        let provider = NativeProvider::new(backend, identity, vec![]);
        assert!(provider.interfaces().events);
        let mut rx = provider.events().expect("event receiver");
        tx.send(ProviderSignal::Event {
            kind: "semantic.text.changed".into(),
            payload: json!({"node_id":"ui-node:test"}),
        })
        .unwrap();
        match rx.recv().await.unwrap() {
            ProviderSignal::Event { kind, payload } => {
                assert_eq!(kind, "semantic.text.changed");
                assert_eq!(payload["node_id"], "ui-node:test");
            }
            other => panic!("unexpected signal: {other:?}"),
        }
    }
}
