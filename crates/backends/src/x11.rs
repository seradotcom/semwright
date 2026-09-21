//! X11 EWMH window control and explicitly gated XTEST input. No xdotool dependency.
use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, feature};
use semwright_types::*;
use serde_json::{Value, json};
use std::sync::Mutex;
use x11rb::{
    CURRENT_TIME, NONE,
    connection::Connection,
    protocol::{xproto::*, xtest::ConnectionExt as _},
    rust_connection::RustConnection,
};
struct State {
    conn: RustConnection,
    root: u32,
}
pub struct X11 {
    state: Mutex<Option<State>>,
}
fn xerr<T>(result: std::result::Result<T, impl std::fmt::Debug>) -> Result<T> {
    result.map_err(|_| Error::new(ErrorCode::BackendFailed, "X11 request failed"))
}
impl Default for X11 {
    fn default() -> Self {
        let local_display = std::env::var("DISPLAY")
            .is_ok_and(|value| value.starts_with(':') || value.starts_with("unix/:"));
        let state = if !local_display
            || std::env::var("XDG_SESSION_TYPE").ok().as_deref() == Some("wayland")
        {
            None
        } else {
            x11rb::connect(None).ok().map(|(conn, screen)| {
                let root = conn.setup().roots[screen].root;
                State { conn, root }
            })
        };
        Self {
            state: Mutex::new(state),
        }
    }
}
impl State {
    fn atom(&self, name: &str) -> Result<u32> {
        Ok(xerr(xerr(self.conn.intern_atom(false, name.as_bytes()))?.reply())?.atom)
    }
    fn prop(&self, window: u32, name: &str, kind: AtomEnum) -> Result<GetPropertyReply> {
        let atom = self.atom(name)?;
        xerr(xerr(self.conn.get_property(false, window, atom, kind, 0, 65536))?.reply())
    }
    fn ids(&self) -> Result<Vec<u32>> {
        Ok(self
            .prop(self.root, "_NET_CLIENT_LIST", AtomEnum::WINDOW)?
            .value32()
            .map(|v| v.collect())
            .unwrap_or_default())
    }
    fn focused(&self) -> Result<Option<u32>> {
        Ok(self
            .prop(self.root, "_NET_ACTIVE_WINDOW", AtomEnum::WINDOW)?
            .value32()
            .and_then(|mut v| v.next()))
    }
    fn target(&self, id: u32) -> Result<NativeTarget> {
        let class = self.prop(id, "WM_CLASS", AtomEnum::STRING)?.value;
        let app = String::from_utf8_lossy(&class).replace('\0', "/");
        let pid = self
            .prop(id, "_NET_WM_PID", AtomEnum::CARDINAL)?
            .value32()
            .and_then(|mut v| v.next())
            .unwrap_or(0);
        Ok(NativeTarget {
            kind: "win".into(),
            identity: id.to_string(),
            revision: 0,
            fingerprint: format!("{pid}:{app}"),
            app,
        })
    }
    fn validate(&self, target: &NativeTarget) -> Result<u32> {
        let id = target
            .identity
            .parse::<u32>()
            .map_err(|_| Error::invalid("Invalid X11 reference"))?;
        if !self.ids()?.contains(&id) || self.target(id)?.fingerprint != target.fingerprint {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "X11 window identity changed",
            ));
        }
        Ok(id)
    }
    fn event(&self, id: u32, name: &str, data: [u32; 5]) -> Result<()> {
        let atom = self.atom(name)?;
        let event = ClientMessageEvent::new(32, id, atom, ClientMessageData::from(data));
        xerr(
            xerr(self.conn.send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            ))?
            .check(),
        )?;
        xerr(self.conn.flush())
    }
}
#[async_trait]
impl Backend for X11 {
    fn name(&self) -> &'static str {
        "x11"
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
                | "input.key"
                | "pointer.click"
                | "pointer.move"
        )
    }
    async fn probe(&self) -> Vec<Feature> {
        let ready = self
            .state
            .lock()
            .is_ok_and(|s| s.as_ref().is_some_and(|s| s.ids().is_ok()));
        vec![feature(
            self.name(),
            "window.manage",
            ready,
            "EWMH client-list query; XWayland is deliberately not a whole-desktop backend",
            "Use a native X11 session with an EWMH window manager",
        )]
    }
    async fn execute(&self, ctx: &Context, c: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        let guard = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "X11 lock poisoned"))?;
        let s = guard
            .as_ref()
            .ok_or_else(|| Error::unavailable("Native X11 connection unavailable"))?;
        if c == "window.list" {
            let focused = s.focused()?;
            let mut windows = vec![];
            for id in s.ids()? {
                let target = match s.target(id) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                let title = s
                    .prop(id, "_NET_WM_NAME", AtomEnum::ANY)
                    .map(|p| String::from_utf8_lossy(&p.value).into_owned())
                    .unwrap_or_default();
                windows.push(json!({"ref":target_marker(target.clone()),"app":target.app,"title":title,"focused":focused==Some(id)}));
            }
            return Ok(json!({"windows":windows}));
        }
        let target = native_target(args)?;
        let id = s.validate(&target)?;
        if (c.starts_with("input.") || c.starts_with("pointer.")) && s.focused()? != Some(id) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "X11 focus changed; input not sent",
            ));
        }
        match c {
            "window.focus" => s.event(id, "_NET_ACTIVE_WINDOW", [2, CURRENT_TIME, 0, 0, 0])?,
            "window.close" | "app.close" => {
                s.event(id, "_NET_CLOSE_WINDOW", [CURRENT_TIME, 2, 0, 0, 0])?
            }
            "window.move" => s.event(
                id,
                "_NET_MOVERESIZE_WINDOW",
                [
                    (1 << 8) | (1 << 9) | (2 << 12),
                    args["x"]
                        .as_i64()
                        .ok_or_else(|| Error::invalid("x required"))? as u32,
                    args["y"]
                        .as_i64()
                        .ok_or_else(|| Error::invalid("y required"))? as u32,
                    0,
                    0,
                ],
            )?,
            "window.resize" => s.event(
                id,
                "_NET_MOVERESIZE_WINDOW",
                [
                    (1 << 10) | (1 << 11) | (2 << 12),
                    0,
                    0,
                    args["width"]
                        .as_u64()
                        .ok_or_else(|| Error::invalid("width required"))?
                        as u32,
                    args["height"]
                        .as_u64()
                        .ok_or_else(|| Error::invalid("height required"))?
                        as u32,
                ],
            )?,
            "pointer.click" => {
                let button = match arg_str(args, "button")? {
                    "left" => 1,
                    "middle" => 2,
                    "right" => 3,
                    _ => return Err(Error::invalid("Unknown pointer button")),
                };
                xerr(
                    xerr(s.conn.xtest_fake_input(
                        BUTTON_PRESS_EVENT,
                        button,
                        CURRENT_TIME,
                        NONE,
                        0,
                        0,
                        0,
                    ))?
                    .check(),
                )?;
                xerr(
                    xerr(s.conn.xtest_fake_input(
                        BUTTON_RELEASE_EVENT,
                        button,
                        CURRENT_TIME,
                        NONE,
                        0,
                        0,
                        0,
                    ))?
                    .check(),
                )?;
            }
            "pointer.move" => {
                let p = xerr(xerr(s.conn.query_pointer(s.root))?.reply())?;
                let x = (p.root_x as f64
                    + args["dx"]
                        .as_f64()
                        .ok_or_else(|| Error::invalid("dx required"))?)
                .round()
                .clamp(i16::MIN as f64, i16::MAX as f64) as i16;
                let y = (p.root_y as f64
                    + args["dy"]
                        .as_f64()
                        .ok_or_else(|| Error::invalid("dy required"))?)
                .round()
                .clamp(i16::MIN as f64, i16::MAX as f64) as i16;
                xerr(
                    xerr(s.conn.xtest_fake_input(
                        MOTION_NOTIFY_EVENT,
                        0,
                        CURRENT_TIME,
                        s.root,
                        x,
                        y,
                        0,
                    ))?
                    .check(),
                )?;
            }
            "input.key" => {
                let keysym = args["keysym"]
                    .as_u64()
                    .ok_or_else(|| Error::invalid("keysym required"))?
                    as u32;
                let setup = s.conn.setup();
                let first = setup.min_keycode;
                let count = setup.max_keycode - first + 1;
                let mapping = xerr(xerr(s.conn.get_keyboard_mapping(first, count))?.reply())?;
                let width = mapping.keysyms_per_keycode as usize;
                // Only unshifted keysyms: never remap the user's global keymap.
                let index=mapping.keysyms.chunks(width).position(|keys|keys.first()==Some(&keysym)).ok_or_else(||Error::new(ErrorCode::Unsupported,"Keysym requires modifiers or keymap mutation; use semantic text/portal input"))?;
                let key = first + index as u8;
                xerr(
                    xerr(s.conn.xtest_fake_input(
                        KEY_PRESS_EVENT,
                        key,
                        CURRENT_TIME,
                        NONE,
                        0,
                        0,
                        0,
                    ))?
                    .check(),
                )?;
                xerr(
                    xerr(s.conn.xtest_fake_input(
                        KEY_RELEASE_EVENT,
                        key,
                        CURRENT_TIME,
                        NONE,
                        0,
                        0,
                        0,
                    ))?
                    .check(),
                )?;
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Unsupported X11 command",
                ));
            }
        }
        xerr(s.conn.flush())?;
        Ok(json!({"accepted":true,"coordinate_space":"x11_root_pixels"}))
    }
    async fn validate(&self, t: &NativeTarget) -> Result<()> {
        let g = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "X11 lock poisoned"))?;
        g.as_ref()
            .ok_or_else(|| Error::unavailable("X11 unavailable"))?
            .validate(t)?;
        Ok(())
    }
    async fn is_focused(&self, t: &NativeTarget) -> Result<bool> {
        let g = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "X11 lock poisoned"))?;
        let s = g
            .as_ref()
            .ok_or_else(|| Error::unavailable("X11 unavailable"))?;
        Ok(s.focused()? == Some(s.validate(t)?))
    }
}
