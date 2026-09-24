//! Native Windows desktop backend for the existing Semwright broker.
//! UI Automation lives on one dedicated COM thread; no UIA interface crosses into Tokio.
#![cfg(target_os = "windows")]
pub mod events;
pub mod roles;
pub mod uia;

use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, feature};
use semwright_platform_windows_sys::{capture, clipboard, input, window};
use semwright_types::{CapabilityStatus, Error, ErrorCode, Feature, NativeTarget, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
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
    "ui.invoke",
    "ui.set_text",
    "ui.read_text",
    "ui.set_value",
    "ui.get_value",
    "ui.toggle",
    "ui.select",
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
    windows: Arc<Mutex<BTreeMap<String, WindowStamp>>>,
    next_revision: Arc<Mutex<u64>>,
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

impl Windows {
    pub fn new() -> Result<Self> {
        semwright_platform_windows_sys::dll::harden_default_dll_search()?;
        Ok(Self {
            uia: UiaActor::start()?,
            windows: Arc::new(Mutex::new(BTreeMap::new())),
            next_revision: Arc::new(Mutex::new(0)),
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

    async fn probe(&self) -> Vec<Feature> {
        COMMANDS.iter().map(|c| {
            if *c == "screen.capture" {
                let mut f = feature("windows", c, false,
                    if capture::capture_supported() { "Windows.Graphics.Capture is present; bounded D3D11 single-frame readback is not yet native-verified" } else { "Windows.Graphics.Capture is unavailable on this session" },
                    "Run LIVE_WINDOWS_TEST_MATRIX.md on an interactive Windows 11 session before enabling capture");
                f.status = CapabilityStatus::Unavailable;
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
            "window.move" | "window.resize" => {
                let h = self.resolve_window(&target(args)?)?;
                let x = args.get("x").and_then(Value::as_i64).unwrap_or(0) as i32;
                let y = args.get("y").and_then(Value::as_i64).unwrap_or(0) as i32;
                let width = args
                    .get("width")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| Error::invalid("width required"))?
                    as i32;
                let height = args
                    .get("height")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| Error::invalid("height required"))?
                    as i32;
                window::move_resize(h, x, y, width, height)?;
                Ok(json!({"changed":true}))
            }
            "window.close" => {
                let h = self.resolve_window(&target(args)?)?;
                window::close(h)?;
                Ok(json!({"requested":true}))
            }
            "ui.snapshot" => self.uia.snapshot(
                args.get("_target")
                    .and_then(|v| serde_json::from_value(v.clone()).ok()),
            ),
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
            "ui.select" => self.uia.select(target(args)?),
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
                Ok(json!({"typed":true}))
            }
            "input.key" => Err(Error::new(
                ErrorCode::Unsupported,
                "Windows virtual-key fallback is not enabled in this source drop; use semantic UIA actions or input.type",
            )),
            "pointer.move" => {
                let t = target(args)?;
                self.ensure_synthetic_target_focused(&t)?;
                let x = args
                    .get("normalized_x")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| Error::invalid("normalized_x required"))?
                    as i32;
                let y = args
                    .get("normalized_y")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| Error::invalid("normalized_y required"))?
                    as i32;
                input::mouse_move_absolute(x, y)?;
                Ok(json!({"moved":true}))
            }
            "pointer.click" => {
                let t = target(args)?;
                self.ensure_synthetic_target_focused(&t)?;
                input::left_click()?;
                Ok(json!({"clicked":true}))
            }
            "pointer.scroll" => {
                let t = target(args)?;
                self.ensure_synthetic_target_focused(&t)?;
                input::wheel(
                    args.get("delta")
                        .and_then(Value::as_i64)
                        .ok_or_else(|| Error::invalid("delta required"))?
                        as i32,
                )?;
                Ok(json!({"scrolled":true}))
            }
            "clipboard.read" => Ok(json!({"text":clipboard::read_text()?})),
            "clipboard.write" => {
                clipboard::write_text(
                    args.get("text")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::invalid("text required"))?,
                )?;
                Ok(json!({"written":true}))
            }
            "screen.capture" => Err(Error::new(
                ErrorCode::Unavailable,
                "Windows.Graphics.Capture target acquisition exists, but D3D11 single-frame readback remains fail-closed pending native verification",
            )),
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
