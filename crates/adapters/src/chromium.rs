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
}
impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            executable: PathBuf::from("/usr/bin/chromium"),
            allowed_origins: vec![],
            allow_downloads: false,
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
struct Cdp {
    writer: Arc<Mutex<SplitSink<Socket, Message>>>,
    pending: Pending,
    sequence: AtomicU64,
    generations: Arc<StdMutex<BTreeMap<String, u64>>>,
    events: Arc<StdMutex<VecDeque<Value>>>,
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
impl Cdp {
    async fn connect(endpoint: &str) -> Result<Arc<Self>> {
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
            sequence: AtomicU64::new(1),
            generations: Arc::new(StdMutex::new(BTreeMap::new())),
            events: Arc::new(StdMutex::new(VecDeque::new())),
            alive: Arc::new(AtomicBool::new(true)),
            stop: CancellationToken::new(),
        });
        let pending = this.pending.clone();
        let generations = this.generations.clone();
        let events = this.events.clone();
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
                    ) && let Some(session) = value.get("sessionId").and_then(Value::as_str)
                        && let Ok(mut map) = generations.lock()
                    {
                        let n = map.entry(session.into()).or_insert(0);
                        *n = n.saturating_add(1);
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
                        for key in ["guid", "state", "receivedBytes", "totalBytes"] {
                            if let Some(v) = value.pointer(&format!("/params/{key}")) {
                                row[key] = v.clone();
                            }
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
        config.check_url(row["url"].as_str().unwrap_or(""))
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
}
impl Chromium {
    pub fn new(config: BrowserConfig, directory: PathBuf) -> Result<Self> {
        config.validate()?;
        private_directory(&directory)?;
        Ok(Self {
            config,
            directory,
            instance: Mutex::new(None),
        })
    }
    async fn launch(&self, state: &mut Option<Instance>, headless: bool) -> Result<Value> {
        if state.is_some() {
            return Err(Error::new(
                ErrorCode::Conflict,
                "An isolated browser is already running",
            ));
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
            if started.elapsed() > std::time::Duration::from_secs(12) {
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
        let cdp = Cdp::connect(&endpoint).await?;
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
        let path = self.directory.join(format!("screen-{}.png", unique_id()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(&data)?;
        file.sync_all()?;
        let expiry = path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
            let _ = tokio::fs::remove_file(expiry).await;
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
            return Ok(
                json!({"running":state.as_ref().is_some_and(|i|i.cdp.alive.load(Ordering::SeqCst)),"isolated_profile":true,"attach_existing":false,"arbitrary_javascript":false,"origin_scope":"top-level command targets, not a network firewall","downloads_enabled":self.config.allow_downloads}),
            );
        }
        if command == "browser.launch" {
            return self
                .launch(&mut state, args["headless"].as_bool().unwrap_or(true))
                .await;
        }
        let instance = state.as_mut().ok_or_else(|| {
            Error::unavailable("No isolated browser is running; use browser.launch with approval")
        })?;
        if instance.child.try_wait()?.is_some() {
            return Err(Error::unavailable("The isolated browser exited"));
        }
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
            return Ok(
                json!({"events":events,"bounded_event_history":256,"download_directory":instance.downloads,"enabled":self.config.allow_downloads,"payloads_redacted":true,"automatic_download_size_limit":false}),
            );
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
        let instance = state
            .as_mut()
            .ok_or_else(|| Error::new(ErrorCode::StaleReference, "Browser is not running"))?;
        self.validate_in(instance, target).await.map(|_| ())
    }
    async fn shutdown(&self) -> Result<()> {
        if let Some(mut instance) = self.instance.lock().await.take() {
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                instance.cdp.call("Browser.close", json!({}), None),
            )
            .await;
            instance.cdp.stop.cancel();
            if tokio::time::timeout(std::time::Duration::from_secs(5), instance.child.wait())
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
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
    fn sensitive_attributes_are_not_selected_by_helper() {
        let n = json!({"attributes":["id","a","value","secret"]});
        assert_eq!(attribute(&n, "id").as_deref(), Some("a"));
        assert_eq!(attribute(&n, "role"), None);
    }
}
