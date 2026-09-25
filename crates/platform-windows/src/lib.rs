//! Native Windows desktop backend for the existing Semwright broker.
//! UI Automation lives on one dedicated COM thread; no UIA interface crosses into Tokio.
#![cfg(target_os = "windows")]
pub mod events;
pub mod roles;
pub mod uia;

use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, ProviderSignal, feature};
use semwright_platform_windows_sys::{capture, clipboard, input, window};
use semwright_types::{CapabilityStatus, Error, ErrorCode, Feature, NativeTarget, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::broadcast;
use uia::UiaActor;
use windows::Win32::Foundation::HWND;

pub const COMMANDS: &[&str] = &[
    "app.list",
    "window.list",
    "window.focus",
    "window.move",
    "window.resize",
    "window.close",
    "ui.snapshot",
    "ui.hit_test",
    "ui.inspect",
    "ui.invoke",
    "ui.set_text",
    "ui.read_text",
    "ui.set_value",
    "ui.get_value",
    "ui.toggle",
    "ui.select",
    "ui.expand",
    "input.type",
    "pointer.move",
    "pointer.click",
    "pointer.scroll",
    "screen.capture",
    "clipboard.read",
    "clipboard.write",
];

#[derive(Clone, Debug)]
struct WindowStamp {
    hwnd: isize,
    pid: u32,
    process_start: u64,
    title: String,
    revision: u64,
}

pub struct Windows {
    uia: UiaActor,
    artifacts: PathBuf,
    windows: Arc<Mutex<BTreeMap<String, WindowStamp>>>,
    next_revision: Arc<Mutex<u64>>,
    signals: broadcast::Sender<ProviderSignal>,
}

fn hash_window(pid: u32, start: u64, hwnd: isize, title: &str) -> String {
    let mut h = Sha256::new();
    h.update(pid.to_le_bytes());
    h.update(start.to_le_bytes());
    h.update(hwnd.to_le_bytes());
    h.update(title.as_bytes());
    let mut s = String::with_capacity(64);
    for b in h.finalize() {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn target(args: &Value) -> Result<NativeTarget> {
    let value = args
        .get("_target")
        .or_else(|| args.get("target"))
        .ok_or_else(|| {
            Error::invalid("Windows semantic operation requires a revalidatable target reference")
        })?;
    serde_json::from_value(value.clone())
        .map_err(|_| Error::invalid("Malformed Windows target reference"))
}

fn integer_arg(args: &Value, key: &str, minimum: i64, maximum: i64) -> Result<i32> {
    let value = args
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| Error::invalid(format!("{key} required")))?;
    if !(minimum..=maximum).contains(&value) {
        return Err(Error::invalid(format!("{key} outside contract bounds")));
    }
    i32::try_from(value).map_err(|_| Error::invalid(format!("{key} out of range")))
}

fn delta_arg(args: &Value, key: &str, limit: f64) -> Result<i32> {
    let value = args
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| Error::invalid(format!("{key} required")))?;
    if !value.is_finite() || value.abs() > limit {
        return Err(Error::invalid(format!("{key} outside contract bounds")));
    }
    Ok(value.round() as i32)
}

impl Windows {
    pub fn new(artifacts: &Path) -> Result<Self> {
        semwright_platform_windows_sys::dll::harden_default_dll_search()?;
        semwright_platform_windows_sys::paths::ensure_owner_only_directory(artifacts)?;
        let (signals, _) = broadcast::channel(512);
        Ok(Self {
            uia: UiaActor::start(signals.clone())?,
            artifacts: artifacts.to_path_buf(),
            windows: Arc::new(Mutex::new(BTreeMap::new())),
            next_revision: Arc::new(Mutex::new(0)),
            signals,
        })
    }

    fn remember_window(&self, w: &window::NativeWindow) -> Result<NativeTarget> {
        let start = window::process_creation_time(w.pid)?;
        let id = format!(
            "win:{}",
            hash_window(w.pid, start, w.hwnd.0 as isize, &w.title)
        );
        let mut r = self
            .next_revision
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Windows ref lock poisoned"))?;
        *r = r.saturating_add(1);
        let revision = *r;
        let stamp = WindowStamp {
            hwnd: w.hwnd.0 as isize,
            pid: w.pid,
            process_start: start,
            title: w.title.clone(),
            revision,
        };
        self.windows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Windows ref lock poisoned"))?
            .insert(id.clone(), stamp);
        Ok(NativeTarget {
            kind: "win".into(),
            identity: id.clone(),
            revision,
            fingerprint: id.trim_start_matches("win:").into(),
            app: format!("pid:{}", w.pid),
        })
    }

    fn resolve_window(&self, t: &NativeTarget) -> Result<HWND> {
        let stamp = self
            .windows
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Windows ref lock poisoned"))?
            .get(&t.identity)
            .cloned()
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::StaleReference,
                    "Unknown Windows window reference",
                )
            })?;
        if stamp.revision != t.revision
            || window::process_creation_time(stamp.pid)? != stamp.process_start
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Windows process/window generation changed",
            ));
        }
        let hwnd = HWND(stamp.hwnd as *mut core::ffi::c_void);
        let live = window::enumerate()?
            .into_iter()
            .find(|w| w.hwnd == hwnd && w.pid == stamp.pid && w.title == stamp.title)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::StaleReference,
                    "Windows HWND was destroyed or reused",
                )
            })?;
        let _ = live;
        Ok(hwnd)
    }

    fn list_windows(&self) -> Result<Value> {
        let mut out = Vec::new();
        for w in window::enumerate()? {
            let reference = self.remember_window(&w)?;
            let mut value = window::to_json(&w);
            value["ref"] = json!({"$ref": reference});
            out.push(value);
        }
        Ok(json!({"windows":out}))
    }

    fn list_apps(&self) -> Result<Value> {
        let windows = window::enumerate()?;
        let mut apps: BTreeMap<(u32, String), Value> = BTreeMap::new();
        for w in windows {
            let image = w
                .image
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            apps.entry((w.pid, image.clone()))
                .or_insert_with(|| json!({"pid":w.pid,"image":image}));
        }
        Ok(json!({"apps":apps.into_values().collect::<Vec<_>>() }))
    }

    fn ensure_synthetic_target_focused(&self, t: &NativeTarget) -> Result<()> {
        self.validate_sync(t)?;
        if t.identity.starts_with("uia:") {
            if !self.uia.focused(t.clone())? {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Focus drift detected before Windows synthetic input",
                ));
            }
        } else {
            let hwnd = self.resolve_window(t)?;
            if window::foreground() != Some(hwnd) {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Focus drift detected before Windows synthetic input",
                ));
            }
        }
        Ok(())
    }

    fn validate_sync(&self, t: &NativeTarget) -> Result<()> {
        if t.identity.starts_with("uia:") {
            self.uia.validate(t.clone())
        } else if t.identity.starts_with("win:") {
            self.resolve_window(t).map(|_| ())
        } else {
            Err(Error::new(
                ErrorCode::StaleReference,
                "Reference does not belong to Windows backend",
            ))
        }
    }
}

#[async_trait]
impl Backend for Windows {
    fn name(&self) -> &'static str {
        "windows"
    }
    fn supports(&self, command: &str) -> bool {
        COMMANDS.contains(&command)
    }
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| command.to_owned())
    }

    fn events(&self) -> Option<broadcast::Receiver<ProviderSignal>> {
        self.uia.events_active().then(|| self.signals.subscribe())
    }

    async fn probe(&self) -> Vec<Feature> {
        COMMANDS.iter().map(|c| {
            if *c == "screen.capture" {
                let supported = capture::capture_supported();
                let mut f = feature(
                    "windows",
                    c,
                    supported,
                    if supported {
                        "Windows.Graphics.Capture system picker and bounded D3D11 readback are available"
                    } else {
                        "Windows.Graphics.Capture is unavailable on this session"
                    },
                    "Capture requires explicit end-user selection in an interactive Windows session",
                );
                if !supported {
                    f.status = CapabilityStatus::Unavailable;
                }
                f
            } else {
                feature("windows", c, true, "Public Windows API implementation present; probe is not an acceptance certificate", "Run native Windows CI and interactive acceptance")
            }
        }).collect()
    }

    async fn execute(&self, ctx: &Context, command: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        if !self.supports(command) {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Windows operation is not implemented",
            ));
        }
        match command {
            "app.list" => self.list_apps(),
            "window.list" => self.list_windows(),
            "window.focus" => {
                let h = self.resolve_window(&target(args)?)?;
                window::focus(h)?;
                Ok(json!({"focused":true}))
            }
            "window.move" => {
                let h = self.resolve_window(&target(args)?)?;
                let x = integer_arg(args, "x", -32_768, 32_767)?;
                let y = integer_arg(args, "y", -32_768, 32_767)?;
                window::move_window(h, x, y)?;
                Ok(json!({"applied":true}))
            }
            "window.resize" => {
                let h = self.resolve_window(&target(args)?)?;
                let width = integer_arg(args, "width", 1, 16_384)?;
                let height = integer_arg(args, "height", 1, 16_384)?;
                window::resize_window(h, width, height)?;
                Ok(json!({"applied":true}))
            }
            "window.close" => {
                let h = self.resolve_window(&target(args)?)?;
                window::close(h)?;
                Ok(json!({"requested":true,"delivery_verified":false}))
            }
            "ui.snapshot" => self.uia.snapshot(
                args.get("_target")
                    .and_then(|v| serde_json::from_value(v.clone()).ok()),
            ),
            "ui.inspect" => self.uia.inspect(target(args)?),
            "ui.hit_test" => {
                let x = args
                    .get("x")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| Error::invalid("x required"))?;
                let y = args
                    .get("y")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| Error::invalid("y required"))?;
                if !(-1_000_000..=1_000_000).contains(&x) || !(-1_000_000..=1_000_000).contains(&y)
                {
                    return Err(Error::invalid("UI hit-test coordinates exceed budget"));
                }
                self.uia.hit_test(x as i32, y as i32)
            }
            "ui.invoke" => self.uia.invoke(target(args)?),
            "ui.set_text" => self.uia.set_text(
                target(args)?,
                args.get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::invalid("text required"))?
                    .into(),
            ),
            "ui.read_text" => self.uia.read_text(target(args)?),
            "ui.set_value" => self.uia.set_value(
                target(args)?,
                args.get("value")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| Error::invalid("numeric value required"))?,
            ),
            "ui.get_value" => self.uia.get_value(target(args)?),
            "ui.toggle" => self.uia.toggle(target(args)?),
            "ui.select" => self.uia.select(
                target(args)?,
                args.get("index")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| Error::invalid("selection index required"))?
                    .try_into()
                    .map_err(|_| Error::invalid("selection index out of range"))?,
            ),
            "ui.expand" => self.uia.expand(
                target(args)?,
                args.get("expand").and_then(Value::as_bool).unwrap_or(true),
            ),
            "input.type" => {
                let t = target(args)?;
                self.ensure_synthetic_target_focused(&t)?;
                input::type_unicode(
                    args.get("text")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::invalid("text required"))?,
                )?;
                Ok(json!({"sent":true}))
            }
            "pointer.move" => {
                let t = target(args)?;
                self.ensure_synthetic_target_focused(&t)?;
                let dx = delta_arg(args, "dx", 10_000.0)?;
                let dy = delta_arg(args, "dy", 10_000.0)?;
                input::mouse_move_relative(dx, dy)?;
                Ok(json!({"sent":true}))
            }
            "pointer.click" => {
                let t = target(args)?;
                self.ensure_synthetic_target_focused(&t)?;
                input::click(
                    args.get("button")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::invalid("button required"))?,
                )?;
                Ok(json!({"sent":true}))
            }
            "pointer.scroll" => {
                let t = target(args)?;
                self.ensure_synthetic_target_focused(&t)?;
                let dx = delta_arg(args, "dx", 1_000.0)?;
                let dy = delta_arg(args, "dy", 1_000.0)?;
                input::scroll(dx, dy)?;
                Ok(json!({"sent":true}))
            }
            "clipboard.read" => {
                let max_bytes = args
                    .get("max_bytes")
                    .and_then(Value::as_u64)
                    .unwrap_or(1_048_576)
                    .min(1_048_576) as usize;
                let text = clipboard::read_text()?;
                if text.len() > max_bytes {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "Clipboard text exceeds requested max_bytes",
                    ));
                }
                Ok(json!({"text":text,"bytes":text.len()}))
            }
            "clipboard.write" => {
                let text = args
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::invalid("text required"))?;
                clipboard::write_text(text)?;
                Ok(json!({"written":true,"bytes":text.len()}))
            }
            "screen.capture" => {
                let artifacts = self.artifacts.clone();
                let cancellation = ctx.cancellation.clone();
                let artifact = tokio::task::spawn_blocking(move || {
                    capture::pick_and_capture_artifact(&artifacts, Duration::from_secs(110), || {
                        cancellation.is_cancelled()
                    })
                })
                .await
                .map_err(|_| {
                    Error::new(
                        ErrorCode::BackendFailed,
                        "Windows capture worker terminated unexpectedly",
                    )
                })??;
                if let Err(error) = ctx.check_cancelled() {
                    let _ = tokio::fs::remove_file(&artifact.path).await;
                    return Err(error);
                }
                let cleanup = artifact.path.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    let _ = tokio::fs::remove_file(cleanup).await;
                });
                Ok(json!({
                    "path": artifact.path,
                    "mime_type": "image/png",
                    "bytes": artifact.bytes,
                    "width": artifact.width,
                    "height": artifact.height,
                    "expires_in_seconds": 60,
                    "selection": "system_content_picker",
                    "coordinate_space": "windows_graphics_capture_item_pixels",
                    "warning": "The user-selected capture may contain sensitive content; image bytes are never audited"
                }))
            }
            _ => Err(Error::new(
                ErrorCode::Unsupported,
                "Windows operation is not implemented",
            )),
        }
    }

    async fn validate(&self, target: &NativeTarget) -> Result<()> {
        self.validate_sync(target)
    }
    async fn is_focused(&self, target: &NativeTarget) -> Result<bool> {
        if target.identity.starts_with("uia:") {
            self.uia.focused(target.clone())
        } else {
            Ok(window::foreground() == Some(self.resolve_window(target)?))
        }
    }
    async fn shutdown(&self) -> Result<()> {
        self.uia.shutdown()
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numeric_helpers_enforce_portable_contract_bounds() {
        assert_eq!(
            integer_arg(&json!({"x": -32768}), "x", -32768, 32767).unwrap(),
            -32768
        );
        assert!(integer_arg(&json!({"x": 32768}), "x", -32768, 32767).is_err());
        assert_eq!(delta_arg(&json!({"dx": 2.6}), "dx", 10_000.0).unwrap(), 3);
        assert!(delta_arg(&json!({"dx": 10_001.0}), "dx", 10_000.0).is_err());
        assert!(delta_arg(&json!({"dx": f64::NAN}), "dx", 10_000.0).is_err());
    }

    #[test]
    fn unsupported_virtual_key_command_is_not_advertised() {
        assert!(!COMMANDS.contains(&"input.key"));
        assert!(COMMANDS.contains(&"input.type"));
        assert!(COMMANDS.contains(&"pointer.move"));
        assert!(COMMANDS.contains(&"pointer.click"));
        assert!(COMMANDS.contains(&"pointer.scroll"));
    }
}
