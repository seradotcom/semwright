//! Backends know Linux/application protocols, never MCP or caller authorization.
pub mod provider;
use async_trait::async_trait;
pub use provider::{
    NativeProvider, ProvidedCapability, Provider, ProviderInterfaces, ProviderSignal,
};
use semwright_types::{Error, ErrorCode, Feature, NativeTarget, Result};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Context {
    pub session: String,
    pub request_id: String,
    pub cancellation: CancellationToken,
}
impl Context {
    pub fn check_cancelled(&self) -> Result<()> {
        if self.cancellation.is_cancelled() {
            Err(Error::new(ErrorCode::Cancelled, "Command cancelled"))
        } else {
            Ok(())
        }
    }
}
#[async_trait]
pub trait Backend: Send + Sync {
    fn name(&self) -> &'static str;
    fn supports(&self, command: &str) -> bool;
    async fn probe(&self) -> Vec<Feature>;
    /// The exact probe key required by an operation. Unknown mappings fail closed.
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| command.to_owned())
    }
    /// Only trusted native providers may emit internal object-reference markers.
    fn emits_native_refs(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &Context, command: &str, args: &Value) -> Result<Value>;
    /// Must reject changed/reused identities immediately before side effects.
    async fn validate(&self, _target: &NativeTarget) -> Result<()> {
        Err(Error::new(
            ErrorCode::Unsupported,
            "Backend does not validate this target type",
        ))
    }
    /// Conservative by default: input is forbidden unless a backend can prove focus.
    async fn is_focused(&self, _target: &NativeTarget) -> Result<bool> {
        Ok(false)
    }
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}
pub fn feature(
    backend: &str,
    capability: &str,
    available: bool,
    reason: &str,
    remedy: &str,
) -> Feature {
    Feature {
        backend: backend.into(),
        capability: capability.into(),
        status: if available {
            semwright_types::CapabilityStatus::Supported
        } else {
            semwright_types::CapabilityStatus::Unavailable
        },
        reason: reason.into(),
        remediation: remedy.into(),
    }
}
