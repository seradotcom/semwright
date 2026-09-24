//! macOS is a Backend of the SAME broker, registry, policy, refs and jobs.
pub mod roles;
pub mod transport;
use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, feature};
use semwright_types::{Error, ErrorCode, Feature, NativeTarget, Result};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;
use transport::{NativeTransport, Transport};
pub const COMMANDS: &[&str] = &[
    "app.list",
    "window.list",
    "window.focus",
    "window.move",
    "window.resize",
    "window.close",
    "ui.snapshot",
    "ui.hit_test",
    "ui.invoke",
    "ui.set_text",
    "ui.read_text",
    "ui.set_value",
    "ui.get_value",
    "ui.toggle",
    "ui.expand",
    "input.key",
    "input.type",
    "pointer.move",
    "pointer.click",
    "pointer.scroll",
    "screen.capture",
    "clipboard.read",
    "clipboard.write",
];
pub struct Macos {
    transport: Arc<dyn Transport>,
}
impl Macos {
    pub async fn new(artifacts: &Path) -> Result<Self> {
        semwright_protocol::private_directory(artifacts)?;
        let t: Arc<dyn Transport> = Arc::new(NativeTransport::default());
        t.call(
            "configure",
            &json!({"artifact_directory":artifacts}),
            CancellationToken::new(),
        )
        .await?;
        Ok(Self { transport: t })
    }
    pub fn with_transport(transport: Arc<dyn Transport>) -> Self {
        Self { transport }
    }
}
#[async_trait]
impl Backend for Macos {
    fn name(&self) -> &'static str {
        "macos"
    }
    fn supports(&self, c: &str) -> bool {
        COMMANDS.contains(&c)
    }
    fn operation_feature(&self, c: &str) -> Option<String> {
        self.supports(c).then(|| c.to_owned())
    }
    async fn probe(&self) -> Vec<Feature> {
        let status = self
            .transport
            .call("permissions", &json!({}), CancellationToken::new())
            .await;
        COMMANDS
            .iter()
            .map(|c| {
                let available = status.as_ref().is_ok_and(|p| {
                    if c.starts_with("clipboard.") || *c == "app.list" {
                        true
                    } else if *c == "screen.capture" {
                        p["capture_picker"].as_bool() == Some(true)
                    } else if c.starts_with("input.") || c.starts_with("pointer.") {
                        p["accessibility"].as_bool() == Some(true)
                            && p["post_events"].as_bool() == Some(true)
                    } else {
                        p["accessibility"].as_bool() == Some(true)
                    }
                });
                let mut result = feature(
                    "macos",
                    c,
                    available,
                    "Public macOS API capability probe; not an acceptance certificate",
                    "Grant only the required permission manually; inspect MACOS_PERMISSIONS.md",
                );
                if *c == "screen.capture" && available {
                    result.status = semwright_types::CapabilityStatus::SupportedWithConsent;
                }
                result
            })
            .collect()
    }
    async fn execute(&self, ctx: &Context, c: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        if !self.supports(c) {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "macOS operation is not implemented",
            ));
        }
        self.transport.call(c, args, ctx.cancellation.clone()).await
    }
    async fn validate(&self, target: &NativeTarget) -> Result<()> {
        self.transport
            .call(
                "validate",
                &json!({"_target":target}),
                CancellationToken::new(),
            )
            .await
            .map(|_| ())
    }
    async fn is_focused(&self, target: &NativeTarget) -> Result<bool> {
        let result = self
            .transport
            .call(
                "focused",
                &json!({"_target":target}),
                CancellationToken::new(),
            )
            .await?;
        Ok(result["focused"].as_bool() == Some(true))
    }
    async fn shutdown(&self) -> Result<()> {
        self.transport
            .call("shutdown", &json!({}), CancellationToken::new())
            .await
            .map(|_| ())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Fake;
    #[async_trait]
    impl Transport for Fake {
        async fn call(&self, c: &str, _: &Value, stop: CancellationToken) -> Result<Value> {
            if stop.is_cancelled() {
                return Err(Error::new(ErrorCode::Cancelled, "cancelled"));
            }
            match c {
                "permissions" => {
                    Ok(json!({"accessibility":false,"post_events":false,"capture_picker":false}))
                }
                "app.list" => Ok(json!({"apps":[]})),
                _ => Err(Error::new(
                    ErrorCode::StaleReference,
                    "fixture ref is stale",
                )),
            }
        }
    }
    #[tokio::test]
    async fn denied_tcc_does_not_disable_unrelated_capabilities() {
        let b = Macos::with_transport(Arc::new(Fake));
        let p = b.probe().await;
        assert!(
            p.iter()
                .find(|p| p.capability == "app.list")
                .unwrap()
                .usable()
        );
        assert!(
            !p.iter()
                .find(|p| p.capability == "input.type")
                .unwrap()
                .usable()
        );
    }
    #[tokio::test]
    async fn unknown_never_falls_back() {
        let b = Macos::with_transport(Arc::new(Fake));
        let c = Context {
            session: "test".into(),
            request_id: semwright_types::unique_id(),
            cancellation: CancellationToken::new(),
        };
        assert_eq!(
            b.execute(&c, "ui.blind_click", &json!({}))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }
    #[tokio::test]
    async fn cancellation_before_dispatch() {
        let b = Macos::with_transport(Arc::new(Fake));
        let stop = CancellationToken::new();
        stop.cancel();
        let c = Context {
            session: "test".into(),
            request_id: semwright_types::unique_id(),
            cancellation: stop,
        };
        assert_eq!(
            b.execute(&c, "app.list", &json!({}))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
    }
}
