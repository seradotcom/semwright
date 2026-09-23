//! First-party compositor bridges. The broker owns org.semwright.Broker.
//! Gnome accepts only that unique sender; KWin replies must come from org.kde.KWin.
use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, feature};
use semwright_types::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use zbus::{Connection, Proxy};
fn bridge_error<T>(r: std::result::Result<T, impl std::fmt::Debug>) -> Result<T> {
    r.map_err(|_| Error::new(ErrorCode::BackendFailed, "Compositor bridge request failed"))
}
fn target(v: &Value) -> Result<NativeTarget> {
    Ok(NativeTarget {
        kind: "win".into(),
        identity: arg_str(v, "id")?.into(),
        revision: 0,
        fingerprint: arg_str(v, "fingerprint")?.into(),
        app: arg_str(v, "app")?.into(),
    })
}
fn window_result(values: &[Value]) -> Result<Value> {
    let mut windows = vec![];
    for value in values {
        let mut row = value.clone();
        row.as_object_mut()
            .ok_or_else(|| Error::new(ErrorCode::BackendFailed, "Invalid bridge window"))?
            .remove("id");
        row.as_object_mut()
            .ok_or_else(|| Error::new(ErrorCode::BackendFailed, "Invalid bridge window"))?
            .remove("fingerprint");
        row["ref"] = target_marker(target(value)?);
        windows.push(row);
    }
    Ok(json!({"windows":windows}))
}
fn instruction(command: &str, args: &Value) -> Result<Value> {
    let t = native_target(args)?;
    let operation = match command {
        "window.focus" => "focus",
        "window.move" => "move",
        "window.resize" => "resize",
        "window.close" | "app.close" => "close",
        _ => {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Unknown compositor operation",
            ));
        }
    };
    let mut value = json!({"id":unique_id(),"operation":operation,"target":t.identity,"fingerprint":t.fingerprint});
    for key in match operation {
        "move" => vec!["x", "y"],
        "resize" => vec!["width", "height"],
        _ => vec![],
    } {
        value[key] = args[key].clone();
    }
    Ok(value)
}
fn supports(command: &str) -> bool {
    matches!(
        command,
        "window.list"
            | "window.focus"
            | "window.move"
            | "window.resize"
            | "window.close"
            | "app.close"
    )
}
fn parse_snapshot(raw: &str) -> Result<Vec<Value>> {
    if raw.len() > 1_048_576 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Bridge snapshot exceeds budget",
        ));
    }
    let data: Value = serde_json::from_str(raw)?;
    if data["version"] != 1 {
        return Err(Error::new(
            ErrorCode::ProtocolMismatch,
            "Bridge protocol version mismatch",
        ));
    }
    let rows = data["windows"]
        .as_array()
        .filter(|r| r.len() <= 2000)
        .ok_or_else(|| Error::new(ErrorCode::BackendFailed, "Invalid bridge window array"))?;
    for row in rows {
        let t = target(row)?;
        if t.identity.len() > 256 || t.fingerprint.len() > 256 || t.app.len() > 255 {
            return Err(Error::invalid("Bridge identity is too long"));
        }
    }
    Ok(rows.clone())
}
pub struct Gnome {
    connection: Option<Connection>,
}
impl Gnome {
    pub fn new(connection: Option<Connection>) -> Self {
        Self { connection }
    }
    async fn call(&self, method: &str, payload: Option<&str>) -> Result<String> {
        let c = self
            .connection
            .as_ref()
            .ok_or_else(|| Error::unavailable("Broker has no owned session-bus connection"))?;
        let proxy = bridge_error(
            Proxy::new(
                c,
                "org.semwright.GnomeBridge",
                "/org/semwright/GnomeBridge",
                "org.semwright.WindowBridge1",
            )
            .await,
        )?;
        match payload {
            Some(s) => bridge_error(proxy.call(method, &(s,)).await),
            None => bridge_error(proxy.call(method, &()).await),
        }
    }
    async fn windows(&self) -> Result<Vec<Value>> {
        parse_snapshot(&self.call("Snapshot", None).await?)
    }
}
#[async_trait]
impl Backend for Gnome {
    fn name(&self) -> &'static str {
        "gnome"
    }
    fn supports(&self, c: &str) -> bool {
        supports(c)
    }
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| "window.manage".to_owned())
    }
    async fn probe(&self) -> Vec<Feature> {
        let ok = tokio::time::timeout(Duration::from_secs(2), self.call("Hello", None))
            .await
            .is_ok_and(|r| {
                r.is_ok_and(|s| serde_json::from_str::<Value>(&s).is_ok_and(|v| v["version"] == 1))
            });
        vec![feature(
            self.name(),
            "window.manage",
            ok,
            "Narrow authenticated GNOME Shell bridge",
            "Install and explicitly enable bridges/gnome; supported shell versions require live verification",
        )]
    }
    async fn execute(&self, ctx: &Context, c: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        if c == "window.list" {
            return window_result(&self.windows().await?);
        }
        self.validate(&native_target(args)?).await?;
        let request = instruction(c, args)?;
        let raw = serde_json::to_string(&request)?;
        let response: Value = serde_json::from_str(
            &self
                .call("Execute", Some(&raw))
                .await
                .map_err(Error::uncertain)?,
        )?;
        if response["id"] != request["id"] || response["accepted"] != true {
            return Err(
                Error::new(ErrorCode::BackendFailed, "Bridge rejected command").uncertain(),
            );
        }
        Ok(response)
    }
    async fn validate(&self, t: &NativeTarget) -> Result<()> {
        for v in self.windows().await? {
            let now = target(&v)?;
            if now.identity == t.identity && now.fingerprint == t.fingerprint {
                return Ok(());
            }
        }
        Err(Error::new(
            ErrorCode::StaleReference,
            "GNOME window disappeared or changed identity",
        ))
    }
    async fn is_focused(&self, t: &NativeTarget) -> Result<bool> {
        for v in self.windows().await? {
            let now = target(&v)?;
            if now.identity == t.identity && now.fingerprint == t.fingerprint {
                return Ok(v["focused"].as_bool().unwrap_or(false));
            }
        }
        Ok(false)
    }
}
struct MailboxState {
    snapshot: RwLock<(Instant, Vec<Value>)>,
    queue: mpsc::Sender<Value>,
    receiver: Mutex<mpsc::Receiver<Value>>,
    pending: std::sync::Mutex<BTreeMap<String, oneshot::Sender<Value>>>,
    heartbeat: std::sync::Mutex<Instant>,
    published: std::sync::atomic::AtomicBool,
}
struct PendingKwin {
    state: Arc<MailboxState>,
    id: String,
}
impl Drop for PendingKwin {
    fn drop(&mut self) {
        if let Ok(mut p) = self.state.pending.lock() {
            p.remove(&self.id);
        }
    }
}
pub struct Kwin {
    state: Arc<MailboxState>,
}
struct Mailbox {
    state: Arc<MailboxState>,
}
async fn check_kwin_sender(
    connection: &Connection,
    header: &zbus::message::Header<'_>,
) -> zbus::fdo::Result<()> {
    let sender = header
        .sender()
        .ok_or_else(|| zbus::fdo::Error::AccessDenied("Missing sender".into()))?;
    let dbus = Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await
    .map_err(|_| zbus::fdo::Error::AccessDenied("Session bus unavailable".into()))?;
    let owner: String = dbus
        .call("GetNameOwner", &("org.kde.KWin",))
        .await
        .map_err(|_| zbus::fdo::Error::AccessDenied("KWin is not running".into()))?;
    if sender.as_str() != owner {
        return Err(zbus::fdo::Error::AccessDenied(
            "Only the active KWin process may use this mailbox".into(),
        ));
    }
    Ok(())
}
#[zbus::interface(name = "org.semwright.KWinMailbox1")]
impl Mailbox {
    async fn publish(
        &self,
        raw: &str,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        check_kwin_sender(connection, &header).await?;
        let rows = parse_snapshot(raw)
            .map_err(|_| zbus::fdo::Error::InvalidArgs("Invalid window snapshot".into()))?;
        *self.state.snapshot.write().await = (Instant::now(), rows);
        self.state
            .published
            .store(true, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut heartbeat) = self.state.heartbeat.lock() {
            *heartbeat = Instant::now();
        }
        Ok(())
    }
    async fn next(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        check_kwin_sender(connection, &header).await?;
        if let Ok(mut heartbeat) = self.state.heartbeat.lock() {
            *heartbeat = Instant::now();
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let mut receiver = self.state.receiver.lock().await;
        loop {
            match tokio::time::timeout_at(deadline, receiver.recv()).await {
                Ok(Some(value)) => {
                    let active = value["id"].as_str().is_some_and(|id| {
                        self.state.pending.lock().is_ok_and(|p| p.contains_key(id))
                    });
                    if active {
                        return Ok(value.to_string());
                    }
                }
                _ => return Ok("{}".into()),
            }
        }
    }
    async fn complete(
        &self,
        raw: &str,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        check_kwin_sender(connection, &header).await?;
        if raw.len() > 4096 {
            return Err(zbus::fdo::Error::InvalidArgs("Reply exceeds budget".into()));
        }
        let value: Value = serde_json::from_str(raw)
            .map_err(|_| zbus::fdo::Error::InvalidArgs("Invalid reply".into()))?;
        let id = value["id"]
            .as_str()
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("Reply id required".into()))?;
        if let Ok(mut pending) = self.state.pending.lock()
            && let Some(sender) = pending.remove(id)
        {
            let _ = sender.send(value);
        }
        Ok(())
    }
}
impl Kwin {
    pub async fn attach(connection: &Connection) -> Result<Self> {
        let (queue, receiver) = mpsc::channel(32);
        let state = Arc::new(MailboxState {
            snapshot: RwLock::new((Instant::now() - Duration::from_secs(3600), vec![])),
            queue,
            receiver: Mutex::new(receiver),
            pending: std::sync::Mutex::new(BTreeMap::new()),
            heartbeat: std::sync::Mutex::new(Instant::now() - Duration::from_secs(3600)),
            published: std::sync::atomic::AtomicBool::new(false),
        });
        bridge_error(
            connection
                .object_server()
                .at(
                    "/org/semwright/KWinBridge",
                    Mailbox {
                        state: state.clone(),
                    },
                )
                .await,
        )?;
        Ok(Self { state })
    }
    async fn windows(&self) -> Result<Vec<Value>> {
        let snapshot = self.state.snapshot.read().await;
        // Publish is event-driven; liveness is refreshed by Next, not by snapshots.
        if !self
            .state
            .published
            .load(std::sync::atomic::Ordering::SeqCst)
            || !self
                .state
                .heartbeat
                .lock()
                .is_ok_and(|h| h.elapsed() < Duration::from_secs(15))
        {
            return Err(Error::unavailable(
                "KWin bridge has not published a window snapshot",
            ));
        }
        Ok(snapshot.1.clone())
    }
}
#[async_trait]
impl Backend for Kwin {
    fn name(&self) -> &'static str {
        "kwin"
    }
    fn supports(&self, c: &str) -> bool {
        supports(c)
    }
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| "window.manage".to_owned())
    }
    async fn probe(&self) -> Vec<Feature> {
        vec![feature(
            self.name(),
            "window.manage",
            self.windows().await.is_ok(),
            "KWin native script mailbox",
            "Install bridges/kwin and enable it after starting the user broker; restart the script after broker restarts",
        )]
    }
    async fn execute(&self, ctx: &Context, c: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        if c == "window.list" {
            return window_result(&self.windows().await?);
        }
        self.validate(&native_target(args)?).await?;
        let request = instruction(c, args)?;
        let id = arg_str(&request, "id")?.to_owned();
        let (sender, receiver) = oneshot::channel();
        self.state
            .pending
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "KWin pending lock poisoned"))?
            .insert(id.clone(), sender);
        let _pending = PendingKwin {
            state: self.state.clone(),
            id: id.clone(),
        };
        self.state
            .queue
            .send(request)
            .await
            .map_err(|_| Error::unavailable("KWin mailbox closed"))?;
        let response = tokio::select! {_ =ctx.cancellation.cancelled()=>Err(Error::new(ErrorCode::Cancelled,"KWin operation cancelled").uncertain()),r=tokio::time::timeout(Duration::from_secs(5),receiver)=>match r{Ok(Ok(v))=>Ok(v),_=>Err(Error::new(ErrorCode::Timeout,"KWin command outcome is unknown").uncertain())}};
        let response = response?;
        if response["accepted"] != true {
            return Err(
                Error::new(ErrorCode::BackendFailed, "KWin rejected operation").uncertain(),
            );
        }
        Ok(response)
    }
    async fn validate(&self, t: &NativeTarget) -> Result<()> {
        for v in self.windows().await? {
            let now = target(&v)?;
            if now.identity == t.identity && now.fingerprint == t.fingerprint {
                return Ok(());
            }
        }
        Err(Error::new(
            ErrorCode::StaleReference,
            "KWin window disappeared or changed identity",
        ))
    }
    async fn is_focused(&self, t: &NativeTarget) -> Result<bool> {
        for v in self.windows().await? {
            let now = target(&v)?;
            if now.identity == t.identity && now.fingerprint == t.fingerprint {
                return Ok(v["focused"].as_bool().unwrap_or(false));
            }
        }
        Ok(false)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrong_bridge_version() {
        assert!(parse_snapshot(r#"{"version":9,"windows":[]}"#).is_err());
    }
    #[test]
    fn targets_are_not_raw_ids() {
        let out = window_result(&[json!({"id":"1","app":"a","fingerprint":"epoch:1"})]).unwrap();
        assert!(out["windows"][0].get("id").is_none());
        assert!(out["windows"][0]["ref"]["$ref"].is_object());
    }
}
