//! Hyprland's local socket API with typed, injection-resistant command construction.
use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, feature};
use semwright_types::*;
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
pub struct Hyprland {
    socket: Option<PathBuf>,
}
impl Default for Hyprland {
    fn default() -> Self {
        let socket = std::env::var_os("XDG_RUNTIME_DIR")
            .zip(std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok())
            .and_then(|(root, signature)| {
                if signature.len() > 200
                    || !signature
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                {
                    return None;
                }
                Some(
                    PathBuf::from(root)
                        .join("hypr")
                        .join(signature)
                        .join(".socket.sock"),
                )
            });
        Self { socket }
    }
}
pub fn validate_address(address: &str) -> Result<&str> {
    if address.len() > 18
        || !address.starts_with("0x")
        || address.len() < 3
        || !address[2..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(Error::invalid("Invalid Hyprland address"));
    }
    Ok(address)
}
impl Hyprland {
    async fn request(&self, command: &str) -> Result<String> {
        let socket = self
            .socket
            .as_ref()
            .ok_or_else(|| Error::unavailable("Hyprland socket is not configured"))?;
        let mut stream = UnixStream::connect(socket).await?;
        // SAFETY: getuid has no pointer arguments or additional preconditions.
        if stream.peer_cred()?.uid() != unsafe { libc::getuid() } {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Compositor owner mismatch",
            ));
        }
        stream.write_all(command.as_bytes()).await?;
        stream.shutdown().await?;
        let mut data = Vec::new();
        stream.take(4_194_305).read_to_end(&mut data).await?;
        if data.len() > 4_194_304 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Hyprland response exceeds budget",
            ));
        }
        String::from_utf8(data)
            .map_err(|_| Error::new(ErrorCode::BackendFailed, "Invalid compositor encoding"))
    }
    async fn clients(&self) -> Result<Vec<Value>> {
        serde_json::from_str(&self.request("j/clients").await?).map_err(Into::into)
    }
    fn target(v: &Value) -> Result<NativeTarget> {
        let address = validate_address(arg_str(v, "address")?)?;
        Ok(NativeTarget {
            kind: "win".into(),
            identity: address.into(),
            revision: 0,
            fingerprint: format!("{}:{}", v["pid"], v["initialClass"]),
            app: v["class"].as_str().unwrap_or("").into(),
        })
    }
}
#[async_trait]
impl Backend for Hyprland {
    fn name(&self) -> &'static str {
        "hyprland"
    }
    fn supports(&self, c: &str) -> bool {
        matches!(
            c,
            "window.list"
                | "window.focus"
                | "window.move"
                | "window.resize"
                | "window.close"
                | "app.close"
        )
    }
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| "window.manage".to_owned())
    }
    async fn probe(&self) -> Vec<Feature> {
        let ready =
            tokio::time::timeout(std::time::Duration::from_secs(2), self.request("j/version"))
                .await
                .is_ok_and(|r| r.is_ok());
        vec![feature(
            self.name(),
            "window.manage",
            ready,
            "Native Hyprland socket probe",
            "Run within the compositor login session",
        )]
    }
    async fn execute(&self, ctx: &Context, c: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        if c == "window.list" {
            let active: Value = serde_json::from_str(&self.request("j/activewindow").await?)?;
            let mut windows = vec![];
            for v in self.clients().await? {
                windows.push(json!({"ref":target_marker(Self::target(&v)?),"title":v["title"],"app":v["class"],"focused":v["address"]==active["address"],"position":v["at"],"size":v["size"],"coordinate_space":"compositor_logical"}));
            }
            return Ok(json!({"windows":windows}));
        }
        let target = native_target(args)?;
        self.validate(&target).await?;
        let address = validate_address(&target.identity)?;
        let command = match c {
            "window.focus" => format!("dispatch focuswindow address:{address}"),
            "window.close" | "app.close" => format!("dispatch closewindow address:{address}"),
            "window.move" => format!(
                "dispatch movewindowpixel exact {} {},address:{address}",
                args["x"]
                    .as_i64()
                    .ok_or_else(|| Error::invalid("x required"))?,
                args["y"]
                    .as_i64()
                    .ok_or_else(|| Error::invalid("y required"))?
            ),
            "window.resize" => format!(
                "dispatch resizewindowpixel exact {} {},address:{address}",
                args["width"]
                    .as_u64()
                    .ok_or_else(|| Error::invalid("width required"))?,
                args["height"]
                    .as_u64()
                    .ok_or_else(|| Error::invalid("height required"))?
            ),
            _ => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Unsupported Hyprland command",
                ));
            }
        };
        ctx.check_cancelled()?;
        if self
            .request(&command)
            .await
            .map_err(Error::uncertain)?
            .trim()
            != "ok"
        {
            return Err(
                Error::new(ErrorCode::BackendFailed, "Hyprland rejected operation").uncertain(),
            );
        }
        Ok(json!({"accepted":true,"coordinate_space":"compositor_logical"}))
    }
    async fn validate(&self, t: &NativeTarget) -> Result<()> {
        for c in self.clients().await? {
            let now = Self::target(&c)?;
            if now.identity == t.identity && now.fingerprint == t.fingerprint {
                return Ok(());
            }
        }
        Err(Error::new(
            ErrorCode::StaleReference,
            "Hyprland client disappeared or changed identity",
        ))
    }
    async fn is_focused(&self, t: &NativeTarget) -> Result<bool> {
        let v: Value = serde_json::from_str(&self.request("j/activewindow").await?)?;
        Ok(Self::target(&v)
            .is_ok_and(|now| now.identity == t.identity && now.fingerprint == t.fingerprint))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    #[test]
    fn rejects_dispatch_injection() {
        for s in ["0x12;exec evil", "0x", "12", "0xzz", "0x12\n"] {
            assert!(validate_address(s).is_err());
        }
    }
    proptest! {#[test]fn hex_addresses_pass(v in any::<u64>()){let s=format!("0x{v:x}");prop_assert!(validate_address(&s).is_ok());}}
}
