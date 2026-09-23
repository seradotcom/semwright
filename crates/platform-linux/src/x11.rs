//! Native X11 EWMH/XTEST backend.
//!
//! x11rb is synchronous. Every server roundtrip therefore crosses a single bounded
//! spawn_blocking gate. If an X server stops replying, the detached worker keeps
//! the sole permit, so subsequent requests fail closed instead of consuming Tokio
//! workers without bound.
//!
//! Window refs carry lifecycle epochs. The epoch changes when a client disappears,
//! reappears, receives an observed Create/Destroy event, or its conservative
//! pid/class fingerprint changes. X11 numeric IDs alone are never treated as stable.
use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, feature};
use semwright_types::*;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use x11rb::{
    CURRENT_TIME, NONE,
    connection::Connection,
    protocol::{Event, xproto::*, xtest::ConnectionExt as _},
    rust_connection::RustConnection,
};

const X11_BLOCKING_TIMEOUT: Duration = Duration::from_secs(3);

fn xerr<T>(result: std::result::Result<T, impl std::fmt::Debug>) -> Result<T> {
    result.map_err(|_| Error::new(ErrorCode::BackendFailed, "X11 request failed"))
}

#[derive(Debug, Clone)]
struct LifeEntry {
    epoch: u64,
    present: bool,
    fingerprint: Option<String>,
}

#[derive(Debug, Default)]
struct Lifecycle {
    next_epoch: u64,
    entries: HashMap<u32, LifeEntry>,
}

impl Lifecycle {
    fn bump(&mut self) -> Result<u64> {
        self.next_epoch = self.next_epoch.checked_add(1).ok_or_else(|| {
            Error::new(
                ErrorCode::ResourceExhausted,
                "X11 lifecycle epoch exhausted",
            )
        })?;
        Ok(self.next_epoch)
    }

    fn created(&mut self, id: u32) -> Result<()> {
        let epoch = self.bump()?;
        self.entries.insert(
            id,
            LifeEntry {
                epoch,
                present: true,
                fingerprint: None,
            },
        );
        Ok(())
    }

    fn destroyed(&mut self, id: u32) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.present = false;
            entry.fingerprint = None;
        }
    }

    fn observe(&mut self, id: u32, fingerprint: &str) -> Result<u64> {
        let needs_epoch = match self.entries.get(&id) {
            None => true,
            Some(entry) if !entry.present => true,
            Some(entry)
                if entry
                    .fingerprint
                    .as_deref()
                    .is_some_and(|old| old != fingerprint) =>
            {
                true
            }
            _ => false,
        };
        if needs_epoch {
            let epoch = self.bump()?;
            self.entries.insert(
                id,
                LifeEntry {
                    epoch,
                    present: true,
                    fingerprint: Some(fingerprint.to_owned()),
                },
            );
            return Ok(epoch);
        }
        let entry = self.entries.get_mut(&id).expect("entry checked above");
        entry.present = true;
        if entry.fingerprint.is_none() {
            entry.fingerprint = Some(fingerprint.to_owned());
        }
        Ok(entry.epoch)
    }

    fn reconcile(&mut self, observed: &[(u32, String)]) -> Result<()> {
        let ids: HashSet<u32> = observed.iter().map(|(id, _)| *id).collect();
        for (id, entry) in &mut self.entries {
            if entry.present && !ids.contains(id) {
                entry.present = false;
                entry.fingerprint = None;
            }
        }
        for (id, fingerprint) in observed {
            self.observe(*id, fingerprint)?;
        }
        Ok(())
    }

    fn epoch(&self, id: u32) -> Option<u64> {
        self.entries
            .get(&id)
            .filter(|entry| entry.present)
            .map(|entry| entry.epoch)
    }
}

#[derive(Clone)]
struct BlockingBoundary {
    gate: Arc<Semaphore>,
    timeout: Duration,
}

impl BlockingBoundary {
    fn new(timeout: Duration) -> Self {
        Self {
            gate: Arc::new(Semaphore::new(1)),
            timeout,
        }
    }

    async fn run<T, F>(
        &self,
        cancellation: CancellationToken,
        mutation: bool,
        operation: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        if cancellation.is_cancelled() {
            return Err(Error::new(ErrorCode::Cancelled, "X11 operation cancelled"));
        }
        let permit = self.gate.clone().try_acquire_owned().map_err(|_| {
            Error::new(
                ErrorCode::ResourceExhausted,
                "X11 blocking worker is busy; a previous server request may be unresponsive",
            )
        })?;
        let mut task = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            operation()
        });
        let timeout = tokio::time::sleep(self.timeout);
        tokio::pin!(timeout);
        tokio::select! {
            joined = &mut task => {
                joined.map_err(|_| Error::new(ErrorCode::Internal, "X11 blocking worker panicked"))?
            }
            _ = cancellation.cancelled() => {
                let error = Error::new(
                    ErrorCode::Cancelled,
                    "X11 operation cancelled after dispatch",
                );
                Err(if mutation { error.uncertain() } else { error })
            }
            _ = &mut timeout => {
                let error = Error::new(
                    ErrorCode::Timeout,
                    "X11 server did not answer before the blocking deadline",
                );
                Err(if mutation { error.uncertain() } else { error })
            }
        }
    }
}

struct State {
    conn: RustConnection,
    root: u32,
    lifecycle: Lifecycle,
}

impl State {
    fn connect() -> Result<Self> {
        let (conn, screen) = x11rb::connect(None)
            .map_err(|_| Error::unavailable("Native X11 connection unavailable"))?;
        let root = conn
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| Error::new(ErrorCode::BackendFailed, "X11 screen disappeared"))?
            .root;
        xerr(
            xerr(conn.change_window_attributes(
                root,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY),
            ))?
            .check(),
        )?;
        xerr(conn.flush())?;
        Ok(Self {
            conn,
            root,
            lifecycle: Lifecycle::default(),
        })
    }

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
            .map(|values| values.collect())
            .unwrap_or_default())
    }

    fn focused(&self) -> Result<Option<u32>> {
        Ok(self
            .prop(self.root, "_NET_ACTIVE_WINDOW", AtomEnum::WINDOW)?
            .value32()
            .and_then(|mut values| values.next()))
    }

    fn fingerprint(&self, id: u32) -> Result<(String, String)> {
        let class = self.prop(id, "WM_CLASS", AtomEnum::STRING)?.value;
        let app = String::from_utf8_lossy(&class).replace('\0', "/");
        let pid = self
            .prop(id, "_NET_WM_PID", AtomEnum::CARDINAL)?
            .value32()
            .and_then(|mut values| values.next())
            .unwrap_or(0);
        Ok((app.clone(), format!("{pid}:{app}")))
    }

    fn drain_lifecycle_events(&mut self) -> Result<()> {
        while let Some(event) = xerr(self.conn.poll_for_event())? {
            match event {
                Event::CreateNotify(event) => self.lifecycle.created(event.window)?,
                Event::DestroyNotify(event) => self.lifecycle.destroyed(event.window),
                Event::ReparentNotify(event) => self.lifecycle.created(event.window)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn targets(&mut self) -> Result<Vec<(u32, NativeTarget)>> {
        self.drain_lifecycle_events()?;
        let ids = self.ids()?;
        let mut raw = Vec::with_capacity(ids.len());
        for id in ids {
            if let Ok((app, fingerprint)) = self.fingerprint(id) {
                raw.push((id, app, fingerprint));
            }
        }
        let observations: Vec<_> = raw
            .iter()
            .map(|(id, _, fingerprint)| (*id, fingerprint.clone()))
            .collect();
        self.lifecycle.reconcile(&observations)?;

        let mut targets = Vec::with_capacity(raw.len());
        for (id, app, fingerprint) in raw {
            let revision = self.lifecycle.epoch(id).ok_or_else(|| {
                Error::new(
                    ErrorCode::Internal,
                    "X11 lifecycle failed to issue a window epoch",
                )
            })?;
            targets.push((
                id,
                NativeTarget {
                    kind: "win".into(),
                    identity: id.to_string(),
                    revision,
                    fingerprint,
                    app,
                },
            ));
        }
        Ok(targets)
    }

    fn validate(&mut self, target: &NativeTarget) -> Result<u32> {
        if target.kind != "win" {
            return Err(Error::invalid("X11 window reference required"));
        }
        let id = target
            .identity
            .parse::<u32>()
            .map_err(|_| Error::invalid("Invalid X11 reference"))?;
        let current = self
            .targets()?
            .into_iter()
            .find(|(window, _)| *window == id)
            .map(|(_, target)| target)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::StaleReference,
                    "X11 window no longer exists in the client list",
                )
            })?;
        if current.revision != target.revision || current.fingerprint != target.fingerprint {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "X11 window lifecycle changed",
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

    fn execute(&mut self, command: &str, args: &Value) -> Result<Value> {
        if command == "window.list" {
            let focused = self.focused()?;
            let mut windows = vec![];
            for (id, target) in self.targets()? {
                let title = self
                    .prop(id, "_NET_WM_NAME", AtomEnum::ANY)
                    .map(|property| String::from_utf8_lossy(&property.value).into_owned())
                    .unwrap_or_default();
                windows.push(json!({
                    "ref":target_marker(target.clone()),
                    "app":target.app,
                    "title":title,
                    "focused":focused==Some(id)
                }));
            }
            return Ok(json!({"windows":windows}));
        }

        let target = native_target(args)?;
        let id = self.validate(&target)?;
        if (command.starts_with("input.") || command.starts_with("pointer."))
            && self.focused()? != Some(id)
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "X11 focus changed; input not sent",
            ));
        }

        match command {
            "window.focus" => self.event(id, "_NET_ACTIVE_WINDOW", [2, CURRENT_TIME, 0, 0, 0])?,
            "window.close" | "app.close" => {
                self.event(id, "_NET_CLOSE_WINDOW", [CURRENT_TIME, 2, 0, 0, 0])?
            }
            "window.move" => self.event(
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
            "window.resize" => self.event(
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
                    xerr(self.conn.xtest_fake_input(
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
                    xerr(self.conn.xtest_fake_input(
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
                let pointer = xerr(xerr(self.conn.query_pointer(self.root))?.reply())?;
                let x = (pointer.root_x as f64
                    + args["dx"]
                        .as_f64()
                        .ok_or_else(|| Error::invalid("dx required"))?)
                .round()
                .clamp(i16::MIN as f64, i16::MAX as f64) as i16;
                let y = (pointer.root_y as f64
                    + args["dy"]
                        .as_f64()
                        .ok_or_else(|| Error::invalid("dy required"))?)
                .round()
                .clamp(i16::MIN as f64, i16::MAX as f64) as i16;
                xerr(
                    xerr(self.conn.xtest_fake_input(
                        MOTION_NOTIFY_EVENT,
                        0,
                        CURRENT_TIME,
                        self.root,
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
                let setup = self.conn.setup();
                let first = setup.min_keycode;
                let count = setup.max_keycode - first + 1;
                let mapping = xerr(xerr(self.conn.get_keyboard_mapping(first, count))?.reply())?;
                let width = mapping.keysyms_per_keycode as usize;
                let index = mapping
                    .keysyms
                    .chunks(width)
                    .position(|keys| keys.first() == Some(&keysym))
                    .ok_or_else(|| {
                        Error::new(
                            ErrorCode::Unsupported,
                            "Keysym requires modifiers or keymap mutation; use semantic text/portal input",
                        )
                    })?;
                let key = first + index as u8;
                xerr(
                    xerr(self.conn.xtest_fake_input(
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
                    xerr(self.conn.xtest_fake_input(
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
        xerr(self.conn.flush())?;
        Ok(json!({"accepted":true,"coordinate_space":"x11_root_pixels"}))
    }
}

pub struct X11 {
    enabled: bool,
    state: Arc<Mutex<Option<State>>>,
    boundary: BlockingBoundary,
}

impl Default for X11 {
    fn default() -> Self {
        let local_display = std::env::var("DISPLAY")
            .is_ok_and(|value| value.starts_with(':') || value.starts_with("unix/:"));
        let native = std::env::var("XDG_SESSION_TYPE").ok().as_deref() != Some("wayland");
        Self {
            enabled: local_display && native,
            state: Arc::new(Mutex::new(None)),
            boundary: BlockingBoundary::new(X11_BLOCKING_TIMEOUT),
        }
    }
}

impl X11 {
    async fn blocking<T, F>(
        &self,
        cancellation: CancellationToken,
        mutation: bool,
        operation: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut State) -> Result<T> + Send + 'static,
    {
        if !self.enabled {
            return Err(Error::unavailable(
                "Native X11 connection unavailable; XWayland is not used as whole-desktop control",
            ));
        }
        let state = self.state.clone();
        self.boundary
            .run(cancellation, mutation, move || {
                let mut guard = state
                    .lock()
                    .map_err(|_| Error::new(ErrorCode::Internal, "X11 state lock poisoned"))?;
                if guard.is_none() {
                    *guard = Some(State::connect()?);
                }
                operation(
                    guard
                        .as_mut()
                        .ok_or_else(|| Error::unavailable("Native X11 connection unavailable"))?,
                )
            })
            .await
    }
}

#[async_trait]
impl Backend for X11 {
    fn name(&self) -> &'static str {
        "x11"
    }

    fn supports(&self, command: &str) -> bool {
        matches!(
            command,
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

    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| "window.manage".to_owned())
    }

    async fn probe(&self) -> Vec<Feature> {
        let ready = self
            .blocking(CancellationToken::new(), false, |state| {
                state.drain_lifecycle_events()?;
                Ok(!state.ids()?.is_empty() || state.focused()?.is_some())
            })
            .await
            .is_ok();
        vec![feature(
            self.name(),
            "window.manage",
            ready,
            "EWMH client-list query behind a bounded blocking worker; XWayland is deliberately not a whole-desktop backend",
            "Use a native X11 session with an EWMH window manager",
        )]
    }

    async fn execute(&self, ctx: &Context, command: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        let command = command.to_owned();
        let args = args.clone();
        let mutation = command != "window.list";
        self.blocking(ctx.cancellation.clone(), mutation, move |state| {
            state.execute(&command, &args)
        })
        .await
    }

    async fn validate(&self, target: &NativeTarget) -> Result<()> {
        let target = target.clone();
        self.blocking(CancellationToken::new(), false, move |state| {
            state.validate(&target)?;
            Ok(())
        })
        .await
    }

    async fn is_focused(&self, target: &NativeTarget) -> Result<bool> {
        let target = target.clone();
        self.blocking(CancellationToken::new(), false, move |state| {
            let id = state.validate(&target)?;
            Ok(state.focused()? == Some(id))
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_epoch_changes_on_disappear_reuse_and_fingerprint_change() {
        let mut life = Lifecycle::default();
        life.reconcile(&[(7, "100:app".into())]).unwrap();
        let first = life.epoch(7).unwrap();
        life.reconcile(&[(7, "100:app".into())]).unwrap();
        assert_eq!(life.epoch(7), Some(first));

        life.reconcile(&[]).unwrap();
        assert_eq!(life.epoch(7), None);
        life.reconcile(&[(7, "100:app".into())]).unwrap();
        let reused = life.epoch(7).unwrap();
        assert!(reused > first);

        life.reconcile(&[(7, "200:other".into())]).unwrap();
        assert!(life.epoch(7).unwrap() > reused);
    }

    #[test]
    fn lifecycle_create_destroy_events_advance_epoch() {
        let mut life = Lifecycle::default();
        life.created(42).unwrap();
        let created = life.epoch(42).unwrap();
        life.observe(42, "1:first").unwrap();
        assert_eq!(life.epoch(42), Some(created));

        life.destroyed(42);
        assert_eq!(life.epoch(42), None);
        life.created(42).unwrap();
        assert!(life.epoch(42).unwrap() > created);
    }

    #[tokio::test]
    async fn blocking_boundary_is_bounded_after_timeout() {
        let boundary = BlockingBoundary::new(Duration::from_millis(20));
        let first = boundary
            .run(CancellationToken::new(), false, || {
                std::thread::sleep(Duration::from_millis(120));
                Ok::<_, Error>(())
            })
            .await
            .unwrap_err();
        assert_eq!(first.code, ErrorCode::Timeout);

        let second = boundary
            .run(CancellationToken::new(), false, || Ok::<_, Error>(()))
            .await
            .unwrap_err();
        assert_eq!(second.code, ErrorCode::ResourceExhausted);

        tokio::time::sleep(Duration::from_millis(130)).await;
        boundary
            .run(CancellationToken::new(), false, || Ok::<_, Error>(()))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn blocking_boundary_marks_dispatched_mutation_timeout_uncertain() {
        let boundary = BlockingBoundary::new(Duration::from_millis(10));
        let error = boundary
            .run(CancellationToken::new(), true, || {
                std::thread::sleep(Duration::from_millis(40));
                Ok::<_, Error>(())
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Timeout);
        assert!(!error.outcome_known);
    }
}
