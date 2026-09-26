//! CDP against a broker-launched private browser only. No attach endpoint or JavaScript evaluator.
//! An origin allowlist constrains top-level commands, NOT all browser network traffic.
use async_trait::async_trait;
use base64::Engine;
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use semwright_backend_api::{Backend, Context};
use semwright_policy::{FilesystemGrant, Root, validate_relative_path};
use semwright_protocol::{current_uid, private_directory};
use semwright_types::*;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tokio::{
    net::TcpStream,
    process::{Child, Command},
    sync::{Mutex, oneshot},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, protocol::WebSocketConfig},
};
use tokio_util::sync::CancellationToken;
use url::Url;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Pending = Arc<StdMutex<BTreeMap<u64, oneshot::Sender<Result<Value>>>>>;
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserConfig {
    pub executable: PathBuf,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    #[serde(default)]
    pub allow_downloads: bool,
    #[serde(default = "default_download_bytes")]
    pub max_download_bytes: u64,
    #[serde(default = "default_total_download_bytes")]
    pub max_total_download_bytes: u64,
    #[serde(default = "default_download_count")]
    pub max_downloads: u32,
}
const BROWSER_STARTUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const BROWSER_UPLOAD_MAX_FILE_BYTES: usize = 32 * 1024 * 1024;
const BROWSER_UPLOAD_MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const BROWSER_UPLOAD_MAX_FILES_PER_ACTION: usize = 8;
const BROWSER_UPLOAD_MAX_STAGED_FILES: u32 = 32;

const fn default_download_bytes() -> u64 {
    32 * 1024 * 1024
}
const fn default_total_download_bytes() -> u64 {
    64 * 1024 * 1024
}
const fn default_download_count() -> u32 {
    8
}
impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            executable: PathBuf::from("/usr/bin/chromium"),
            allowed_origins: vec![],
            allow_downloads: false,
            max_download_bytes: default_download_bytes(),
            max_total_download_bytes: default_total_download_bytes(),
            max_downloads: default_download_count(),
        }
    }
}
impl BrowserConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.executable.is_absolute()
            || !matches!(
                self.executable.file_name().and_then(|s| s.to_str()),
                Some("chromium" | "chromium-browser" | "google-chrome" | "chrome")
            )
        {
            return Err(Error::invalid(
                "Browser executable must be an absolute Chromium-family executable, not a command line",
            ));
        }
        if self.allowed_origins.len() > 128 {
            return Err(Error::invalid("Too many browser origins"));
        }
        if self.max_download_bytes == 0
            || self.max_total_download_bytes == 0
            || self.max_downloads == 0
            || self.max_download_bytes > 256 * 1024 * 1024
            || self.max_total_download_bytes > 1024 * 1024 * 1024
            || self.max_downloads > 64
            || self.max_download_bytes > self.max_total_download_bytes
        {
            return Err(Error::invalid(
                "Browser download quotas must be positive, bounded, and per-file <= total",
            ));
        }
        for origin in &self.allowed_origins {
            let parsed =
                Url::parse(origin).map_err(|_| Error::invalid("Invalid configured origin"))?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.origin().ascii_serialization() != *origin
            {
                return Err(Error::invalid(
                    "Origins must be exact http(s) scheme://host[:port], without paths, credentials or wildcards",
                ));
            }
        }
        Ok(())
    }
    pub fn check_url(&self, text: &str) -> Result<()> {
        if text == "about:blank" {
            return Ok(());
        }
        let url = Url::parse(text).map_err(|_| Error::invalid("Invalid browser URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Only explicitly allowed http(s) origins and about:blank may be targeted",
            ));
        }
        if !self
            .allowed_origins
            .contains(&url.origin().ascii_serialization())
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Browser URL origin is not in the owner's allowlist",
            ));
        }
        Ok(())
    }
}
fn target_url_ready(config: &BrowserConfig, text: &str) -> Result<bool> {
    match config.check_url(text) {
        Ok(()) => Ok(true),
        Err(error) if error.code == ErrorCode::InvalidArgument => Ok(false),
        Err(error) => Err(error),
    }
}
struct Cdp {
    writer: Arc<Mutex<SplitSink<Socket, Message>>>,
    pending: Pending,
    sequence: Arc<AtomicU64>,
    generations: Arc<StdMutex<BTreeMap<String, u64>>>,
    attached_sessions: Arc<StdMutex<BTreeMap<String, String>>>,
    events: Arc<StdMutex<VecDeque<Value>>>,
    downloads: Arc<StdMutex<DownloadState>>,
    alive: Arc<AtomicBool>,
    stop: CancellationToken,
}
struct PendingGuard {
    pending: Pending,
    id: u64,
}
impl Drop for PendingGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = self.pending.lock() {
            map.remove(&self.id);
        }
    }
}
#[derive(Clone, Copy)]
struct DownloadLimits {
    per_file: u64,
    total: u64,
    count: u32,
}
impl From<&BrowserConfig> for DownloadLimits {
    fn from(config: &BrowserConfig) -> Self {
        Self {
            per_file: config.max_download_bytes,
            total: config.max_total_download_bytes,
            count: config.max_downloads,
        }
    }
}
#[derive(Clone, Debug, Default)]
struct DownloadRecord {
    received: u64,
    total_hint: Option<u64>,
    state: String,
    quota_exceeded: bool,
}
#[derive(Clone, Debug, Default)]
struct DownloadState {
    started: u32,
    total_received: u64,
    records: BTreeMap<String, DownloadRecord>,
    quota_cancellations: u32,
}
fn safe_download_guid(value: &Value) -> Option<String> {
    let guid = value.as_str()?;
    (guid.len() <= 128
        && !guid.is_empty()
        && guid
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')))
    .then(|| guid.to_owned())
}
fn bounded_byte_count(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value.as_f64().and_then(|number| {
            (number.is_finite() && number >= 0.0 && number <= u64::MAX as f64)
                .then_some(number as u64)
        })
    })
}
fn remove_owned_regular(path: &std::path::Path) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_file()
        && !metadata.file_type().is_symlink()
        && metadata.uid() == current_uid()
        && metadata.mode() & 0o022 == 0
    {
        let _ = std::fs::remove_file(path);
    }
}
fn remove_owned_download(download_dir: &std::path::Path, guid: &str) {
    remove_owned_regular(&download_dir.join(guid));
}
fn cancel_download_request(id: u64, guid: &str) -> Message {
    Message::Text(
        json!({"id":id,"method":"Browser.cancelDownload","params":{"guid":guid}})
            .to_string()
            .into(),
    )
}
impl DownloadState {
    fn ensure_record(&mut self, guid: &str) -> &mut DownloadRecord {
        if !self.records.contains_key(guid) {
            self.started = self.started.saturating_add(1);
            self.records
                .insert(guid.to_owned(), DownloadRecord::default());
        }
        self.records
            .get_mut(guid)
            .expect("download record inserted")
    }
    fn begin(&mut self, guid: &str, limits: DownloadLimits) -> bool {
        self.ensure_record(guid);
        let over = self.started > limits.count;
        if over {
            let record = self.records.get_mut(guid).expect("download record exists");
            if !record.quota_exceeded {
                record.quota_exceeded = true;
                self.quota_cancellations = self.quota_cancellations.saturating_add(1);
            }
        }
        over
    }
    fn progress(
        &mut self,
        guid: &str,
        received: u64,
        total_hint: Option<u64>,
        state: &str,
        limits: DownloadLimits,
    ) -> bool {
        self.ensure_record(guid);
        let previous = self
            .records
            .get(guid)
            .map(|record| record.received)
            .unwrap_or(0);
        self.total_received = self
            .total_received
            .saturating_add(received.saturating_sub(previous));
        let total_received = self.total_received;
        let count_over = self.started > limits.count;
        let record = self.records.get_mut(guid).expect("download record exists");
        record.received = record.received.max(received);
        record.total_hint = total_hint.or(record.total_hint);
        record.state = state.chars().take(32).collect();
        let over = count_over
            || record.received > limits.per_file
            || record
                .total_hint
                .is_some_and(|total| total > limits.per_file)
            || total_received > limits.total;
        if over && !record.quota_exceeded {
            record.quota_exceeded = true;
            self.quota_cancellations = self.quota_cancellations.saturating_add(1);
        }
        over
    }
}

fn cdp_event_changes_document_identity(method: &str, event: &Value) -> bool {
    match method {
        "DOM.documentUpdated" | "Inspector.targetCrashed" => true,
        // Parent sessions also observe subframe navigation. That does not replace
        // the parent's document. OOPIF child sessions report their own top-level
        // frame without parentId and therefore advance only the child document.
        "Page.frameNavigated" => event
            .pointer("/params/frame/parentId")
            .and_then(Value::as_str)
            .is_none(),
        _ => false,
    }
}

impl Cdp {
    async fn connect(
        endpoint: &str,
        limits: DownloadLimits,
        download_dir: PathBuf,
    ) -> Result<Arc<Self>> {
        let url = Url::parse(endpoint).map_err(|_| Error::invalid("Invalid CDP endpoint"))?;
        if url.scheme() != "ws"
            || url.host_str() != Some("127.0.0.1")
            || !url.path().starts_with("/devtools/browser/")
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "CDP endpoint must come from our isolated loopback browser",
            ));
        }
        let config = WebSocketConfig::default()
            .max_message_size(Some(48 * 1024 * 1024))
            .max_frame_size(Some(48 * 1024 * 1024));
        let (socket, _) =
            tokio_tungstenite::connect_async_with_config(endpoint, Some(config), false)
                .await
                .map_err(|_| Error::unavailable("Cannot connect to isolated browser CDP"))?;
        let (sink, mut stream) = socket.split();
        let this = Arc::new(Self {
            writer: Arc::new(Mutex::new(sink)),
            pending: Arc::new(StdMutex::new(BTreeMap::new())),
            sequence: Arc::new(AtomicU64::new(1)),
            generations: Arc::new(StdMutex::new(BTreeMap::new())),
            attached_sessions: Arc::new(StdMutex::new(BTreeMap::new())),
            events: Arc::new(StdMutex::new(VecDeque::new())),
            downloads: Arc::new(StdMutex::new(DownloadState::default())),
            alive: Arc::new(AtomicBool::new(true)),
            stop: CancellationToken::new(),
        });
        let pending = this.pending.clone();
        let writer = this.writer.clone();
        let sequence = this.sequence.clone();
        let generations = this.generations.clone();
        let attached_sessions = this.attached_sessions.clone();
        let events = this.events.clone();
        let downloads = this.downloads.clone();
        let alive = this.alive.clone();
        let stop = this.stop.clone();
        tokio::spawn(async move {
            loop {
                let message =
                    tokio::select! {_ = stop.cancelled()=>break,value=stream.next()=>value};
                let Some(Ok(message)) = message else { break };
                let Message::Text(text) = message else {
                    continue;
                };
                let Ok(value) = serde_json::from_str::<Value>(text.as_str()) else {
                    break;
                };
                if let Some(id) = value.get("id").and_then(Value::as_u64) {
                    let sender = pending.lock().ok().and_then(|mut map| map.remove(&id));
                    if let Some(sender) = sender {
                        let result = if value.get("error").is_some() {
                            Err(Error::new(
                                ErrorCode::BackendFailed,
                                "CDP method rejected; browser details redacted",
                            )
                            .uncertain())
                        } else {
                            Ok(value.get("result").cloned().unwrap_or_else(|| json!({})))
                        };
                        let _ = sender.send(result);
                    }
                } else if let Some(method) = value.get("method").and_then(Value::as_str) {
                    if method == "Target.attachedToTarget"
                        && let Some(target) = value
                            .pointer("/params/targetInfo/targetId")
                            .and_then(Value::as_str)
                        && let Some(child_session) =
                            value.pointer("/params/sessionId").and_then(Value::as_str)
                    {
                        if let Ok(mut map) = attached_sessions.lock() {
                            map.insert(target.to_owned(), child_session.to_owned());
                        }
                        if let Ok(mut map) = generations.lock() {
                            map.entry(child_session.to_owned()).or_insert(0);
                        }
                    } else if method == "Target.detachedFromTarget" {
                        let target = value.pointer("/params/targetId").and_then(Value::as_str);
                        let child_session =
                            value.pointer("/params/sessionId").and_then(Value::as_str);
                        if let Ok(mut map) = attached_sessions.lock() {
                            if let Some(target) = target {
                                map.remove(target);
                            } else if let Some(child_session) = child_session {
                                map.retain(|_, session| session != child_session);
                            }
                        }
                    }
                    // A semantic ref identifies a concrete backend DOM node, not a frozen
                    // snapshot of all page attributes/children. Invalidate a whole document
                    // generation only when its document identity can change. Ordinary DOM
                    // mutations keep existing backendNodeIds usable and are validated again
                    // with DOM.describeNode/hit-testing immediately before mutation.
                    if cdp_event_changes_document_identity(method, &value)
                        && let Some(session) = value.get("sessionId").and_then(Value::as_str)
                        && let Ok(mut map) = generations.lock()
                    {
                        let n = map.entry(session.into()).or_insert(0);
                        *n = n.saturating_add(1);
                    }

                    let mut quota_exceeded = false;
                    let mut download_guid = None;
                    let mut download_state = None;
                    if matches!(
                        method,
                        "Browser.downloadWillBegin" | "Browser.downloadProgress"
                    ) && let Some(guid) =
                        value.pointer("/params/guid").and_then(safe_download_guid)
                    {
                        download_guid = Some(guid.clone());
                        if let Ok(mut state) = downloads.lock() {
                            quota_exceeded = if method == "Browser.downloadWillBegin" {
                                state.begin(&guid, limits)
                            } else {
                                let received = value
                                    .pointer("/params/receivedBytes")
                                    .and_then(bounded_byte_count)
                                    .unwrap_or(0);
                                let total = value
                                    .pointer("/params/totalBytes")
                                    .and_then(bounded_byte_count);
                                let status = value
                                    .pointer("/params/state")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                download_state = Some(status.chars().take(32).collect::<String>());
                                state.progress(&guid, received, total, status, limits)
                            };
                        }
                        if quota_exceeded {
                            let id = sequence.fetch_add(1, Ordering::Relaxed);
                            let _ = writer
                                .lock()
                                .await
                                .send(cancel_download_request(id, &guid))
                                .await;
                        }
                        if quota_exceeded || download_state.as_deref() == Some("canceled") {
                            let directory = download_dir.clone();
                            let guid = guid.clone();
                            tokio::spawn(async move {
                                for _ in 0..20 {
                                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                    remove_owned_download(&directory, &guid);
                                    if !directory.join(&guid).exists() {
                                        break;
                                    }
                                }
                            });
                        }
                    }

                    // Bounded metadata only: never console text, headers, URLs, request bodies,
                    // download names or dialog prompt input. Dialog messages are page-visible data.
                    if matches!(
                        method,
                        "Browser.downloadWillBegin"
                            | "Browser.downloadProgress"
                            | "Log.entryAdded"
                            | "Network.loadingFailed"
                            | "Inspector.targetCrashed"
                            | "Page.javascriptDialogOpening"
                            | "Page.javascriptDialogClosed"
                    ) {
                        let mut row = json!({"event":method});
                        if let Some(session) = value.get("sessionId").and_then(Value::as_str) {
                            row["session"] = json!(session.chars().take(256).collect::<String>());
                        }
                        if let Some(guid) = download_guid {
                            row["guid"] = json!(guid);
                        }
                        if let Some(state) = download_state {
                            row["state"] = json!(state);
                        }
                        if method == "Page.javascriptDialogOpening" {
                            if let Some(kind) =
                                value.pointer("/params/type").and_then(Value::as_str)
                            {
                                row["dialog_type"] =
                                    json!(kind.chars().take(32).collect::<String>());
                            }
                            if let Some(message) =
                                value.pointer("/params/message").and_then(Value::as_str)
                            {
                                row["message"] =
                                    json!(message.chars().take(512).collect::<String>());
                            }
                        }
                        for key in ["receivedBytes", "totalBytes"] {
                            if let Some(count) = value
                                .pointer(&format!("/params/{key}"))
                                .and_then(bounded_byte_count)
                            {
                                row[key] = json!(count);
                            }
                        }
                        if quota_exceeded {
                            row["quota_exceeded"] = json!(true);
                        }
                        if let Ok(mut queue) = events.lock() {
                            if queue.len() == 256 {
                                queue.pop_front();
                            }
                            queue.push_back(row);
                        }
                    }
                }
            }
            alive.store(false, Ordering::SeqCst);
            if let Ok(mut map) = pending.lock() {
                for (_, sender) in std::mem::take(&mut *map) {
                    let _ = sender.send(Err(
                        Error::unavailable("Browser CDP disconnected").uncertain()
                    ));
                }
            }
        });
        Ok(this)
    }
    async fn call(&self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(Error::unavailable("Browser CDP is disconnected"));
        }
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| Error::new(ErrorCode::Internal, "CDP request lock poisoned"))?;
            if pending.len() >= 64 {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Too many CDP requests",
                ));
            }
            pending.insert(id, sender);
        }
        let _guard = PendingGuard {
            pending: self.pending.clone(),
            id,
        };
        let mut request = json!({"id":id,"method":method,"params":params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        self.writer
            .lock()
            .await
            .send(Message::Text(request.to_string().into()))
            .await
            .map_err(|_| Error::unavailable("Browser CDP write failed").uncertain())?;
        tokio::time::timeout(std::time::Duration::from_secs(15), receiver)
            .await
            .map_err(|_| {
                Error::new(ErrorCode::Timeout, "Browser CDP response timed out").uncertain()
            })?
            .map_err(|_| Error::unavailable("Browser CDP response channel closed").uncertain())?
    }
    fn generation(&self, session: &str) -> Result<u64> {
        Ok(*self
            .generations
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "CDP generation lock poisoned"))?
            .get(session)
            .unwrap_or(&0))
    }
    fn attached_session(&self, target: &str) -> Result<Option<String>> {
        Ok(self
            .attached_sessions
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "CDP attached-session lock poisoned"))?
            .get(target)
            .cloned())
    }
    /// Invalidate semantic DOM references synchronously before a Semwright-initiated
    /// mutation. CDP events may advance the generation again; generation equality, not
    /// adjacency, is the contract.
    fn invalidate_session(&self, session: &str) -> Result<u64> {
        let mut generations = self
            .generations
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "CDP generation lock poisoned"))?;
        let generation = generations.entry(session.to_owned()).or_insert(0);
        *generation = generation.saturating_add(1);
        Ok(*generation)
    }
    fn metadata(&self) -> Result<Vec<Value>> {
        Ok(self
            .events
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "CDP event lock poisoned"))?
            .iter()
            .cloned()
            .collect())
    }
    fn download_snapshot(&self) -> Result<DownloadState> {
        Ok(self
            .downloads
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Download state lock poisoned"))?
            .clone())
    }
}
impl Drop for Cdp {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
struct Instance {
    child: Child,
    profile: PathBuf,
    downloads: PathBuf,
    epoch: String,
    cdp: Arc<Cdp>,
    sessions: BTreeMap<String, String>,
    staged_upload_bytes: u64,
    staged_upload_files: u32,
    // A successful close invalidates refs immediately, before asynchronous CDP target events.
    closed_tabs: VecDeque<String>,
}
impl Instance {
    async fn targets(&self) -> Result<Vec<Value>> {
        let response = self.cdp.call("Target.getTargets", json!({}), None).await?;
        let rows = response["targetInfos"]
            .as_array()
            .ok_or_else(|| Error::new(ErrorCode::BackendFailed, "CDP target list is malformed"))?;
        Ok(rows
            .iter()
            .filter(|row| row["type"] == "page")
            .take(256)
            .cloned()
            .collect())
    }
    async fn check_tab(&self, target: &str, config: &BrowserConfig) -> Result<()> {
        if self.closed_tabs.iter().any(|id| id == target) {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Browser tab was closed",
            ));
        }
        for attempt in 0..=50 {
            let row = self
                .targets()
                .await?
                .into_iter()
                .find(|row| row["targetId"] == target)
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::StaleReference,
                        "Isolated browser tab no longer exists",
                    )
                })?;
            if target_url_ready(config, row["url"].as_str().unwrap_or(""))? {
                return Ok(());
            }
            if attempt == 50 {
                return Err(Error::new(
                    ErrorCode::BackendFailed,
                    "Browser target URL did not stabilize",
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        unreachable!("bounded browser target stabilization loop")
    }
    async fn session(&mut self, target: &str) -> Result<String> {
        if let Some(session) = self.sessions.get(target) {
            return Ok(session.clone());
        }
        let result = self
            .cdp
            .call(
                "Target.attachToTarget",
                json!({"targetId":target,"flatten":true}),
                None,
            )
            .await?;
        let session = arg_str(&result, "sessionId")?.to_owned();
        for domain in [
            "DOM.enable",
            "Page.enable",
            "Accessibility.enable",
            "Log.enable",
            "Network.enable",
        ] {
            self.cdp.call(domain, json!({}), Some(&session)).await?;
        }
        self.cdp
            .call(
                "Target.setAutoAttach",
                json!({"autoAttach":true,"waitForDebuggerOnStart":false,"flatten":true}),
                Some(&session),
            )
            .await?;
        // Establish a read barrier after enabling domains. CDP may emit the current
        // document/frame events as a consequence of enable/attach; process those before
        // the first semantic ref captures this session's generation.
        self.cdp
            .call("Page.getFrameTree", json!({}), Some(&session))
            .await?;
        self.cdp
            .call("DOM.getDocument", json!({"depth":0}), Some(&session))
            .await?;
        self.sessions.insert(target.into(), session.clone());
        Ok(session)
    }
    fn tab_ref(&self, target: &str) -> Value {
        target_marker(NativeTarget {
            kind: "tab".into(),
            identity: target.into(),
            revision: 0,
            fingerprint: self.epoch.clone(),
            app: "org.semwright.Chromium".into(),
        })
    }
    fn dom_ref(&self, target: &str, node: &Value, session: &str) -> Result<Value> {
        self.dom_ref_in_frame(target, node, session, None, None)
    }
    fn dom_ref_in_frame(
        &self,
        target: &str,
        node: &Value,
        session: &str,
        frame: Option<&str>,
        frame_revision: Option<u64>,
    ) -> Result<Value> {
        let id = node["backendNodeId"].as_u64().ok_or_else(|| {
            Error::new(
                ErrorCode::BackendFailed,
                "DOM node has no stable backend identity",
            )
        })?;
        let identity = match frame {
            Some(frame) => format!("{target}#frame:{frame}#node:{id}"),
            None => format!("{target}#{id}"),
        };
        Ok(target_marker(NativeTarget {
            kind: "dom".into(),
            identity,
            revision: frame_revision.unwrap_or(self.cdp.generation(session)?),
            fingerprint: self.epoch.clone(),
            app: "org.semwright.Chromium".into(),
        }))
    }
    fn frame_ref(&self, target: &str, frame: &Value) -> Result<Value> {
        let id = arg_str(frame, "id")?;
        Ok(target_marker(NativeTarget {
            kind: "frame".into(),
            identity: format!("{target}#frame:{id}"),
            revision: frame_document_revision(frame),
            fingerprint: self.epoch.clone(),
            app: "org.semwright.Chromium".into(),
        }))
    }
    fn download_artifacts(&self, limits: DownloadLimits) -> Result<Vec<Value>> {
        let snapshot = self.cdp.download_snapshot()?;
        let mut artifacts = Vec::new();
        for (guid, record) in snapshot.records {
            if record.state != "completed" || record.quota_exceeded {
                continue;
            }
            let path = self.downloads.join(&guid);
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.file_type().is_file()
                || metadata.file_type().is_symlink()
                || metadata.uid() != current_uid()
                || metadata.mode() & 0o022 != 0
                || metadata.len() > limits.per_file
            {
                continue;
            }
            artifacts.push(json!({
                "id": guid,
                "path": path,
                "bytes": metadata.len(),
                "lifetime": "browser_instance"
            }));
            if artifacts.len() >= limits.count as usize {
                break;
            }
        }
        Ok(artifacts)
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        self.cdp.stop.cancel();
        let _ = self.child.start_kill();
    }
}
pub struct Chromium {
    config: BrowserConfig,
    directory: PathBuf,
    filesystem_grants: Vec<FilesystemGrant>,
    instance: Mutex<Option<Instance>>,
    artifacts: Arc<StdMutex<BTreeMap<PathBuf, u64>>>,
}
impl Chromium {
    pub fn new(config: BrowserConfig, directory: PathBuf) -> Result<Self> {
        Self::new_with_grants(config, directory, &[])
    }
    pub fn new_with_grants(
        config: BrowserConfig,
        directory: PathBuf,
        filesystem_grants: &[FilesystemGrant],
    ) -> Result<Self> {
        config.validate()?;
        private_directory(&directory)?;
        let mut names = std::collections::BTreeSet::new();
        for grant in filesystem_grants {
            if !names.insert(grant.name.clone()) {
                return Err(Error::invalid("Duplicate browser filesystem grant name"));
            }
        }
        Ok(Self {
            config,
            directory,
            filesystem_grants: filesystem_grants.to_vec(),
            instance: Mutex::new(None),
            artifacts: Arc::new(StdMutex::new(BTreeMap::new())),
        })
    }
    fn stage_uploads(
        &self,
        instance: &Instance,
        root_name: &str,
        paths: &[Value],
    ) -> Result<(PathBuf, Vec<PathBuf>, u64)> {
        if paths.is_empty() || paths.len() > BROWSER_UPLOAD_MAX_FILES_PER_ACTION {
            return Err(Error::invalid("Upload requires between 1 and 8 files"));
        }
        if instance
            .staged_upload_files
            .saturating_add(paths.len() as u32)
            > BROWSER_UPLOAD_MAX_STAGED_FILES
        {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Browser upload staging file budget exceeded",
            ));
        }
        let grant = self
            .filesystem_grants
            .iter()
            .find(|grant| grant.name == root_name && grant.read)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PolicyDenied,
                    "No readable owner filesystem grant is available for this upload root",
                )
            })?;
        let root = Root::open(&grant.path, true, false)?;
        let staging = instance
            .profile
            .join("uploads")
            .join(format!("upload-{}", unique_id()));
        private_directory(&instance.profile.join("uploads"))?;
        private_directory(&staging)?;
        let mut output = Vec::new();
        let mut total = 0u64;
        let mut names = std::collections::BTreeSet::new();
        let result = (|| -> Result<()> {
            for value in paths {
                let text = value
                    .as_str()
                    .ok_or_else(|| Error::invalid("Upload paths must be strings"))?;
                let relative = Path::new(text);
                validate_relative_path(relative)?;
                let name = relative
                    .file_name()
                    .and_then(|value| value.to_str())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| Error::invalid("Upload path has no UTF-8 filename"))?;
                if !names.insert(name.to_owned()) {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Upload paths must have unique filenames",
                    ));
                }
                let bytes = root.read(relative, BROWSER_UPLOAD_MAX_FILE_BYTES)?;
                total = total.saturating_add(bytes.len() as u64);
                if instance.staged_upload_bytes.saturating_add(total)
                    > BROWSER_UPLOAD_MAX_TOTAL_BYTES
                {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "Browser upload staging byte budget exceeded",
                    ));
                }
                let destination = staging.join(name);
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&destination)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                output.push(destination);
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(error);
        }
        Ok((staging, output, total))
    }
    fn cleanup_artifacts(&self) {
        let paths = self
            .artifacts
            .lock()
            .map(|mut artifacts| std::mem::take(&mut *artifacts))
            .unwrap_or_default();
        for path in paths.keys() {
            remove_owned_regular(path);
        }
    }
    async fn cleanup_instance(&self, mut instance: Instance, graceful: bool) -> Result<()> {
        if graceful && instance.cdp.alive.load(Ordering::SeqCst) {
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                instance.cdp.call("Browser.close", json!({}), None),
            )
            .await;
        }
        instance.cdp.stop.cancel();
        if instance.child.try_wait()?.is_none()
            && tokio::time::timeout(std::time::Duration::from_secs(5), instance.child.wait())
                .await
                .is_err()
        {
            let _ = instance.child.kill().await;
            let _ = instance.child.wait().await;
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            match std::fs::remove_dir_all(&instance.profile) {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(_) if std::time::Instant::now() < deadline => {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(_) => {
                    return Err(Error::new(
                        ErrorCode::BackendFailed,
                        "Failed to remove the owned Chromium profile after shutdown",
                    ));
                }
            }
        }
        self.cleanup_artifacts();
        Ok(())
    }
    async fn launch(&self, state: &mut Option<Instance>, headless: bool) -> Result<Value> {
        if let Some(existing) = state.as_mut() {
            let dead =
                existing.child.try_wait()?.is_some() || !existing.cdp.alive.load(Ordering::SeqCst);
            if !dead {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "An isolated browser is already running",
                ));
            }
            let stale = state.take().expect("dead browser instance exists");
            self.cleanup_instance(stale, false).await?;
        }
        if current_uid() == 0 {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Browser adapter refuses root; it never adds --no-sandbox",
            ));
        }
        let meta = std::fs::metadata(&self.config.executable)
            .map_err(|_| Error::unavailable("Configured Chromium executable not found"))?;
        if !meta.is_file() || meta.mode() & 0o022 != 0 {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Browser executable is writable by another user or is not a regular file",
            ));
        }
        let profile = self.directory.join(format!("profile-{}", unique_id()));
        private_directory(&profile)?;
        let downloads = profile.join("downloads");
        private_directory(&downloads)?;
        let mut command = Command::new(&self.config.executable);
        command
            .arg(format!("--user-data-dir={}", profile.display()))
            .args([
                "--remote-debugging-address=127.0.0.1",
                "--remote-debugging-port=0",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-sync",
                "--disable-extensions",
                "--password-store=basic",
                "--disk-cache-size=33554432",
            ]);
        if headless {
            command.arg("--headless=new");
        }
        command
            .arg("about:blank")
            .env_clear()
            .env("HOME", &profile)
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8");
        for key in [
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
            "XAUTHORITY",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|_| Error::unavailable("Failed to launch configured Chromium"))?;
        let started = std::time::Instant::now();
        let active = profile.join("DevToolsActivePort");
        let endpoint = loop {
            if child.try_wait()?.is_some() {
                let _ = std::fs::remove_dir_all(&profile);
                return Err(Error::unavailable(
                    "Chromium exited before opening CDP; verify the browser sandbox manually",
                ));
            }
            if started.elapsed() > BROWSER_STARTUP_TIMEOUT {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = std::fs::remove_dir_all(&profile);
                return Err(Error::new(
                    ErrorCode::Timeout,
                    "Isolated browser startup timed out",
                ));
            }
            if let Ok(text) = tokio::fs::read_to_string(&active).await
                && text.len() <= 4096
                && let Some((port, path)) = text.trim().split_once('\n')
                && let Ok(port) = port.parse::<u16>()
                && port > 0
                && path.starts_with("/devtools/browser/")
                && path
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/-_".contains(&b))
            {
                break format!("ws://127.0.0.1:{port}{path}");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        let cdp = Cdp::connect(
            &endpoint,
            DownloadLimits::from(&self.config),
            downloads.clone(),
        )
        .await?;
        cdp.call("Browser.setDownloadBehavior",json!({"behavior":if self.config.allow_downloads{"allowAndName"}else{"deny"},"downloadPath":downloads,"eventsEnabled":true}),None).await?;
        *state = Some(Instance {
            child,
            profile,
            downloads,
            epoch: unique_id(),
            cdp,
            sessions: BTreeMap::new(),
            staged_upload_bytes: 0,
            staged_upload_files: 0,
            closed_tabs: VecDeque::new(),
        });
        Ok(
            json!({"launched":true,"isolated_profile":true,"headless":headless,"browser_sandbox_disabled":false,"downloads_enabled":self.config.allow_downloads}),
        )
    }
    fn target_parts(target: &NativeTarget) -> Result<(String, Option<u64>, Option<String>)> {
        match target.kind.as_str() {
            "tab" => Ok((target.identity.clone(), None, None)),
            "dom" => {
                let (tab, identity) = target.identity.split_once('#').ok_or_else(|| {
                    Error::new(ErrorCode::StaleReference, "Invalid DOM reference")
                })?;
                if let Some(context) = identity.strip_prefix("frame:") {
                    let (frame, id) = context.split_once("#node:").ok_or_else(|| {
                        Error::new(
                            ErrorCode::StaleReference,
                            "Invalid frame-scoped DOM reference",
                        )
                    })?;
                    if frame.is_empty() || frame.len() > 256 {
                        return Err(Error::new(
                            ErrorCode::StaleReference,
                            "Invalid DOM frame identity",
                        ));
                    }
                    return Ok((
                        tab.into(),
                        Some(id.parse().map_err(|_| {
                            Error::new(ErrorCode::StaleReference, "Invalid DOM identity")
                        })?),
                        Some(frame.into()),
                    ));
                }
                Ok((
                    tab.into(),
                    Some(identity.parse().map_err(|_| {
                        Error::new(ErrorCode::StaleReference, "Invalid DOM identity")
                    })?),
                    None,
                ))
            }
            "frame" => {
                let (tab, frame) = target.identity.split_once("#frame:").ok_or_else(|| {
                    Error::new(ErrorCode::StaleReference, "Invalid frame reference")
                })?;
                if frame.is_empty() || frame.len() > 256 {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "Invalid frame identity",
                    ));
                }
                Ok((tab.into(), None, Some(frame.into())))
            }
            _ => Err(Error::new(
                ErrorCode::InvalidArgument,
                "A browser tab, frame or DOM reference is required",
            )),
        }
    }
    async fn validate_in(
        &self,
        instance: &mut Instance,
        target: &NativeTarget,
    ) -> Result<(String, Option<u64>, Option<String>, String)> {
        if target.fingerprint != instance.epoch {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Browser instance has changed",
            ));
        }
        let (tab, node, frame) = Self::target_parts(target)?;
        instance.check_tab(&tab, &self.config).await?;
        let parent_session = instance.session(&tab).await?;
        let mut session = parent_session.clone();

        if let Some(frame_id) = frame.as_deref() {
            let tree = instance
                .cdp
                .call("Page.getFrameTree", json!({}), Some(&parent_session))
                .await?;
            let frame_tree = frame_tree_find(&tree["frameTree"], frame_id).ok_or_else(|| {
                Error::new(ErrorCode::StaleReference, "Browser frame no longer exists")
            })?;
            let frame_url = frame_tree["frame"]["url"].as_str().unwrap_or("");
            self.config.check_url(frame_url)?;
            if frame_document_revision(&frame_tree["frame"]) != target.revision {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "Frame document changed; obtain a fresh semantic reference",
                ));
            }
            session = instance
                .cdp
                .attached_session(frame_id)?
                .unwrap_or_else(|| parent_session.clone());
        } else if node.is_some() && instance.cdp.generation(&session)? != target.revision {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Browser document context changed; obtain a fresh semantic reference",
            ));
        }

        if let Some(node) = node {
            instance
                .cdp
                .call(
                    "DOM.describeNode",
                    json!({"backendNodeId":node,"depth":0}),
                    Some(&session),
                )
                .await
                .map_err(|_| {
                    Error::new(
                        ErrorCode::StaleReference,
                        "DOM node is no longer addressable in its frame context",
                    )
                })?;
            if frame.is_none() && instance.cdp.generation(&session)? != target.revision {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "DOM changed during validation",
                ));
            }
        }
        Ok((tab, node, frame, session))
    }
    async fn artifact(&self, encoded: &str) -> Result<Value> {
        if encoded.len() > 44 * 1024 * 1024 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Screenshot exceeds 32 MiB budget",
            ));
        }
        let data = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "Browser screenshot is invalid base64",
                )
            })?;
        if !data.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Browser screenshot is not PNG",
            ));
        }
        let path = self
            .directory
            .join(format!("artifact-screen-{}.png", unique_id()));
        let mut artifacts = self
            .artifacts
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Artifact registry lock poisoned"))?;
        artifacts.retain(|path, _| path.exists());
        let total = artifacts.values().copied().sum::<u64>();
        if artifacts.len() >= 16 || total.saturating_add(data.len() as u64) > 64 * 1024 * 1024 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Browser artifact budget exceeded",
            ));
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(&data)?;
        file.sync_all()?;
        artifacts.insert(path.clone(), data.len() as u64);
        drop(artifacts);
        let expiry = path.clone();
        let registry = self.artifacts.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
            remove_owned_regular(&expiry);
            if let Ok(mut artifacts) = registry.lock() {
                artifacts.remove(&expiry);
            }
        });
        Ok(
            json!({"artifact":{"path":path,"mime_type":"image/png","expires_in_seconds":300,"bytes":data.len()},"coordinate_space":"viewport_css_pixels","scale":"browser_device_scale_factor_not_assumed"}),
        )
    }
}
fn frame_document_revision(frame: &Value) -> u64 {
    // FNV-1a is sufficient here: this is an ephemeral equality token, not a
    // cryptographic authority. LoaderId changes on cross-document navigation.
    let mut hash = 0xcbf29ce484222325u64;
    let identity = frame["loaderId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| frame["id"].as_str())
        .unwrap_or("");
    for byte in identity.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
fn frame_tree_contains(tree: &Value, frame_id: &str) -> bool {
    frame_tree_find(tree, frame_id).is_some()
}

fn frame_tree_find<'a>(tree: &'a Value, frame_id: &str) -> Option<&'a Value> {
    if tree["frame"]["id"].as_str() == Some(frame_id) {
        return Some(tree);
    }
    tree["childFrames"]
        .as_array()?
        .iter()
        .find_map(|child| frame_tree_find(child, frame_id))
}

fn collect_frame_rows(
    instance: &Instance,
    config: &BrowserConfig,
    target: &str,
    tree: &Value,
    parent: Option<&str>,
    rows: &mut Vec<Value>,
) -> Result<()> {
    if rows.len() >= 256 {
        return Ok(());
    }
    let frame = &tree["frame"];
    let id = arg_str(frame, "id")?;
    let url = frame["url"].as_str().unwrap_or("");
    rows.push(json!({
        "ref": instance.frame_ref(target, frame)?,
        "name": frame["name"].as_str().unwrap_or("").chars().take(256).collect::<String>(),
        "url_origin": Url::parse(url).ok().map(|value| value.origin().ascii_serialization()),
        "allowed_origin": config.check_url(url).is_ok(),
        "parent_frame_id": parent,
        "secure_context_type": frame["secureContextType"].as_str(),
        "mime_type": frame["mimeType"].as_str()
    }));
    if let Some(children) = tree["childFrames"].as_array() {
        for child in children {
            collect_frame_rows(instance, config, target, child, Some(id), rows)?;
            if rows.len() >= 256 {
                break;
            }
        }
    }
    Ok(())
}

fn attribute(node: &Value, name: &str) -> Option<String> {
    node["attributes"]
        .as_array()?
        .as_chunks::<2>()
        .0
        .iter()
        .find(|pair| pair[0].as_str() == Some(name))
        .and_then(|pair| pair[1].as_str())
        .map(str::to_owned)
}
fn ax_text(node: &Value, key: &str) -> Option<String> {
    node[key]["value"]
        .as_str()
        .map(|value| value.chars().take(512).collect())
}

fn ax_property(node: &Value, name: &str) -> Option<Value> {
    let raw = node["properties"]
        .as_array()?
        .iter()
        .find(|entry| entry["name"].as_str() == Some(name))?
        .pointer("/value/value")?;
    match raw {
        Value::Bool(_) | Value::Number(_) => Some(raw.clone()),
        Value::String(value) => Some(json!(value.chars().take(512).collect::<String>())),
        _ => None,
    }
}

fn compact_ax(
    instance: &Instance,
    target: &str,
    session: &str,
    frame: Option<&str>,
    frame_revision: Option<u64>,
    node: &Value,
) -> Result<Value> {
    let role = ax_text(node, "role").unwrap_or_default();
    let backend = node["backendDOMNodeId"].as_u64();
    let reference = backend
        .map(|id| {
            instance.dom_ref_in_frame(
                target,
                &json!({"backendNodeId":id}),
                session,
                frame,
                frame_revision,
            )
        })
        .transpose()?;
    let mut states = serde_json::Map::new();
    for key in [
        "disabled",
        "focused",
        "selected",
        "checked",
        "expanded",
        "required",
        "readonly",
        "editable",
        "multiselectable",
        "orientation",
        "haspopup",
        "invalid",
        "valuemin",
        "valuemax",
        "autocomplete",
        "level",
    ] {
        if let Some(value) = ax_property(node, key) {
            states.insert(key.into(), value);
        }
    }
    let mut actions = Vec::new();
    if reference.is_some() {
        actions.extend(["focus", "scroll", "hover"]);
        if matches!(
            role.as_str(),
            "button"
                | "link"
                | "menuitem"
                | "checkbox"
                | "radio"
                | "switch"
                | "tab"
                | "treeitem"
                | "option"
        ) {
            actions.extend(["click", "press"]);
        }
        if matches!(role.as_str(), "textbox" | "searchbox" | "spinbutton") {
            actions.extend(["fill", "press"]);
        }
        if matches!(role.as_str(), "combobox" | "listbox") {
            actions.extend(["select", "press"]);
        }
        if matches!(role.as_str(), "checkbox" | "radio" | "switch") {
            actions.push("check");
        }
    }
    Ok(json!({
        "ax_id": node["nodeId"].as_str().unwrap_or(""),
        "ignored": node["ignored"].as_bool().unwrap_or(false),
        "role": role,
        "name": ax_text(node, "name"),
        "description": ax_text(node, "description"),
        "value": ax_text(node, "value"),
        "states": states,
        "actions": actions,
        "ref": reference,
        "frame_id": node["frameId"].as_str(),
        "parent_ax_id": node["parentId"].as_str(),
        "child_ax_ids": node["childIds"].as_array().map(|items| items.iter().take(64).cloned().collect::<Vec<_>>()).unwrap_or_default()
    }))
}

fn semantic_query_matches(row: &Value, args: &Value) -> bool {
    if let Some(role) = args["role"].as_str()
        && !row["role"]
            .as_str()
            .is_some_and(|value| value.eq_ignore_ascii_case(role))
    {
        return false;
    }
    if let Some(name) = args["name"].as_str() {
        let Some(actual) = row["name"].as_str() else {
            return false;
        };
        if args["exact_name"].as_bool().unwrap_or(false) {
            if actual != name {
                return false;
            }
        } else if !actual
            .to_ascii_lowercase()
            .contains(&name.to_ascii_lowercase())
        {
            return false;
        }
    }
    if let Some(action) = args["action"].as_str()
        && !row["actions"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(action)))
    {
        return false;
    }
    true
}

fn key_spec(name: &str) -> Option<(&'static str, &'static str, u64)> {
    Some(match name {
        "Enter" => ("Enter", "Enter", 13),
        "Tab" => ("Tab", "Tab", 9),
        "Escape" => ("Escape", "Escape", 27),
        "Space" => (" ", "Space", 32),
        "ArrowUp" => ("ArrowUp", "ArrowUp", 38),
        "ArrowDown" => ("ArrowDown", "ArrowDown", 40),
        "ArrowLeft" => ("ArrowLeft", "ArrowLeft", 37),
        "ArrowRight" => ("ArrowRight", "ArrowRight", 39),
        "Home" => ("Home", "Home", 36),
        "End" => ("End", "End", 35),
        "PageUp" => ("PageUp", "PageUp", 33),
        "PageDown" => ("PageDown", "PageDown", 34),
        _ => return None,
    })
}

fn modifier_mask(args: &Value) -> Result<u64> {
    let Some(items) = args["modifiers"].as_array() else {
        return Ok(0);
    };
    if items.len() > 4 {
        return Err(Error::invalid(
            "At most four keyboard modifiers are allowed",
        ));
    }
    let mut mask = 0u64;
    for item in items {
        mask |= match item.as_str() {
            Some("Alt") => 1,
            Some("Control") => 2,
            Some("Meta") => 4,
            Some("Shift") => 8,
            _ => return Err(Error::invalid("Unknown keyboard modifier")),
        };
    }
    Ok(mask)
}

async fn dispatch_key(cdp: &Cdp, session: &str, name: &str, modifiers: u64) -> Result<()> {
    let (key, code, virtual_key) =
        key_spec(name).ok_or_else(|| Error::invalid("Unsupported semantic key"))?;
    for kind in ["keyDown", "keyUp"] {
        cdp.call(
            "Input.dispatchKeyEvent",
            json!({
                "type":kind,
                "key":key,
                "code":code,
                "windowsVirtualKeyCode":virtual_key,
                "nativeVirtualKeyCode":virtual_key,
                "modifiers":modifiers
            }),
            Some(session),
        )
        .await?;
    }
    Ok(())
}

async fn focus_backend_node(cdp: &Cdp, session: &str, node: u64) -> Result<()> {
    cdp.call("DOM.focus", json!({"backendNodeId":node}), Some(session))
        .await?;
    let doc = cdp
        .call("DOM.getDocument", json!({"depth":0}), Some(session))
        .await?;
    let focused = cdp
        .call(
            "DOM.querySelector",
            json!({"nodeId":doc["root"]["nodeId"],"selector":":focus"}),
            Some(session),
        )
        .await?;
    let focused = cdp
        .call(
            "DOM.describeNode",
            json!({"nodeId":focused["nodeId"],"depth":0}),
            Some(session),
        )
        .await?;
    if focused["node"]["backendNodeId"].as_u64() != Some(node) {
        return Err(Error::new(ErrorCode::Conflict, "DOM focus changed"));
    }
    Ok(())
}

async fn actionable_point(cdp: &Cdp, session: &str, node: u64) -> Result<(f64, f64)> {
    let model = cdp
        .call(
            "DOM.getBoxModel",
            json!({"backendNodeId":node}),
            Some(session),
        )
        .await?;
    let quad = model["model"]["content"]
        .as_array()
        .filter(|quad| quad.len() == 8)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::NotFound,
                "DOM node has no actionable content quad",
            )
        })?;
    let coordinates: Vec<f64> = quad
        .iter()
        .map(|value| value.as_f64().unwrap_or(f64::NAN))
        .collect();
    let x = (coordinates[0] + coordinates[2] + coordinates[4] + coordinates[6]) / 4.0;
    let y = (coordinates[1] + coordinates[3] + coordinates[5] + coordinates[7]) / 4.0;
    if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 || x > 16384.0 || y > 16384.0 {
        return Err(Error::new(
            ErrorCode::Conflict,
            "DOM action coordinate is outside the bounded viewport",
        ));
    }
    let hit = cdp
        .call(
            "DOM.getNodeForLocation",
            json!({
                "x":x.round() as i32,
                "y":y.round() as i32,
                "includeUserAgentShadowDOM":false,
                "ignorePointerEventsNone":false
            }),
            Some(session),
        )
        .await?;
    if hit["backendNodeId"].as_u64() != Some(node) {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Another DOM element covers the target; choose the precise hit element explicitly",
        ));
    }
    Ok((x, y))
}

fn checked_state(node: &Value) -> Option<bool> {
    match ax_property(node, "checked")? {
        Value::Bool(value) => Some(value),
        Value::String(value) if value == "true" => Some(true),
        Value::String(value) if value == "false" => Some(false),
        _ => None,
    }
}

async fn ax_node_for_backend(cdp: &Cdp, session: &str, node: u64) -> Result<Value> {
    let result = cdp
        .call(
            "Accessibility.getPartialAXTree",
            json!({"backendNodeId":node,"fetchRelatives":false}),
            Some(session),
        )
        .await?;
    result["nodes"]
        .as_array()
        .and_then(|nodes| {
            nodes
                .iter()
                .find(|candidate| candidate["backendDOMNodeId"].as_u64() == Some(node))
                .or_else(|| nodes.first())
        })
        .cloned()
        .ok_or_else(|| Error::new(ErrorCode::NotFound, "No accessibility node found"))
}

fn dom_node_text(node: &Value, remaining: &mut usize, out: &mut String) {
    if *remaining == 0 || out.len() >= 512 {
        return;
    }
    *remaining -= 1;
    let value = node["nodeValue"].as_str().unwrap_or("").trim();
    if !value.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(value.chars().take(512usize.saturating_sub(out.len())));
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            dom_node_text(child, remaining, out);
            if *remaining == 0 || out.len() >= 512 {
                break;
            }
        }
    }
}

fn collect_select_options(node: &Value, rows: &mut Vec<(usize, String)>) -> Result<()> {
    if node["nodeName"].as_str() == Some("OPTION") {
        if rows.len() >= 200 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Select contains more than 200 enabled options",
            ));
        }
        if attribute(node, "disabled").is_some() {
            return Ok(());
        }
        let mut label = attribute(node, "label").unwrap_or_default();
        if label.is_empty() {
            let mut budget = 16;
            dom_node_text(node, &mut budget, &mut label);
        }
        let index = rows.len();
        rows.push((index, label));
        return Ok(());
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            collect_select_options(child, rows)?;
        }
    }
    Ok(())
}

fn select_options(node: &Value) -> Result<Vec<(usize, String)>> {
    let mut rows = Vec::new();
    collect_select_options(node, &mut rows)?;
    Ok(rows)
}

fn compact_dom(
    instance: &Instance,
    target: &str,
    session: &str,
    revision: u64,
    node: &Value,
    remaining: &mut usize,
) -> Result<Option<Value>> {
    if *remaining == 0 {
        return Ok(None);
    }
    *remaining -= 1;
    let name = node["nodeName"].as_str().unwrap_or("");
    if matches!(name, "SCRIPT" | "STYLE" | "NOSCRIPT") {
        return Ok(None);
    }
    let mut row = json!({"node_name":name,"text":node["nodeValue"].as_str().unwrap_or("").chars().take(512).collect::<String>(),"ref":instance.dom_ref_in_frame(target,node,session,None,Some(revision))?});
    let mut attrs = serde_json::Map::new();
    for key in [
        "id",
        "class",
        "role",
        "name",
        "type",
        "aria-label",
        "aria-disabled",
        "disabled",
    ] {
        if let Some(value) = attribute(node, key) {
            attrs.insert(
                key.into(),
                json!(value.chars().take(512).collect::<String>()),
            );
        }
    }
    row["attributes"] = Value::Object(attrs);
    let mut children = vec![];
    for key in ["children", "shadowRoots"] {
        if let Some(nodes) = node[key].as_array() {
            for child in nodes {
                if let Some(v) = compact_dom(instance, target, session, revision, child, remaining)?
                {
                    children.push(v);
                }
                if *remaining == 0 {
                    break;
                }
            }
        }
    }
    if *remaining > 0
        && let Some(document) = node.get("contentDocument")
        && document.is_object()
        && let Some(v) = compact_dom(instance, target, session, revision, document, remaining)?
    {
        children.push(v);
    }
    row["children"] = json!(children);
    Ok(Some(row))
}
#[async_trait]
impl Backend for Chromium {
    fn name(&self) -> &'static str {
        "chromium"
    }
    fn supports(&self, command: &str) -> bool {
        command.starts_with("browser.")
    }
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| {
            match command {
                "browser.status" => "browser.status",
                "browser.launch" => "browser.launch",
                _ => "browser.running",
            }
            .into()
        })
    }
    async fn probe(&self) -> Vec<Feature> {
        let executable = std::fs::metadata(&self.config.executable)
            .is_ok_and(|meta| meta.is_file() && meta.mode() & 0o022 == 0);
        let mut instance = self.instance.lock().await;
        let running = instance.as_mut().is_some_and(|instance| {
            instance.child.try_wait().is_ok_and(|exit| exit.is_none())
                && instance.cdp.alive.load(Ordering::SeqCst)
                && !instance.cdp.stop.is_cancelled()
        });
        vec![
            semwright_backend_api::feature(
                self.name(),
                "browser.status",
                true,
                "Diagnostic only; does not attach to a normal browser profile",
                "",
            ),
            semwright_backend_api::feature(
                self.name(),
                "browser.launch",
                executable && current_uid() != 0 && !running,
                "A safe executable and an unused owned browser slot are required",
                "Configure an installed Chromium executable and run as a normal user",
            ),
            semwright_backend_api::feature(
                self.name(),
                "browser.running",
                running,
                "An owned live browser process is required; origin and node context are revalidated per call",
                "Launch a disposable browser through Semwright first",
            ),
        ]
    }
    async fn execute(&self, ctx: &Context, command: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        let mut state = self.instance.lock().await;
        if command == "browser.status" {
            let running = state.as_mut().is_some_and(|instance| {
                instance
                    .child
                    .try_wait()
                    .is_ok_and(|status| status.is_none())
                    && instance.cdp.alive.load(Ordering::SeqCst)
            });
            return Ok(json!({
                "running": running,
                "isolated_profile": true,
                "attach_existing": false,
                "arbitrary_javascript": false,
                "origin_scope": "top-level command targets, not a network firewall",
                "downloads_enabled": self.config.allow_downloads,
                "download_limits": {
                    "per_file_bytes": self.config.max_download_bytes,
                    "total_bytes": self.config.max_total_download_bytes,
                    "count": self.config.max_downloads
                }
            }));
        }
        if command == "browser.launch" {
            return self
                .launch(&mut state, args["headless"].as_bool().unwrap_or(true))
                .await;
        }
        let dead = state.as_mut().is_some_and(|instance| {
            instance
                .child
                .try_wait()
                .is_ok_and(|status| status.is_some())
                || !instance.cdp.alive.load(Ordering::SeqCst)
        });
        if dead {
            let stale = state.take().expect("dead browser instance exists");
            self.cleanup_instance(stale, false).await?;
            return Err(Error::unavailable(
                "The isolated browser exited; owned state was cleaned and may be relaunched",
            ));
        }
        let instance = state.as_mut().ok_or_else(|| {
            Error::unavailable("No isolated browser is running; use browser.launch with approval")
        })?;
        if command == "browser.tab.list" {
            let mut rows = vec![];
            for target in instance.targets().await? {
                let id = arg_str(&target, "targetId")?;
                rows.push(json!({"ref":instance.tab_ref(id),"title":target["title"].as_str().unwrap_or("").chars().take(512).collect::<String>(),"url_origin":Url::parse(target["url"].as_str().unwrap_or("")).ok().map(|url|url.origin().ascii_serialization()),"allowed_origin":self.config.check_url(target["url"].as_str().unwrap_or("")).is_ok()}));
            }
            return Ok(json!({"tabs":rows}));
        }
        if matches!(command, "browser.downloads.status" | "browser.diagnostics") {
            let mut events = instance.cdp.metadata()?;
            if command == "browser.downloads.status" {
                events.retain(|event| {
                    event["event"]
                        .as_str()
                        .is_some_and(|s| s.starts_with("Browser.download"))
                });
            }
            if command == "browser.downloads.status" {
                let snapshot = instance.cdp.download_snapshot()?;
                return Ok(json!({
                    "events": events,
                    "artifacts": instance.download_artifacts(DownloadLimits::from(&self.config))?,
                    "bounded_event_history": 256,
                    "enabled": self.config.allow_downloads,
                    "payloads_redacted": true,
                    "automatic_download_size_limit": true,
                    "limits": {
                        "per_file_bytes": self.config.max_download_bytes,
                        "total_bytes": self.config.max_total_download_bytes,
                        "count": self.config.max_downloads
                    },
                    "started": snapshot.started,
                    "total_received_bytes": snapshot.total_received,
                    "quota_cancellations": snapshot.quota_cancellations
                }));
            }
            return Ok(json!({
                "events": events,
                "bounded_event_history": 256,
                "payloads_redacted": true
            }));
        }
        if command == "browser.tab.open" {
            let url = arg_str(args, "url")?;
            self.config.check_url(url)?;
            let result = instance
                .cdp
                .call("Target.createTarget", json!({"url":"about:blank"}), None)
                .await?;
            let tab = arg_str(&result, "targetId")?.to_owned();
            let session = instance.session(&tab).await?;
            let navigation = instance
                .cdp
                .call("Page.navigate", json!({"url":url}), Some(&session))
                .await?;
            if navigation.get("errorText").is_some() {
                return Err(
                    Error::new(ErrorCode::BackendFailed, "Browser navigation failed").uncertain(),
                );
            }
            return Ok(json!({"ref":instance.tab_ref(&tab),"changed":true}));
        }
        let target = native_target(args)?;
        let (tab, node, frame, session) = self.validate_in(instance, &target).await?;
        let cdp = instance.cdp.clone();
        match command {
            "browser.tab.close" => {
                if node.is_some() || frame.is_some() {
                    return Err(Error::invalid("Tab reference required"));
                }
                let result = cdp
                    .call("Target.closeTarget", json!({"targetId":tab}), None)
                    .await?;
                if result["success"] == true {
                    instance.sessions.remove(&tab);
                    if instance.closed_tabs.len() == 256 {
                        instance.closed_tabs.pop_front();
                    }
                    instance.closed_tabs.push_back(tab);
                }
                Ok(json!({"changed":result["success"]==true}))
            }
            "browser.tab.focus" => {
                if node.is_some() || frame.is_some() {
                    return Err(Error::invalid("Tab reference required"));
                }
                cdp.call("Target.activateTarget", json!({"targetId":tab}), None)
                    .await?;
                Ok(json!({"accepted":true}))
            }
            "browser.navigate" => {
                if node.is_some() || frame.is_some() {
                    return Err(Error::invalid("Tab reference required"));
                }
                let url = arg_str(args, "url")?;
                self.config.check_url(url)?;
                // Navigation is a known identity boundary. Invalidate current DOM refs
                // before dispatch so a partial/late navigation cannot leave them reusable.
                cdp.invalidate_session(&session)?;
                let result = cdp
                    .call("Page.navigate", json!({"url":url}), Some(&session))
                    .await?;
                if result.get("errorText").is_some() {
                    return Err(
                        Error::new(ErrorCode::BackendFailed, "Navigation failed").uncertain()
                    );
                }
                Ok(json!({"changed":true}))
            }
            "browser.page.reload" => {
                if node.is_some() || frame.is_some() {
                    return Err(Error::invalid("Tab reference required"));
                }
                cdp.invalidate_session(&session)?;
                cdp.call(
                    "Page.reload",
                    json!({"ignoreCache":args["ignore_cache"].as_bool().unwrap_or(false)}),
                    Some(&session),
                )
                .await?;
                Ok(json!({"changed":true}))
            }
            "browser.page.stop" => {
                if node.is_some() || frame.is_some() {
                    return Err(Error::invalid("Tab reference required"));
                }
                cdp.call("Page.stopLoading", json!({}), Some(&session))
                    .await?;
                Ok(json!({"accepted":true}))
            }
            "browser.page.scroll" => {
                if node.is_some() || frame.is_some() {
                    return Err(Error::invalid("Tab reference required"));
                }
                let direction = arg_str(args, "direction")?;
                let amount = args["amount"].as_str().unwrap_or("page");
                let before = cdp
                    .call("Page.getLayoutMetrics", json!({}), Some(&session))
                    .await?;
                let viewport = &before["visualViewport"];
                let width = viewport["clientWidth"].as_f64().unwrap_or(0.0);
                let height = viewport["clientHeight"].as_f64().unwrap_or(0.0);
                let before_x = viewport["pageX"].as_f64().unwrap_or(0.0);
                let before_y = viewport["pageY"].as_f64().unwrap_or(0.0);
                if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
                    return Err(Error::new(
                        ErrorCode::BackendFailed,
                        "Browser visual viewport is unavailable",
                    ));
                }
                let factor = match amount {
                    "small" => 0.25,
                    "half_page" => 0.5,
                    "page" => 0.9,
                    _ => return Err(Error::invalid("Unknown semantic scroll amount")),
                };
                let vertical = (height * factor).clamp(1.0, 4096.0);
                let horizontal = (width * factor).clamp(1.0, 4096.0);
                let (delta_x, delta_y) = match direction {
                    "up" => (0.0, -vertical),
                    "down" => (0.0, vertical),
                    "left" => (-horizontal, 0.0),
                    "right" => (horizontal, 0.0),
                    _ => return Err(Error::invalid("Unknown semantic scroll direction")),
                };
                ctx.check_cancelled()?;
                cdp.call(
                    "Input.dispatchMouseEvent",
                    json!({
                        "type":"mouseWheel",
                        "x":width / 2.0,
                        "y":height / 2.0,
                        "deltaX":delta_x,
                        "deltaY":delta_y,
                        "button":"none"
                    }),
                    Some(&session),
                )
                .await?;
                let mut after_x = before_x;
                let mut after_y = before_y;
                for _ in 0..10 {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    ctx.check_cancelled()?;
                    let after = cdp
                        .call("Page.getLayoutMetrics", json!({}), Some(&session))
                        .await?;
                    after_x = after["visualViewport"]["pageX"]
                        .as_f64()
                        .unwrap_or(before_x);
                    after_y = after["visualViewport"]["pageY"]
                        .as_f64()
                        .unwrap_or(before_y);
                    if (after_x - before_x).abs() > 0.5 || (after_y - before_y).abs() > 0.5 {
                        break;
                    }
                }
                Ok(json!({
                    "accepted":true,
                    "changed":(after_x-before_x).abs()>0.5 || (after_y-before_y).abs()>0.5,
                    "direction":direction,
                    "amount":amount,
                    "method":"viewport metrics + bounded CDP wheel"
                }))
            }
            "browser.page.back" | "browser.page.forward" => {
                if node.is_some() || frame.is_some() {
                    return Err(Error::invalid("Tab reference required"));
                }
                let history = cdp
                    .call("Page.getNavigationHistory", json!({}), Some(&session))
                    .await?;
                let current = history["currentIndex"].as_i64().ok_or_else(|| {
                    Error::new(ErrorCode::BackendFailed, "Browser history index is missing")
                })?;
                let next = if command == "browser.page.back" {
                    current.saturating_sub(1)
                } else {
                    current.saturating_add(1)
                };
                let Some(entry) = history["entries"].as_array().and_then(|entries| {
                    usize::try_from(next)
                        .ok()
                        .and_then(|index| entries.get(index))
                }) else {
                    return Ok(json!({"changed":false}));
                };
                let entry_id = entry["id"].as_i64().ok_or_else(|| {
                    Error::new(
                        ErrorCode::BackendFailed,
                        "Browser history entry is malformed",
                    )
                })?;
                cdp.invalidate_session(&session)?;
                cdp.call(
                    "Page.navigateToHistoryEntry",
                    json!({"entryId":entry_id}),
                    Some(&session),
                )
                .await?;
                Ok(json!({"changed":true}))
            }
            "browser.dialog.status" => {
                if node.is_some() || frame.is_some() {
                    return Err(Error::invalid("Tab reference required"));
                }
                let mut latest: Option<Value> = None;
                for event in cdp.metadata()? {
                    if event["session"].as_str() != Some(session.as_str()) {
                        continue;
                    }
                    if matches!(
                        event["event"].as_str(),
                        Some("Page.javascriptDialogOpening" | "Page.javascriptDialogClosed")
                    ) {
                        latest = Some(event);
                    }
                }
                let open = latest.as_ref().and_then(|event| event["event"].as_str())
                    == Some("Page.javascriptDialogOpening");
                Ok(json!({
                    "open":open,
                    "dialog_type":if open { latest.as_ref().and_then(|event|event["dialog_type"].as_str()) } else { None },
                    "message":if open { latest.as_ref().and_then(|event|event["message"].as_str()) } else { None },
                    "prompt_value_redacted":true
                }))
            }
            "browser.dialog.respond" => {
                if node.is_some() || frame.is_some() {
                    return Err(Error::invalid("Tab reference required"));
                }
                let accept = args["accept"].as_bool().unwrap_or(false);
                if !accept && args.get("prompt_text").is_some() {
                    return Err(Error::invalid(
                        "prompt_text is only valid when accepting a prompt",
                    ));
                }
                let mut params = json!({"accept":accept});
                if let Some(prompt) = args["prompt_text"].as_str() {
                    params["promptText"] = json!(prompt);
                }
                ctx.check_cancelled()?;
                cdp.invalidate_session(&session)?;
                cdp.call("Page.handleJavaScriptDialog", params, Some(&session))
                    .await?;
                Ok(json!({"accepted":accept,"handled":true}))
            }
            "browser.frame.list" => {
                if node.is_some() {
                    return Err(Error::invalid("Tab or frame reference required"));
                }
                let tree = cdp
                    .call("Page.getFrameTree", json!({}), Some(&session))
                    .await?;
                let root = if let Some(frame_id) = frame.as_deref() {
                    frame_tree_find(&tree["frameTree"], frame_id).ok_or_else(|| {
                        Error::new(ErrorCode::StaleReference, "Browser frame no longer exists")
                    })?
                } else {
                    &tree["frameTree"]
                };
                let mut rows = Vec::new();
                collect_frame_rows(instance, &self.config, &tab, root, None, &mut rows)?;
                let count = rows.len();
                Ok(json!({"frames":rows,"count":count,"truncated":count==256}))
            }
            "browser.semantic.snapshot" | "browser.semantic.query" => {
                if node.is_some() {
                    return Err(Error::invalid("Tab or frame reference required"));
                }
                let mut params = json!({
                    "depth": args["depth"].as_u64().unwrap_or(8).min(32)
                });
                if let Some(frame_id) = frame.as_deref()
                    && cdp.attached_session(frame_id)?.is_none()
                {
                    params["frameId"] = json!(frame_id);
                }
                let result = cdp
                    .call("Accessibility.getFullAXTree", params, Some(&session))
                    .await?;
                let nodes = result["nodes"].as_array().ok_or_else(|| {
                    Error::new(ErrorCode::BackendFailed, "Accessibility tree is malformed")
                })?;
                let snapshot_revision = if frame.is_some() {
                    target.revision
                } else {
                    cdp.generation(&session)?
                };
                let include_ignored = args["include_ignored"].as_bool().unwrap_or(false);
                let limit = if command == "browser.semantic.query" {
                    args["max_results"].as_u64().unwrap_or(50).clamp(1, 200) as usize
                } else {
                    args["max_nodes"].as_u64().unwrap_or(500).clamp(1, 1000) as usize
                };
                let mut rows = Vec::new();
                let mut matched = 0usize;
                for ax in nodes {
                    if !include_ignored && ax["ignored"].as_bool().unwrap_or(false) {
                        continue;
                    }
                    let row = compact_ax(
                        instance,
                        &tab,
                        &session,
                        frame.as_deref(),
                        Some(snapshot_revision),
                        ax,
                    )?;
                    if command == "browser.semantic.query" && !semantic_query_matches(&row, args) {
                        continue;
                    }
                    matched = matched.saturating_add(1);
                    if rows.len() < limit {
                        rows.push(row);
                    }
                }
                if command == "browser.semantic.query" {
                    let single = if matched == 1 {
                        rows.first().cloned()
                    } else {
                        None
                    };
                    return Ok(json!({
                        "matches": rows,
                        "single": single,
                        "count": matched,
                        "truncated": matched > limit,
                        "source": "cdp_accessibility_tree"
                    }));
                }
                Ok(json!({
                    "nodes": rows,
                    "count": matched,
                    "truncated": matched > limit,
                    "source": "cdp_accessibility_tree",
                    "generation": snapshot_revision,
                    "arbitrary_javascript": false
                }))
            }
            "browser.semantic.wait" => {
                if node.is_some() || frame.is_some() {
                    return Err(Error::invalid("Tab reference required for semantic wait"));
                }
                let timeout_ms = args["timeout_ms"]
                    .as_u64()
                    .unwrap_or(5_000)
                    .clamp(1, 30_000);
                let poll_ms = args["poll_ms"].as_u64().unwrap_or(100).clamp(50, 1_000);
                let present = args["present"].as_bool().unwrap_or(true);
                let deadline =
                    std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
                loop {
                    ctx.check_cancelled()?;
                    let generation = cdp.generation(&session)?;
                    let result = cdp
                        .call(
                            "Accessibility.getFullAXTree",
                            json!({"depth":args["depth"].as_u64().unwrap_or(8).min(32)}),
                            Some(&session),
                        )
                        .await?;
                    let nodes = result["nodes"].as_array().ok_or_else(|| {
                        Error::new(ErrorCode::BackendFailed, "Accessibility tree is malformed")
                    })?;
                    let mut rows = Vec::new();
                    for ax in nodes {
                        if ax["ignored"].as_bool().unwrap_or(false) {
                            continue;
                        }
                        let row = compact_ax(
                            instance,
                            &tab,
                            &session,
                            frame.as_deref(),
                            Some(generation),
                            ax,
                        )?;
                        if semantic_query_matches(&row, args) {
                            rows.push(row);
                            if rows.len() == 20 {
                                break;
                            }
                        }
                    }
                    if cdp.generation(&session)? == generation {
                        let satisfied = if present {
                            !rows.is_empty()
                        } else {
                            rows.is_empty()
                        };
                        if satisfied {
                            return Ok(json!({
                                "satisfied":true,
                                "present":present,
                                "matches":rows,
                                "generation":generation
                            }));
                        }
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(Error::new(
                            ErrorCode::Timeout,
                            "Semantic wait timed out before the requested condition was satisfied",
                        ));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
                }
            }
            "browser.semantic.inspect" => {
                let node = node.ok_or_else(|| Error::invalid("DOM reference required"))?;
                let generation = cdp.generation(&session)?;
                let result = cdp
                    .call(
                        "Accessibility.getPartialAXTree",
                        json!({"backendNodeId":node,"fetchRelatives":true}),
                        Some(&session),
                    )
                    .await?;
                let nodes = result["nodes"].as_array().ok_or_else(|| {
                    Error::new(ErrorCode::BackendFailed, "Accessibility node is malformed")
                })?;
                let ax = nodes
                    .iter()
                    .find(|candidate| candidate["backendDOMNodeId"].as_u64() == Some(node))
                    .or_else(|| nodes.first())
                    .ok_or_else(|| {
                        Error::new(ErrorCode::NotFound, "No accessibility node found")
                    })?;
                let semantic = compact_ax(
                    instance,
                    &tab,
                    &session,
                    frame.as_deref(),
                    frame.as_ref().map(|_| target.revision),
                    ax,
                )?;
                let dom = cdp
                    .call(
                        "DOM.describeNode",
                        json!({"backendNodeId":node,"depth":0}),
                        Some(&session),
                    )
                    .await?;
                if cdp.generation(&session)? != generation {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "Element changed while inspecting; repeat discovery",
                    ));
                }
                Ok(json!({
                    "semantic": semantic,
                    "dom": {
                        "node_name": dom["node"]["nodeName"],
                        "local_name": dom["node"]["localName"],
                        "input_type": attribute(&dom["node"], "type"),
                        "id": attribute(&dom["node"], "id"),
                        "class": attribute(&dom["node"], "class")
                    }
                }))
            }
            "browser.dom.snapshot" => {
                if node.is_some() || frame.is_some() {
                    return Err(Error::invalid(
                        "Tab reference required for a document snapshot",
                    ));
                }
                let result = cdp
                    .call(
                        "DOM.getDocument",
                        json!({"depth":args["depth"].as_u64().unwrap_or(4),"pierce":true}),
                        Some(&session),
                    )
                    .await?;
                let revision = cdp.generation(&session)?;
                let mut budget = 500;
                let root = compact_dom(
                    instance,
                    &tab,
                    &session,
                    revision,
                    &result["root"],
                    &mut budget,
                )?;
                Ok(
                    json!({"root":root,"node_budget":500,"truncated":budget==0,"sensitive_attributes_omitted":true,"semantic_coverage":"bounded DOM including open shadow roots and frame documents exposed by CDP; script/style bodies excluded"}),
                )
            }
            "browser.dom.query" => {
                if node.is_some() || frame.is_some() {
                    return Err(Error::invalid("Tab reference required for a CSS query"));
                }
                let doc = cdp
                    .call("DOM.getDocument", json!({"depth":0}), Some(&session))
                    .await?;
                let found=cdp.call("DOM.querySelectorAll",json!({"nodeId":doc["root"]["nodeId"],"selector":arg_str(args,"selector")?}),Some(&session)).await?;
                let ids = found["nodeIds"].as_array().ok_or_else(|| {
                    Error::new(ErrorCode::BackendFailed, "DOM query has no node array")
                })?;
                if ids.len() > 200 {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "CSS query matches more than 200 nodes; narrow the selector",
                    ));
                }
                let revision = cdp.generation(&session)?;
                let mut rows = vec![];
                for id in ids {
                    let detail = cdp
                        .call(
                            "DOM.describeNode",
                            json!({"nodeId":id,"depth":0}),
                            Some(&session),
                        )
                        .await?;
                    rows.push(json!({"ref":instance.dom_ref_in_frame(&tab,&detail["node"],&session,None,Some(revision))?,"node_name":detail["node"]["nodeName"],"name":attribute(&detail["node"],"aria-label").or_else(||attribute(&detail["node"],"name"))}));
                }
                let single = if rows.len() == 1 {
                    Some(rows[0].clone())
                } else {
                    None
                };
                Ok(json!({"matches":rows,"single":single,"count":ids.len()}))
            }
            "browser.element.focus" => {
                let node = node.ok_or_else(|| Error::invalid("DOM reference required"))?;
                cdp.call("DOM.focus", json!({"backendNodeId":node}), Some(&session))
                    .await?;
                let doc = cdp
                    .call("DOM.getDocument", json!({"depth":0}), Some(&session))
                    .await?;
                let focused = cdp
                    .call(
                        "DOM.querySelector",
                        json!({"nodeId":doc["root"]["nodeId"],"selector":":focus"}),
                        Some(&session),
                    )
                    .await?;
                let focused = cdp
                    .call(
                        "DOM.describeNode",
                        json!({"nodeId":focused["nodeId"],"depth":0}),
                        Some(&session),
                    )
                    .await?;
                if focused["node"]["backendNodeId"].as_u64() != Some(node) {
                    return Err(Error::new(ErrorCode::Conflict, "DOM focus changed"));
                }
                Ok(json!({"accepted":true,"focused":true}))
            }
            "browser.element.scroll" => {
                let node = node.ok_or_else(|| Error::invalid("DOM reference required"))?;
                cdp.call(
                    "DOM.scrollIntoViewIfNeeded",
                    json!({"backendNodeId":node}),
                    Some(&session),
                )
                .await?;
                self.validate_in(instance, &target).await?;
                Ok(json!({"accepted":true,"method":"DOM.scrollIntoViewIfNeeded"}))
            }
            "browser.element.upload" => {
                let node = node.ok_or_else(|| Error::invalid("DOM reference required"))?;
                let description = cdp
                    .call(
                        "DOM.describeNode",
                        json!({"backendNodeId":node,"depth":0}),
                        Some(&session),
                    )
                    .await?;
                let info = &description["node"];
                if info["nodeName"].as_str() != Some("INPUT")
                    || attribute(info, "type")
                        .is_none_or(|value| !value.eq_ignore_ascii_case("file"))
                {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "Upload requires an exact INPUT type=file semantic target",
                    ));
                }
                let paths = args["paths"]
                    .as_array()
                    .ok_or_else(|| Error::invalid("Upload paths array is required"))?;
                if paths.len() > 1 && attribute(info, "multiple").is_none() {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "Multiple files require an INPUT with the multiple attribute",
                    ));
                }
                let root_name = arg_str(args, "root")?;
                let (staging, staged, total_bytes) =
                    self.stage_uploads(instance, root_name, paths)?;
                let files: Result<Vec<String>> = staged
                    .iter()
                    .map(|path| {
                        path.to_str()
                            .map(str::to_owned)
                            .ok_or_else(|| Error::invalid("Staged upload path is not UTF-8"))
                    })
                    .collect();
                let files = match files {
                    Ok(files) => files,
                    Err(error) => {
                        let _ = std::fs::remove_dir_all(&staging);
                        return Err(error);
                    }
                };
                let result = async {
                    ctx.check_cancelled()?;
                    cdp.invalidate_session(&session)?;
                    cdp.call(
                        "DOM.setFileInputFiles",
                        json!({"files":files,"backendNodeId":node}),
                        Some(&session),
                    )
                    .await?;
                    Ok::<(), Error>(())
                }
                .await;
                if let Err(error) = result {
                    let _ = std::fs::remove_dir_all(&staging);
                    return Err(error);
                }
                instance.staged_upload_bytes =
                    instance.staged_upload_bytes.saturating_add(total_bytes);
                instance.staged_upload_files = instance
                    .staged_upload_files
                    .saturating_add(staged.len() as u32);
                Ok(json!({
                    "accepted":true,
                    "files":staged.len(),
                    "bytes":total_bytes,
                    "source_root":root_name,
                    "host_source_paths_exposed_to_browser":false,
                    "staging_lifetime":"browser_instance"
                }))
            }
            "browser.element.drag_to" => {
                let source = node.ok_or_else(|| Error::invalid("Source DOM reference required"))?;
                let destination_target: NativeTarget = serde_json::from_value(
                    args.get("_target2")
                        .cloned()
                        .ok_or_else(|| Error::invalid("Broker-resolved target_ref required"))?,
                )?;
                let (destination_tab, destination_node, destination_frame, destination_session) =
                    self.validate_in(instance, &destination_target).await?;
                let destination = destination_node
                    .ok_or_else(|| Error::invalid("Destination DOM reference required"))?;
                if destination_tab != tab
                    || destination_frame != frame
                    || destination_session != session
                {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "Drag source and destination must be in the same tab and frame context",
                    ));
                }
                cdp.call(
                    "DOM.scrollIntoViewIfNeeded",
                    json!({"backendNodeId":source}),
                    Some(&session),
                )
                .await?;
                cdp.call(
                    "DOM.scrollIntoViewIfNeeded",
                    json!({"backendNodeId":destination}),
                    Some(&session),
                )
                .await?;
                self.validate_in(instance, &target).await?;
                self.validate_in(instance, &destination_target).await?;
                let (source_x, source_y) = actionable_point(&cdp, &session, source).await?;
                let (destination_x, destination_y) =
                    actionable_point(&cdp, &session, destination).await?;
                ctx.check_cancelled()?;
                cdp.invalidate_session(&session)?;
                cdp.call(
                    "Input.dispatchMouseEvent",
                    json!({"type":"mouseMoved","x":source_x,"y":source_y,"button":"none","buttons":0}),
                    Some(&session),
                )
                .await?;
                cdp.call(
                    "Input.dispatchMouseEvent",
                    json!({"type":"mousePressed","x":source_x,"y":source_y,"button":"left","buttons":1,"clickCount":1}),
                    Some(&session),
                )
                .await?;
                for step in 1..=8 {
                    let ratio = f64::from(step) / 8.0;
                    let x = source_x + (destination_x - source_x) * ratio;
                    let y = source_y + (destination_y - source_y) * ratio;
                    cdp.call(
                        "Input.dispatchMouseEvent",
                        json!({"type":"mouseMoved","x":x,"y":y,"button":"left","buttons":1}),
                        Some(&session),
                    )
                    .await?;
                }
                cdp.call(
                    "Input.dispatchMouseEvent",
                    json!({"type":"mouseReleased","x":destination_x,"y":destination_y,"button":"left","buttons":0,"clickCount":1}),
                    Some(&session),
                )
                .await?;
                Ok(json!({
                    "accepted":true,
                    "method":"two semantic DOM refs + CDP Input drag",
                    "same_frame":true,
                    "coordinate_space":"viewport_css_pixels"
                }))
            }
            "browser.element.hover" => {
                let node = node.ok_or_else(|| Error::invalid("DOM reference required"))?;
                cdp.call(
                    "DOM.scrollIntoViewIfNeeded",
                    json!({"backendNodeId":node}),
                    Some(&session),
                )
                .await?;
                self.validate_in(instance, &target).await?;
                let (x, y) = actionable_point(&cdp, &session, node).await?;
                ctx.check_cancelled()?;
                cdp.invalidate_session(&session)?;
                cdp.call(
                    "Input.dispatchMouseEvent",
                    json!({"type":"mouseMoved","x":x,"y":y,"button":"none"}),
                    Some(&session),
                )
                .await?;
                Ok(json!({
                    "accepted":true,
                    "method":"DOM hit-test + CDP Input hover",
                    "coordinate_space":"viewport_css_pixels"
                }))
            }
            "browser.element.press" => {
                let node = node.ok_or_else(|| Error::invalid("DOM reference required"))?;
                let key = arg_str(args, "key")?;
                let modifiers = modifier_mask(args)?;
                let reported_modifiers = args["modifiers"].as_array().cloned().unwrap_or_default();
                focus_backend_node(&cdp, &session, node).await?;
                ctx.check_cancelled()?;
                cdp.invalidate_session(&session)?;
                dispatch_key(&cdp, &session, key, modifiers).await?;
                Ok(json!({"accepted":true,"key":key,"modifiers":reported_modifiers}))
            }
            "browser.element.check" => {
                let node = node.ok_or_else(|| Error::invalid("DOM reference required"))?;
                let desired = args["checked"].as_bool().unwrap_or(true);
                let before = ax_node_for_backend(&cdp, &session, node).await?;
                let role = ax_text(&before, "role").unwrap_or_default();
                if !matches!(role.as_str(), "checkbox" | "radio" | "switch") {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "Check requires a checkbox, radio or switch semantic role",
                    ));
                }
                if role == "radio" && !desired {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "Radio buttons cannot be unchecked directly; choose another radio option",
                    ));
                }
                let current = checked_state(&before).ok_or_else(|| {
                    Error::new(
                        ErrorCode::BackendFailed,
                        "Semantic checked state is unavailable",
                    )
                })?;
                if current == desired {
                    return Ok(json!({"changed":false,"checked":current}));
                }
                cdp.call(
                    "DOM.scrollIntoViewIfNeeded",
                    json!({"backendNodeId":node}),
                    Some(&session),
                )
                .await?;
                self.validate_in(instance, &target).await?;
                let (x, y) = actionable_point(&cdp, &session, node).await?;
                ctx.check_cancelled()?;
                cdp.invalidate_session(&session)?;
                for kind in ["mousePressed", "mouseReleased"] {
                    cdp.call(
                        "Input.dispatchMouseEvent",
                        json!({"type":kind,"x":x,"y":y,"button":"left","clickCount":1}),
                        Some(&session),
                    )
                    .await?;
                }
                let after = ax_node_for_backend(&cdp, &session, node).await?;
                let checked = checked_state(&after).ok_or_else(|| {
                    Error::new(
                        ErrorCode::BackendFailed,
                        "Checked state disappeared after action",
                    )
                })?;
                if checked != desired {
                    return Err(Error::new(
                        ErrorCode::BackendFailed,
                        "Semantic checked state did not reach the requested value",
                    ));
                }
                Ok(json!({"changed":true,"checked":checked}))
            }
            "browser.element.select" => {
                let node = node.ok_or_else(|| Error::invalid("DOM reference required"))?;
                let label = arg_str(args, "label")?;
                let description = cdp
                    .call(
                        "DOM.describeNode",
                        json!({"backendNodeId":node,"depth":4,"pierce":false}),
                        Some(&session),
                    )
                    .await?;
                let select = &description["node"];
                if select["nodeName"].as_str() != Some("SELECT")
                    || attribute(select, "multiple").is_some()
                {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "Select currently supports one native single-select HTML SELECT element",
                    ));
                }
                let options = select_options(select)?;
                let matches: Vec<_> = options
                    .iter()
                    .filter(|(_, candidate)| candidate == label)
                    .collect();
                if matches.len() != 1 {
                    return Err(if matches.is_empty() {
                        Error::new(
                            ErrorCode::NotFound,
                            "No enabled option has that exact label",
                        )
                    } else {
                        Error::new(
                            ErrorCode::Conflict,
                            "Multiple enabled options share that label",
                        )
                    });
                }
                let index = matches[0].0;
                focus_backend_node(&cdp, &session, node).await?;
                ctx.check_cancelled()?;
                cdp.invalidate_session(&session)?;
                dispatch_key(&cdp, &session, "Home", 0).await?;
                for _ in 0..index {
                    dispatch_key(&cdp, &session, "ArrowDown", 0).await?;
                }
                dispatch_key(&cdp, &session, "Enter", 0).await?;
                let after = ax_node_for_backend(&cdp, &session, node).await?;
                let value = ax_text(&after, "value").unwrap_or_default();
                if value != label {
                    return Err(Error::new(
                        ErrorCode::BackendFailed,
                        "Native select did not expose the requested semantic value after input",
                    ));
                }
                Ok(json!({"changed":true,"selected":value}))
            }
            "browser.dom.click" | "browser.element.click" => {
                let node = node.ok_or_else(|| Error::invalid("DOM reference required"))?;
                cdp.call(
                    "DOM.scrollIntoViewIfNeeded",
                    json!({"backendNodeId":node}),
                    Some(&session),
                )
                .await?;
                self.validate_in(instance, &target).await?;
                let (x, y) = actionable_point(&cdp, &session, node).await?;
                ctx.check_cancelled()?;
                // A click may synchronously mutate or navigate before CDP emits its
                // corresponding event. Conservatively retire all current DOM refs first.
                cdp.invalidate_session(&session)?;
                for kind in ["mousePressed", "mouseReleased"] {
                    cdp.call(
                        "Input.dispatchMouseEvent",
                        json!({"type":kind,"x":x,"y":y,"button":"left","clickCount":1}),
                        Some(&session),
                    )
                    .await?;
                }
                Ok(
                    json!({"accepted":true,"method":"DOM hit-test + CDP Input","coordinate_space":"viewport_css_pixels"}),
                )
            }
            "browser.dom.fill" | "browser.element.fill" => {
                let node = node.ok_or_else(|| Error::invalid("DOM reference required"))?;
                let description = cdp
                    .call(
                        "DOM.describeNode",
                        json!({"backendNodeId":node,"depth":0}),
                        Some(&session),
                    )
                    .await?;
                let info = &description["node"];
                let kind = info["nodeName"].as_str().unwrap_or("");
                let input_type = attribute(info, "type")
                    .unwrap_or_else(|| "text".into())
                    .to_ascii_lowercase();
                let contenteditable = attribute(info, "contenteditable")
                    .is_some_and(|value| matches!(value.as_str(), "" | "true" | "plaintext-only"));
                if (!matches!(kind, "INPUT" | "TEXTAREA") && !contenteditable)
                    || (kind == "INPUT"
                        && matches!(
                            input_type.as_str(),
                            "file"
                                | "hidden"
                                | "button"
                                | "submit"
                                | "checkbox"
                                | "radio"
                                | "range"
                                | "color"
                        ))
                {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "Fill requires a text-like input, textarea or contenteditable element",
                    ));
                }
                cdp.call("DOM.focus", json!({"backendNodeId":node}), Some(&session))
                    .await?;
                let doc = cdp
                    .call("DOM.getDocument", json!({"depth":0}), Some(&session))
                    .await?;
                let focused = cdp
                    .call(
                        "DOM.querySelector",
                        json!({"nodeId":doc["root"]["nodeId"],"selector":":focus"}),
                        Some(&session),
                    )
                    .await?;
                let focused = cdp
                    .call(
                        "DOM.describeNode",
                        json!({"nodeId":focused["nodeId"],"depth":0}),
                        Some(&session),
                    )
                    .await?;
                if focused["node"]["backendNodeId"].as_u64() != Some(node) {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "DOM focus changed before fill",
                    ));
                }
                ctx.check_cancelled()?;
                // Input changes application-visible DOM state. Retire the discovery
                // generation before dispatch instead of depending on event scheduling.
                cdp.invalidate_session(&session)?;
                for kind in ["keyDown", "keyUp"] {
                    cdp.call("Input.dispatchKeyEvent",json!({"type":kind,"modifiers":2,"key":"a","code":"KeyA","windowsVirtualKeyCode":65,"nativeVirtualKeyCode":65}),Some(&session)).await?;
                }
                cdp.call(
                    "Input.insertText",
                    json!({"text":arg_str(args,"text")?}),
                    Some(&session),
                )
                .await?;
                Ok(json!({"accepted":true,"replaced_selection":true}))
            }
            "browser.screenshot" => {
                let result = cdp
                    .call(
                        "Page.captureScreenshot",
                        json!({"format":"png","captureBeyondViewport":false,"fromSurface":true}),
                        Some(&session),
                    )
                    .await?;
                self.artifact(arg_str(&result, "data")?).await
            }
            _ => Err(Error::new(
                ErrorCode::Unsupported,
                "Unknown typed browser operation",
            )),
        }
    }
    async fn validate(&self, target: &NativeTarget) -> Result<()> {
        let mut state = self.instance.lock().await;
        let dead = state.as_mut().is_some_and(|instance| {
            instance
                .child
                .try_wait()
                .is_ok_and(|status| status.is_some())
                || !instance.cdp.alive.load(Ordering::SeqCst)
        });
        if dead {
            let stale = state.take().expect("dead browser instance exists");
            self.cleanup_instance(stale, false).await?;
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Browser instance exited; obtain references from a relaunched instance",
            ));
        }
        let instance = state
            .as_mut()
            .ok_or_else(|| Error::new(ErrorCode::StaleReference, "Browser is not running"))?;
        self.validate_in(instance, target).await.map(|_| ())
    }
    async fn shutdown(&self) -> Result<()> {
        if let Some(instance) = self.instance.lock().await.take() {
            self.cleanup_instance(instance, true).await?;
        } else {
            self.cleanup_artifacts();
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    #[test]
    fn browser_startup_timeout_is_bounded_but_ci_tolerant() {
        assert!(BROWSER_STARTUP_TIMEOUT >= std::time::Duration::from_secs(20));
        assert!(BROWSER_STARTUP_TIMEOUT <= std::time::Duration::from_secs(60));
    }

    #[test]
    fn default_denies_navigation() {
        let c = BrowserConfig::default();
        assert!(c.check_url("https://example.org").is_err());
        assert!(c.check_url("about:blank").is_ok());
    }
    #[test]
    fn exact_origin_not_prefix() {
        let c = BrowserConfig {
            allowed_origins: vec!["https://example.org".into()],
            ..Default::default()
        };
        assert!(c.validate().is_ok());
        assert!(c.check_url("https://example.org/path").is_ok());
        assert!(c.check_url("https://example.org.evil.test/").is_err());
        assert!(c.check_url("https://secret@example.org").is_err());
    }
    #[test]
    fn file_urls_and_script_urls_denied() {
        let c = BrowserConfig::default();
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "chrome://settings",
            "data:text/html,hello",
        ] {
            assert!(c.check_url(url).is_err());
        }
    }
    #[test]
    fn origin_configuration_cannot_be_path_or_wildcard() {
        for origin in [
            "*",
            "https://example.org/a",
            "https://example.org/",
            "https://user@example.org",
        ] {
            let c = BrowserConfig {
                allowed_origins: vec![origin.into()],
                ..Default::default()
            };
            assert!(c.validate().is_err());
        }
    }
    #[test]
    fn transient_target_metadata_is_retryable_without_relaxing_url_policy() {
        let c = BrowserConfig {
            allowed_origins: vec!["http://127.0.0.1:1234".into()],
            ..Default::default()
        };
        assert!(!target_url_ready(&c, "").unwrap());
        assert!(target_url_ready(&c, "about:blank").unwrap());
        assert!(target_url_ready(&c, "http://127.0.0.1:1234/path").unwrap());
        assert_eq!(
            target_url_ready(&c, "https://example.org/")
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
        assert_eq!(
            c.check_url("").unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
    #[test]
    fn sensitive_attributes_are_not_selected_by_helper() {
        let n = json!({"attributes":["id","a","value","secret"]});
        assert_eq!(attribute(&n, "id").as_deref(), Some("a"));
        assert_eq!(attribute(&n, "role"), None);
    }
    #[test]
    fn frame_tree_lookup_is_recursive_and_exact() {
        let tree = json!({
            "frame":{"id":"root"},
            "childFrames":[{
                "frame":{"id":"child"},
                "childFrames":[{"frame":{"id":"grandchild"}}]
            }]
        });
        assert!(frame_tree_contains(&tree, "root"));
        assert!(frame_tree_contains(&tree, "child"));
        assert!(frame_tree_contains(&tree, "grandchild"));
        assert!(!frame_tree_contains(&tree, "missing"));
    }

    #[test]
    fn document_identity_events_do_not_confuse_subframe_mutation_with_parent_navigation() {
        assert!(cdp_event_changes_document_identity(
            "DOM.documentUpdated",
            &json!({})
        ));
        assert!(cdp_event_changes_document_identity(
            "Page.frameNavigated",
            &json!({"params":{"frame":{"id":"root"}}})
        ));
        assert!(!cdp_event_changes_document_identity(
            "Page.frameNavigated",
            &json!({"params":{"frame":{"id":"child","parentId":"root"}}})
        ));
        for method in [
            "DOM.childNodeInserted",
            "DOM.childNodeRemoved",
            "DOM.attributeModified",
            "DOM.characterDataModified",
        ] {
            assert!(!cdp_event_changes_document_identity(method, &json!({})));
        }
    }

    #[test]
    fn frame_revision_tracks_loader_not_attach_session() {
        let first = json!({"id":"frame","loaderId":"loader-a","url":"https://example.test/a"});
        let same = json!({"id":"frame","loaderId":"loader-a","url":"https://example.test/b#same-document"});
        let next = json!({"id":"frame","loaderId":"loader-b","url":"https://example.test/b"});
        assert_eq!(
            frame_document_revision(&first),
            frame_document_revision(&same)
        );
        assert_ne!(
            frame_document_revision(&first),
            frame_document_revision(&next)
        );
    }

    #[test]
    fn semantic_query_filters_role_name_and_action() {
        let row = json!({
            "role":"button",
            "name":"Save changes",
            "actions":["focus","scroll","click"]
        });
        assert!(semantic_query_matches(&row, &json!({"role":"BUTTON"})));
        assert!(semantic_query_matches(&row, &json!({"name":"save"})));
        assert!(semantic_query_matches(
            &row,
            &json!({"name":"Save changes","exact_name":true})
        ));
        assert!(semantic_query_matches(&row, &json!({"action":"click"})));
        assert!(!semantic_query_matches(&row, &json!({"role":"textbox"})));
        assert!(!semantic_query_matches(
            &row,
            &json!({"name":"Save","exact_name":true})
        ));
        assert!(!semantic_query_matches(&row, &json!({"action":"fill"})));
    }

    #[test]
    fn accessibility_scalars_are_bounded_and_typed() {
        let node = json!({
            "role":{"value":"textbox"},
            "name":{"value":"n".repeat(700)},
            "properties":[
                {"name":"disabled","value":{"value":false}},
                {"name":"checked","value":{"value":"mixed"}},
                {"name":"ignored-object","value":{"value":{"nested":true}}}
            ]
        });
        assert_eq!(ax_text(&node, "role").as_deref(), Some("textbox"));
        assert_eq!(ax_text(&node, "name").unwrap().chars().count(), 512);
        assert_eq!(ax_property(&node, "disabled"), Some(json!(false)));
        assert_eq!(ax_property(&node, "checked"), Some(json!("mixed")));
        assert_eq!(ax_property(&node, "ignored-object"), None);
    }

    #[test]
    fn download_quota_configuration_is_bounded() {
        let mut config = BrowserConfig::default();
        assert!(config.validate().is_ok());
        config.max_download_bytes = 0;
        assert!(config.validate().is_err());
        config = BrowserConfig::default();
        config.max_download_bytes = config.max_total_download_bytes + 1;
        assert!(config.validate().is_err());
        config = BrowserConfig::default();
        config.max_downloads = 65;
        assert!(config.validate().is_err());
    }
    #[test]
    fn download_accounting_enforces_file_total_and_count_budgets() {
        let limits = DownloadLimits {
            per_file: 100,
            total: 150,
            count: 2,
        };
        let mut state = DownloadState::default();
        assert!(!state.begin("one", limits));
        assert!(!state.progress("one", 90, Some(90), "inProgress", limits));
        assert!(!state.begin("two", limits));
        assert!(state.progress("two", 70, Some(70), "inProgress", limits));
        assert_eq!(state.quota_cancellations, 1);
        assert!(state.begin("three", limits));
        assert_eq!(state.quota_cancellations, 2);

        let mut state = DownloadState::default();
        assert!(!state.begin("large", limits));
        assert!(state.progress("large", 1, Some(101), "inProgress", limits));
        assert_eq!(state.quota_cancellations, 1);
    }
    #[test]
    fn download_event_fields_are_strictly_bounded() {
        assert_eq!(
            safe_download_guid(&json!("abc-123_DEF")).as_deref(),
            Some("abc-123_DEF")
        );
        assert!(safe_download_guid(&json!("../escape")).is_none());
        assert!(safe_download_guid(&json!("a".repeat(129))).is_none());
        assert_eq!(bounded_byte_count(&json!(42)), Some(42));
        assert_eq!(bounded_byte_count(&json!(-1)), None);
        assert_eq!(bounded_byte_count(&json!("42")), None);
    }

    proptest! {
        #[test]
        fn frame_scoped_dom_target_parses_without_losing_identity(
            tab in "[A-Za-z0-9_-]{1,32}",
            frame in "[A-Za-z0-9_-]{1,64}",
            node in 1u64..u32::MAX as u64,
        ) {
            let target = NativeTarget {
                kind: "dom".into(),
                identity: format!("{tab}#frame:{frame}#node:{node}"),
                revision: 7,
                fingerprint: "epoch".into(),
                app: "org.semwright.Chromium".into(),
            };
            let (parsed_tab, parsed_node, parsed_frame) = Chromium::target_parts(&target).unwrap();
            prop_assert_eq!(parsed_tab, tab);
            prop_assert_eq!(parsed_node, Some(node));
            prop_assert_eq!(parsed_frame.as_deref(), Some(frame.as_str()));
        }

        #[test]
        fn frame_target_identity_is_exact_and_bounded(
            tab in "[A-Za-z0-9_-]{1,32}",
            frame in "[A-Za-z0-9_-]{1,128}",
        ) {
            let target = NativeTarget {
                kind: "frame".into(),
                identity: format!("{tab}#frame:{frame}"),
                revision: 3,
                fingerprint: "epoch".into(),
                app: "org.semwright.Chromium".into(),
            };
            let (parsed_tab, node, parsed_frame) = Chromium::target_parts(&target).unwrap();
            prop_assert_eq!(parsed_tab, tab);
            prop_assert_eq!(node, None);
            prop_assert_eq!(parsed_frame.as_deref(), Some(frame.as_str()));
        }
    }
}
