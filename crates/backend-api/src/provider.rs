//! Providers enumerate and execute capabilities without knowing caller authorization or MCP.
use crate::{Backend, Context};
use async_trait::async_trait;
use semwright_types::{
    CommandDescriptor, Error, ErrorCode, Feature, NativeTarget, ProviderIdentity, Result,
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
    Event { kind: String, payload: Value },
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ProviderInterfaces {
    pub dynamic_capabilities: bool,
    pub cooperative_cancellation: bool,
    pub events: bool,
    pub health: bool,
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
    /// Implementations must execute this pinned descriptor or reject it as stale, never reinterpret it.
    async fn execute(
        &self,
        context: &Context,
        descriptor: &CommandDescriptor,
        args: &Value,
    ) -> Result<Value>;
    fn emits_native_refs(&self) -> bool {
        false
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
            ..Default::default()
        }
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
