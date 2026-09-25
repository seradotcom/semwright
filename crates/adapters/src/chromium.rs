//! CDP against a broker-launched private browser only. No attach endpoint or JavaScript evaluator.
//! An origin allowlist constrains top-level commands, NOT all browser network traffic.
use async_trait::async_trait;
use base64::Engine;
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use semwright_backend_api::{Backend, Context};
use semwright_protocol::{current_uid, private_directory};
use semwright_types::*;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::PathBuf,
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
            events: Arc::new(StdMutex::new(VecDeque::new())),
            downloads: Arc::new(StdMutex::new(DownloadState::default())),
            alive: Arc::new(AtomicBool::new(true)),
            stop: CancellationToken::new(),
        });
        let pending = this.pending.clone();
        let writer = this.writer.clone();
        let sequence = this.sequence.clone();
        let generations = this.generations.clone();
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
                    if matches!(
                        method,
                        "DOM.documentUpdated"
                            | "DOM.childNodeInserted"
                            | "DOM.childNodeRemoved"
                            | "DOM.attributeModified"
                            | "DOM.attributeRemoved"
                            | "DOM.characterDataModified"
                            | "Page.frameNavigated"
                            | "Inspector.targetCrashed"
                    ) && let Some(session) = value.get("sessionId").and_then(Value::as_str)
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

                    // Metadata only: no console text, headers, URLs, request bodies or download names.
                    if matches!(
                        method,
                        "Browser.downloadWillBegin"
                            | "Browser.downloadProgress"
                            | "Log.entryAdded"
                            | "Network.loadingFailed"
                            | "Inspector.targetCrashed"
                    ) {
                        let mut row = json!({"event":method});
                        if let Some(guid) = download_guid {
                            row["guid"] = json!(guid);
                        }
                        if let Some(state) = download_state {
                            row["state"] = json!(state);
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
        for domain in ["DOM.enable", "Page.enable", "Log.enable", "Network.enable"] {
            self.cdp.call(domain, json!({}), Some(&session)).await?;
        }
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
        let id = node["backendNodeId"].as_u64().ok_or_else(|| {
            Error::new(
                ErrorCode::BackendFailed,
                "DOM node has no stable backend identity",
            )
        })?;
        Ok(target_marker(NativeTarget {
            kind: "dom".into(),
            identity: format!("{target}#{id}"),
            revision: self.cdp.generation(session)?,
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
    instance: Mutex<Option<Instance>>,
    artifacts: Arc<StdMutex<BTreeMap<PathBuf, u64>>>,
}
impl Chromium {
    pub fn new(config: BrowserConfig, directory: PathBuf) -> Result<Self> {
        config.validate()?;
        private_directory(&directory)?;
        Ok(Self {
            config,
            directory,
            instance: Mutex::new(None),
            artifacts: Arc::new(StdMutex::new(BTreeMap::new())),
        })
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
            closed_tabs: VecDeque::new(),
        });
        Ok(
            json!({"launched":true,"isolated_profile":true,"headless":headless,"browser_sandbox_disabled":false,"downloads_enabled":self.config.allow_downloads}),
        )
    }
    fn target_parts(target: &NativeTarget) -> Result<(String, Option<u64>)> {
        match target.kind.as_str() {
            "tab" => Ok((target.identity.clone(), None)),
            "dom" => {
                let (tab, id) = target.identity.split_once('#').ok_or_else(|| {
                    Error::new(ErrorCode::StaleReference, "Invalid DOM reference")
                })?;
                Ok((
                    tab.into(),
                    Some(id.parse().map_err(|_| {
                        Error::new(ErrorCode::StaleReference, "Invalid DOM identity")
                    })?),
                ))
            }
            _ => Err(Error::new(
                ErrorCode::InvalidArgument,
                "A browser tab or DOM reference is required",
            )),
        }
    }
    async fn validate_in(
        &self,
        instance: &mut Instance,
        target: &NativeTarget,
    ) -> Result<(String, Option<u64>, String)> {
        if target.fingerprint != instance.epoch {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Browser instance has changed",
            ));
        }
        let (tab, node) = Self::target_parts(target)?;
        // Navigation invalidates a DOM ref semantically before target metadata necessarily
        // settles on a parseable URL. Prefer the known generation mismatch over a transient
        // URL/configuration error so stale identities never degrade into InvalidArgument.
        if node.is_some()
            && let Some(session) = instance.sessions.get(&tab)
            && instance.cdp.generation(session)? != target.revision
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "DOM changed; obtain a fresh semantic reference",
            ));
        }
        instance.check_tab(&tab, &self.config).await?;
        let session = instance.session(&tab).await?;
        if let Some(node) = node {
            if instance.cdp.generation(&session)? != target.revision {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "DOM changed; obtain a fresh semantic reference",
                ));
            }
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
                        "DOM node is no longer addressable",
                    )
                })?;
            if instance.cdp.generation(&session)? != target.revision {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "DOM changed during validation",
                ));
            }
        }
        Ok((tab, node, session))
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
fn compact_dom(
    instance: &Instance,
    target: &str,
    session: &str,
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
    let mut row = json!({"node_name":name,"text":node["nodeValue"].as_str().unwrap_or("").chars().take(512).collect::<String>(),"ref":instance.dom_ref(target,node,session)?});
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
    if let Some(nodes) = node["children"].as_array() {
        for child in nodes {
            if let Some(v) = compact_dom(instance, target, session, child, remaining)? {
                children.push(v);
            }
            if *remaining == 0 {
                break;
            }
        }
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
        let (tab, node, session) = self.validate_in(instance, &target).await?;
        let cdp = instance.cdp.clone();
        match command {
            "browser.tab.close" => {
                if node.is_some() {
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
                cdp.call("Target.activateTarget", json!({"targetId":tab}), None)
                    .await?;
                Ok(json!({"accepted":true}))
            }
            "browser.navigate" => {
                if node.is_some() {
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
            "browser.dom.snapshot" => {
                if node.is_some() {
                    return Err(Error::invalid(
                        "Tab reference required for a document snapshot",
                    ));
                }
                let generation = cdp.generation(&session)?;
                let result = cdp
                    .call(
                        "DOM.getDocument",
                        json!({"depth":args["depth"].as_u64().unwrap_or(4),"pierce":false}),
                        Some(&session),
                    )
                    .await?;
                if cdp.generation(&session)? != generation {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "Document changed while observing; request a fresh snapshot",
                    ));
                }
                let mut budget = 500;
                let root = compact_dom(instance, &tab, &session, &result["root"], &mut budget)?;
                if cdp.generation(&session)? != generation {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "Document changed while creating references",
                    ));
                }
                Ok(
                    json!({"root":root,"node_budget":500,"truncated":budget==0,"sensitive_attributes_omitted":true,"semantic_coverage":"partial; shadow DOM, cross-origin frames and script/style bodies excluded"}),
                )
            }
            "browser.dom.query" => {
                if node.is_some() {
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
                    rows.push(json!({"ref":instance.dom_ref(&tab,&detail["node"],&session)?,"node_name":detail["node"]["nodeName"],"name":attribute(&detail["node"],"aria-label").or_else(||attribute(&detail["node"],"name"))}));
                }
                if cdp.generation(&session)? != revision {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "DOM changed during query; repeat discovery",
                    ));
                }
                let single = if rows.len() == 1 {
                    Some(rows[0].clone())
                } else {
                    None
                };
                Ok(json!({"matches":rows,"single":single,"count":ids.len()}))
            }
            "browser.dom.click" => {
                let node = node.ok_or_else(|| Error::invalid("DOM reference required"))?;
                cdp.call(
                    "DOM.scrollIntoViewIfNeeded",
                    json!({"backendNodeId":node}),
                    Some(&session),
                )
                .await?;
                self.validate_in(instance, &target).await?;
                let model = cdp
                    .call(
                        "DOM.getBoxModel",
                        json!({"backendNodeId":node}),
                        Some(&session),
                    )
                    .await?;
                let quad = model["model"]["content"]
                    .as_array()
                    .filter(|q| q.len() == 8)
                    .ok_or_else(|| {
                        Error::new(
                            ErrorCode::NotFound,
                            "DOM node has no actionable content quad",
                        )
                    })?;
                let coordinates: Vec<f64> = quad
                    .iter()
                    .map(|v| v.as_f64().unwrap_or(f64::NAN))
                    .collect();
                let x = (coordinates[0] + coordinates[2] + coordinates[4] + coordinates[6]) / 4.0;
                let y = (coordinates[1] + coordinates[3] + coordinates[5] + coordinates[7]) / 4.0;
                if !x.is_finite()
                    || !y.is_finite()
                    || x < 0.0
                    || y < 0.0
                    || x > 16384.0
                    || y > 16384.0
                {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "DOM click coordinate is outside the bounded viewport",
                    ));
                }
                let hit=cdp.call("DOM.getNodeForLocation",json!({"x":x.round()as i32,"y":y.round()as i32,"includeUserAgentShadowDOM":false,"ignorePointerEventsNone":false}),Some(&session)).await?;
                if hit["backendNodeId"].as_u64() != Some(node) {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Another DOM element covers the target; choose the precise hit element explicitly",
                    ));
                }
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
            "browser.dom.fill" => {
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
                if !matches!(kind, "INPUT" | "TEXTAREA")
                    || matches!(
                        input_type.as_str(),
                        "file"
                            | "hidden"
                            | "button"
                            | "submit"
                            | "checkbox"
                            | "radio"
                            | "range"
                            | "color"
                    )
                {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "Fill requires a text-like input or textarea, never a file picker",
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
}
