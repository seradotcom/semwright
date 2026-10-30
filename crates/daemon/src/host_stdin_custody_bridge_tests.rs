use super::*;
use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_policy::FilesystemGrant;
use semwright_types::CommandDescriptor;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use tokio::sync::{Notify, mpsc};
const INPUT: &[u8] = b"semwright-custody-v7\0payload\n";
const INPUT_SHA: &str = "a5a25ed66c58e252cb147c18c354d30318c0076855c39600e9a2ba8ee3e75359";
const SESSION: &str = "daemon-custody-bridge-session";
fn owner_fixture_config() -> semwright_daemon::config::Config {
    let path = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_STDIN_CUSTODY_CONFIG")
            .expect("explicit owner fixture config is required for selected Linux tests"),
    );
    let config = semwright_daemon::config::load(Some(&path)).expect("real owner config");
    assert_eq!(config.drivers.len(), 1);
    assert!(!config.driver_network && !config.driver_nvidia_gpu);
    assert_eq!(config.policy.filesystem.len(), 1);
    let grant = &config.policy.filesystem[0];
    assert_eq!(grant.name, "fixture-tool-root");
    assert!(grant.read && !grant.write && grant.path.is_file());
    let manifest = semwright_daemon::config::driver_manifest(&config.drivers[0])
        .expect("real owner manifest and validation");
    assert_eq!(manifest.id, "fixture");
    assert_eq!(manifest.publisher, "semwright-tests");
    assert_eq!(manifest.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest.protocol, 7);
    assert!(manifest.interfaces.host_tools && manifest.mounts.is_empty());
    assert!(!manifest.network && !manifest.nvidia_gpu);
    assert_eq!(manifest.tools.len(), 1);
    assert_eq!(manifest.tools[0].name, "probe");
    assert_eq!(manifest.tools[0].root, "fixture-tool-root");
    assert!(manifest.tools[0].mounts.is_empty());
    config
}
fn private_tree() -> tempfile::TempDir {
    let root = tempfile::Builder::new()
        .prefix("semwright-custody-bridge-")
        .tempdir()
        .unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    root
}
fn private_child(root: &Path, name: &str) -> PathBuf {
    let path = root.canonicalize().unwrap().join(name);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .unwrap();
    path
}
fn read_json(path: &Path) -> Value {
    let raw = std::fs::read(path).unwrap();
    assert!(raw.len() <= MAX_METADATA);
    serde_json::from_slice(&raw).unwrap()
}
async fn wait_for(path: &Path) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.is_file() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        read_json(path)
    })
    .await
    .expect("bounded custody publication")
}
struct RealHost {
    provider: Arc<DriverProvider>,
    descriptor: CommandDescriptor,
    identity: ProviderIdentity,
}
impl RealHost {
    async fn connect(
        manifest: Manifest,
        roots: &[FilesystemGrant],
        state: &Path,
        sink: HostStdinCustody,
    ) -> Self {
        let identity = manifest.identity().unwrap();
        let helper = PathBuf::from(
            std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
                .expect("existing real sandbox helper fixture"),
        );
        let provider = DriverProvider::connect_with_gpu_and_stdin_custody(
            manifest,
            state,
            &helper,
            roots,
            false,
            false,
            Some(sink),
        )
        .await
        .expect("actual sandboxed DriverProvider with opt-in custody");
        let descriptor = Provider::capabilities(provider.as_ref())
            .await
            .unwrap()
            .into_iter()
            .find(|item| item.descriptor.name == "driver.fixture.tool_job")
            .expect("actual fixture tool-job capability")
            .descriptor;
        Self {
            provider,
            descriptor,
            identity,
        }
    }
    async fn invoke(&self, request: &str, args: Value) -> Value {
        let context = Context {
            session: SESSION.into(),
            request_id: request.into(),
            cancellation: CancellationToken::new(),
        };
        Provider::execute(self.provider.as_ref(), &context, &self.descriptor, &args)
            .await
            .expect("actual Host fixture result; never retry an uncertain start")
    }
    async fn start(&self, parent: &str) -> String {
        let result = self
            .invoke(parent, json!({"action":"start_stdin_custody"}))
            .await;
        assert_eq!(result["state"], "running");
        result["job"]
            .as_str()
            .expect("actual Host-issued job id")
            .to_owned()
    }
    async fn succeed(&self, job: &str) {
        let result = tokio::time::timeout(Duration::from_secs(8), async {
            for attempt in 0..160usize {
                let result = self
                    .invoke(
                        &format!("custody-job-status-{attempt}"),
                        json!({"action":"status","job":job}),
                    )
                    .await;
                match result["state"].as_str() {
                    Some("succeeded") => return result,
                    Some("running") => tokio::time::sleep(Duration::from_millis(20)).await,
                    state => panic!("unexpected actual tool terminal: {state:?}"),
                }
            }
            panic!("bounded tool-job status was not terminal")
        })
        .await
        .expect("bounded original job completion");
        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["stdout"], "tool-ok");
    }
    async fn shutdown(&self) {
        Provider::shutdown(self.provider.as_ref())
            .await
            .expect("original provider shutdown");
    }
}
fn assert_metadata(metadata: &Value, host: &RealHost, job: &str, parent: &str) {
    assert_eq!(
        metadata["provider"],
        serde_json::to_value(&host.identity).unwrap()
    );
    assert!(metadata["provider_process_id"].as_u64().unwrap() > 0);
    assert_eq!(metadata["job_id"], job);
    assert_eq!(metadata["parent_request_id"], parent);
    assert_eq!(metadata["session_id"], SESSION);
    assert_eq!(metadata["tool"], "probe");
    assert_eq!(metadata["stdin_bytes"], INPUT.len());
    assert_eq!(metadata["stdin_sha256"], INPUT_SHA);
    assert_eq!(metadata["terminal_file_required"], true);
    let request = metadata["request_id"].as_str().unwrap();
    assert!(!request.is_empty() && request != parent);
}
async fn genuine_capture(
    parent: &str,
) -> (
    tempfile::TempDir,
    RealHost,
    HostStdinCustody,
    HostStdinCapture,
    Arc<PrivateDirectory>,
) {
    let config = owner_fixture_config();
    let root = private_tree();
    let state = private_child(root.path(), "state");
    let directory =
        Arc::new(PrivateDirectory::open(&private_child(root.path(), "custody")).unwrap());
    let manifest = semwright_daemon::config::driver_manifest(&config.drivers[0]).unwrap();
    let (sink, mut receiver) = HostStdinCustody::channel_for_tool("probe", QUEUE).unwrap();
    let host = RealHost::connect(manifest, &config.policy.filesystem, &state, sink.clone()).await;
    let job = host.start(parent).await;
    let capture = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .expect("bounded genuine Host capture")
        .expect("original Host capture");
    assert_eq!(capture.original_stdin(), INPUT);
    let metadata: Value =
        serde_json::from_slice(&capture_metadata(&capture, &host.identity, "probe", None).unwrap())
            .unwrap();
    assert_metadata(&metadata, &host, &job, parent);
    host.succeed(&job).await;
    receiver.close();
    drop(receiver);
    (root, host, sink, capture, directory)
}
impl CaptureQueue for mpsc::Receiver<HostStdinCapture> {
    fn recv(&mut self) -> impl std::future::Future<Output = Option<HostStdinCapture>> + Send + '_ {
        mpsc::Receiver::recv(self)
    }
    fn close(&mut self) {
        mpsc::Receiver::close(self);
    }
}

#[tokio::test]
#[ignore = "requires selected real Linux Bubblewrap/Landlock and owner fixture config"]
async fn linux_real_host_nonempty_capture_drains_to_bound_original_and_terminal() {
    let config = owner_fixture_config();
    let root = private_tree();
    let directory = private_child(root.path(), "custody");
    let state = private_child(root.path(), "state");
    let options = Options {
        host_stdin_custody_dir: Some(directory.clone()),
        host_stdin_custody_provider: Some("driver:fixture".into()),
        host_stdin_custody_tool: Some("probe".into()),
    };
    let mut bootstrap = Bootstrap::prepare(&options, false, &config.drivers)
        .unwrap()
        .expect("explicit custody bootstrap");
    let (manifest, _) = bootstrap
        .retained_manifest(&config.drivers[0])
        .unwrap()
        .into_parts();
    let driver_sha = manifest.sha256.clone();
    let sink = bootstrap.sink_for(&manifest).unwrap();
    let host = RealHost::connect(manifest, &config.policy.filesystem, &state, sink).await;
    let parent = "daemon-custody-positive-start";
    let job = host.start(parent).await;
    host.succeed(&job).await;
    let terminal = wait_for(&directory.join("capture-0001.terminal.json")).await;
    let metadata = read_json(&directory.join("capture-0001.json"));
    assert_metadata(&metadata, &host, &job, parent);
    assert_eq!(
        std::fs::read(directory.join("capture-0001.stdin")).unwrap(),
        INPUT
    );
    assert_eq!(terminal["write_state"], "write_all_completed");
    assert_eq!(terminal["partial_delivery_possible"], false);
    assert_eq!(terminal["child_consumption"], "not_observed");
    assert_eq!(terminal["native_render_success"], "not_inferred");
    let session = read_json(&directory.join("session.json"));
    assert_eq!(
        session["provider"],
        serde_json::to_value(&host.identity).unwrap()
    );
    assert_eq!(session["driver_sha256"], driver_sha);
    bootstrap.finish().await;
    let closed = wait_for(&directory.join("closed.json")).await;
    assert_eq!(closed["status"], "observer_closed");
    assert_eq!(closed["capture_records_written"], 1);
    assert_eq!(closed["terminal_records_written"], 1);
    assert_eq!(closed["input_bytes_reserved"], INPUT.len());
    assert_eq!(closed["queued_captures_discarded"], 0);
    assert_eq!(closed["queue_disconnection_proven"], true);
    assert!(
        closed["loss_counters_at_observer_close"]
            .as_object()
            .unwrap()
            .values()
            .all(|count| count.as_u64() == Some(0))
    );
    host.shutdown().await;
}
#[tokio::test]
#[ignore = "requires selected real Linux Bubblewrap/Landlock and owner fixture config"]
async fn linux_real_host_metadata_collision_preserves_partial_bytes_and_native_job() {
    let config = owner_fixture_config();
    let root = private_tree();
    let directory = private_child(root.path(), "custody");
    let state = private_child(root.path(), "state");
    let options = Options {
        host_stdin_custody_dir: Some(directory.clone()),
        host_stdin_custody_provider: Some("driver:fixture".into()),
        host_stdin_custody_tool: Some("probe".into()),
    };
    let mut bootstrap = Bootstrap::prepare(&options, false, &config.drivers)
        .unwrap()
        .unwrap();
    let collision = directory.join("capture-0001.json");
    std::fs::write(&collision, b"existing-private-sentinel").unwrap();
    let (manifest, _) = bootstrap
        .retained_manifest(&config.drivers[0])
        .unwrap()
        .into_parts();
    let sink = bootstrap.sink_for(&manifest).unwrap();
    let host = RealHost::connect(manifest, &config.policy.filesystem, &state, sink).await;
    let job = host.start("daemon-custody-publication-failure-start").await;
    let closed = wait_for(&directory.join("closed.json")).await;
    assert_eq!(closed["status"], "incomplete");
    assert_eq!(closed["reason"], "capture_publication_failed");
    assert_eq!(closed["capture_records_written"], 0);
    assert_eq!(closed["terminal_records_written"], 0);
    assert_eq!(closed["input_bytes_reserved"], INPUT.len());
    assert_eq!(
        std::fs::read(directory.join("capture-0001.stdin")).unwrap(),
        INPUT
    );
    assert_eq!(
        std::fs::read(&collision).unwrap(),
        b"existing-private-sentinel"
    );
    assert!(!directory.join("capture-0001.terminal.json").exists());
    host.succeed(&job).await;
    bootstrap.finish().await;
    host.shutdown().await;
}
#[tokio::test]
#[ignore = "requires selected real Linux Bubblewrap/Landlock and owner fixture config"]
async fn linux_genuine_capture_stop_during_consumer_terminal_wait_stays_incomplete() {
    let (_root, host, sink, capture, directory) =
        genuine_capture("daemon-custody-pending-terminal-start").await;
    let path = directory.path.clone();
    let (sender, receiver) = mpsc::channel(1);
    sender
        .send(capture)
        .await
        .ok()
        .expect("move genuine capture once");
    drop(sender);
    let entered = Arc::new(Notify::new());
    let observed = entered.clone();
    let stop = CancellationToken::new();
    let cancel = stop.clone();
    let observer = drain_with(
        directory,
        host.identity.clone(),
        "probe".into(),
        receiver,
        sink,
        stop,
        move |capture: HostStdinCapture| {
            let entered = entered.clone();
            async move {
                // Pause the consumer await only; no claim of a pending kernel write.
                entered.notify_one();
                std::future::pending::<()>().await;
                capture.terminal().await
            }
        },
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(observer, async {
            observed.notified().await;
            cancel.cancel();
        });
    })
    .await
    .expect("bounded observer cancellation");
    assert_eq!(
        std::fs::read(path.join("capture-0001.stdin")).unwrap(),
        INPUT
    );
    let terminal = read_json(&path.join("capture-0001.terminal.json"));
    assert_eq!(
        terminal["write_state"],
        "observer_stopped_terminal_unavailable"
    );
    assert_eq!(terminal["child_consumption"], "not_observed");
    assert_eq!(terminal["native_render_success"], "not_inferred");
    let closed = read_json(&path.join("closed.json"));
    assert_eq!(closed["status"], "incomplete");
    assert_eq!(closed["capture_records_written"], 1);
    assert_eq!(closed["terminal_records_written"], 1);
    assert_eq!(closed["queue_disconnection_proven"], true);
    host.shutdown().await;
}
#[tokio::test]
#[ignore = "requires selected real Linux Bubblewrap/Landlock and owner fixture config"]
async fn linux_genuine_capture_late_reserved_permit_is_counted_not_false_eof() {
    let (_root, host, sink, capture, directory) =
        genuine_capture("daemon-custody-late-permit-start").await;
    let path = directory.path.clone();
    let (sender, receiver) = mpsc::channel(1);
    let permit = sender.clone().reserve_owned().await.unwrap();
    let stop = CancellationToken::new();
    stop.cancel();
    let observer = drain_with(
        directory,
        host.identity.clone(),
        "probe".into(),
        receiver,
        sink,
        stop,
        |capture: HostStdinCapture| capture.terminal(),
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(observer, async move {
            sender.closed().await;
            drop(permit.send(capture));
            drop(sender);
        });
    })
    .await
    .expect("bounded reserved-permit drain");
    let closed = read_json(&path.join("closed.json"));
    assert_eq!(closed["status"], "incomplete");
    assert_eq!(closed["queued_captures_discarded"], 1);
    assert_eq!(closed["queue_disconnection_proven"], true);
    assert_eq!(closed["capture_records_written"], 0);
    assert_eq!(closed["terminal_records_written"], 0);
    assert_eq!(closed["input_bytes_reserved"], 0);
    assert!(!path.join("capture-0001.stdin").exists());
    assert!(!path.join("capture-0001.json").exists());
    host.shutdown().await;
}
