//! Real Linux subprocess test. No fake Provider, capsule, Host or authority.
//! Fixed file effects only; this does not exercise Motionwright SQLite/Graph.
use super::{Broker, NoApprover, audit};
use crate::providers::ProviderLease;
use semwright_backend_api::Provider;
use semwright_driver_host::DriverProvider;
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
use semwright_types::{Envelope, Error, ErrorCode, ExecuteRequest, Result, unique_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
const PROVIDER: &str = "driver:owner-permit-fixture";
const OBSERVE: &str = "driver.owner-permit-fixture.observe";
const APPLY: &str = "driver.owner-permit-fixture.apply";
const DATA: &str = "motionwright-data";
const CONTROL: &str = "permit-fixture-control";
fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::new(ErrorCode::BackendFailed, message))
    }
}
fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
fn publish_control(control: &Path, marker: &str) -> Result<()> {
    let (staging_name, bytes): (&str, &[u8]) = match marker {
        "entered" => ("entered.pending", b"entered\n"),
        "close-seen" => ("close-seen.pending", b"closed\n"),
        "release" => ("release.pending", b"go\n"),
        _ => return Err(Error::invalid("Unknown fixture control marker")),
    };
    let staging = control.join(staging_name);
    let destination = control.join(marker);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&staging)?;
    // Preserve staging on any failure; do not hide partial custody or retry it.
    file.write_all(bytes)
        .map_err(|error| Error::from(error).uncertain())?;
    file.sync_all()
        .map_err(|error| Error::from(error).uncertain())?;
    drop(file);
    std::fs::hard_link(&staging, &destination).map_err(|error| Error::from(error).uncertain())?;
    // Only remove our staging alias after publication. Never roll back destination.
    std::fs::remove_file(&staging).map_err(|error| Error::from(error).uncertain())?;
    Ok(())
}
fn data(envelope: Envelope) -> Result<Value> {
    if !envelope.ok {
        return Err(envelope.error.unwrap_or_else(|| {
            Error::new(
                ErrorCode::BackendFailed,
                "Failed fixture envelope has no typed error",
            )
        }));
    }
    envelope
        .data
        .ok_or_else(|| Error::new(ErrorCode::BackendFailed, "Missing fixture data"))
}
fn request(command: &str, args: Value) -> ExecuteRequest {
    ExecuteRequest {
        command: command.into(),
        args,
        dry_run: false,
        backend: None,
    }
}
fn query() -> Value {
    json!({"resource":"fixture:owner-operation","scope":"fixture","limit":1})
}
struct CustodyRoot {
    directory: Option<tempfile::TempDir>,
    remove_on_drop: bool,
}
impl CustodyRoot {
    fn new() -> Result<Self> {
        Ok(Self {
            directory: Some(tempfile::tempdir()?),
            remove_on_drop: false,
        })
    }
    fn path(&self) -> &Path {
        self.directory.as_ref().expect("live fixture root").path()
    }
}
impl Drop for CustodyRoot {
    fn drop(&mut self) {
        if !self.remove_on_drop {
            if let Some(directory) = self.directory.take() {
                let retained = directory.keep();
                eprintln!("RETAINED_OWNER_PERMIT_FIXTURE_ROOT={}", retained.display());
            }
        }
    }
}
struct Fixture {
    broker: Arc<Broker>,
    provider: Arc<DriverProvider>,
    lease: Arc<ProviderLease>,
    session: String,
    data: PathBuf,
    control: PathBuf,
    request: Option<JoinHandle<Envelope>>,
    cancellation: CancellationToken,
    _root: CustodyRoot,
}
impl Fixture {
    async fn new() -> Result<Self> {
        require(
            Path::new("/usr/bin/bwrap").is_file(),
            "Real Bubblewrap is required",
        )?;
        let source = PathBuf::from(
            std::env::var_os("SEMWRIGHT_TEST_OWNER_PERMIT_DRIVER")
                .ok_or_else(|| Error::invalid("Set the built native permit fixture path"))?,
        )
        .canonicalize()?;
        let helper = PathBuf::from(
            std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
                .ok_or_else(|| Error::invalid("Set the built sandbox helper path"))?,
        )
        .canonicalize()?;
        require(
            source.is_file() && helper.is_file(),
            "Fixture binaries must be files",
        )?;
        let root = CustodyRoot::new()?;
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))?;
        let bin = root.path().join("bin");
        let state = root.path().join("host-state");
        let data_root = root.path().join("data");
        let control = root.path().join("control");
        for path in [&bin, &state, &data_root, &control] {
            private_directory(path)?;
        }
        let executable = bin.join("driver");
        fs::copy(source, &executable)?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o500))?;
        let executable = executable.canonicalize()?;
        let digest = hex::encode(Sha256::digest(fs::read(&executable)?));
        let data_root = data_root.canonicalize()?;
        let control = control.canonicalize()?;
        let roots = vec![
            FilesystemGrant {
                name: DATA.into(),
                path: data_root.clone(),
                read: true,
                write: true,
            },
            FilesystemGrant {
                name: CONTROL.into(),
                path: control.clone(),
                read: true,
                write: true,
            },
        ];
        let policy = Policy::new(PolicyConfig {
            allow: [PROVIDER.into(), "providers.execute_bound".into()].into(),
            filesystem: roots.clone(),
            ..Default::default()
        })?;
        let audit = audit::Audit::open(&root.path().join("audit"), 65_536, 2)?;
        let broker = Broker::new(
            policy,
            vec![],
            audit,
            Arc::new(NoApprover),
            None,
            json!({}),
            false,
        )?;
        let manifest = serde_json::from_value(json!({
            "manifest_version":1,"protocol":9,"id":"owner-permit-fixture",
            "version":env!("CARGO_PKG_VERSION"),"publisher":"semwright-tests",
            "executable":executable,"sha256":digest,
            "application":{"desktop_id":"org.semwright.OwnerOperationFixture",
                "process_names":[],"supported_versions":[]},
            "transport":"stdio_v1",
            "mounts":[
                {"root":DATA,"read_only":false,"execute":false},
                {"root":CONTROL,"read_only":false,"execute":false}],
            "system_config":[],"secrets":[],"tools":[],"network":false,
            "nvidia_gpu":false,"loopback_port":null,
            "resources":{"open_files":128,"processes":16,"cpu_seconds":20,
                "operation_cpu_seconds":0,"address_space_bytes":536870912,
                "file_size_bytes":16777216},
            "request_timeout_ms":15_000,
            "interfaces":{"dynamic_capabilities":false,"cooperative_cancellation":true,
                "events":false,"progress":false,"artifacts":false,"health":true,
                "native_refs":true,"host_tools":false}
        }))?;
        let provider = DriverProvider::connect(manifest, &state, &helper, &roots, false).await?;
        if let Err(error) = broker.mount_driver_provider(provider.clone()).await {
            if let Err(close) = provider.shutdown().await {
                eprintln!("fixture mount failed: {error:?}; physical cleanup failed: {close:?}");
                return Err(error.uncertain());
            }
            return Err(error);
        }
        let lease = match broker.provider(PROVIDER) {
            Some(lease) => lease,
            None => {
                let closed = provider.shutdown().await;
                eprintln!("mounted fixture missing from catalog; cleanup: {closed:?}");
                return Err(Error::new(
                    ErrorCode::BackendFailed,
                    "Mounted fixture missing from catalog",
                )
                .uncertain());
            }
        };
        Ok(Self {
            broker,
            provider,
            lease,
            session: unique_id(),
            data: data_root,
            control,
            request: None,
            cancellation: CancellationToken::new(),
            _root: root,
        })
    }
    async fn call(&self, session: &str, command: &str, args: Value) -> Envelope {
        self.broker
            .clone()
            .execute(
                session.into(),
                unique_id(),
                request(command, args),
                CancellationToken::new(),
            )
            .await
    }
    async fn start(&mut self, closing: bool) -> Result<()> {
        let mount = data(
            self.call(
                &self.session,
                "providers.mount.observe",
                json!({
            "provider":PROVIDER,"root":DATA}),
            )
            .await,
        )?;
        require(
            mount["mount"]["root"] == json!(DATA) && mount["mount"]["write"] == json!(true),
            "Actual data capsule is not the expected writable workspace",
        )?;
        let binding = mount["binding_ref"]
            .as_str()
            .ok_or_else(|| Error::invalid("Missing genuine mount reference"))?;
        let observed = data(self.call(&self.session, OBSERVE, query()).await)?;
        let native = observed["ref"]
            .as_str()
            .ok_or_else(|| Error::invalid("Missing genuine native reference"))?;
        require(
            native.starts_with("native:"),
            "Observation did not materialize a native ref",
        )?;
        let call = request(
            "providers.execute_bound",
            json!({
            "binding_ref":binding,"request":{"command":APPLY,
                "args":{"ref":native,"mode":if closing {"closing"} else {"commit"}},
                "dry_run":false}}),
        );
        self.request = Some(tokio::spawn(self.broker.clone().execute(
            self.session.clone(),
            unique_id(),
            call,
            self.cancellation.clone(),
        )));
        Ok(())
    }
    async fn marker(&self, name: &str, expected: &[u8]) -> Result<()> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            match fs::read(self.control.join(name)) {
                Ok(bytes) => return require(bytes == expected, "Fixture marker differs"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(
                    Error::new(ErrorCode::Timeout, "Fixture marker deadline expired").uncertain(),
                );
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    async fn original_result(&mut self) -> Result<Envelope> {
        let task = self
            .request
            .as_mut()
            .ok_or_else(|| Error::new(ErrorCode::Internal, "Missing original request task"))?;
        let joined = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .map_err(|_| {
                Error::new(
                    ErrorCode::Timeout,
                    "Original request terminal is unconfirmed",
                )
                .uncertain()
            })?;
        self.request.take();
        joined.map_err(|error| {
            Error::new(
                ErrorCode::Internal,
                format!("Original request task failed: {error}"),
            )
            .uncertain()
        })
    }
    async fn scenario(&mut self, closing: bool) -> Result<()> {
        self.start(closing).await?;
        self.marker("entered", b"entered\n").await?;
        require(
            !self.lease.operation_gate.drained()?,
            "Real Host permit was not retained",
        )?;
        require(
            !self.data.join("effect").exists(),
            "Effect preceded the fixture release",
        )?;
        require(
            self.lease.active(),
            "Original provider died before the guard checkpoint",
        )?;
        if closing {
            self.broker.revoke_session(&self.session);
            require(
                !self.cancellation.is_cancelled(),
                "Session revoke cancelled outer Execute",
            )?;
            self.marker("close-seen", b"closed\n").await?;
            require(
                !self.lease.operation_gate.drained()?,
                "Owner Close falsely acknowledged a borrowed guard as drained",
            )?;
            require(
                self.lease.active(),
                "Close killed the provider instead of closing the scope",
            )?;
            require(
                !self.data.join("effect").exists(),
                "Closed guard performed an effect",
            )?;
        }
        publish_control(&self.control, "release")?;
        let terminal = self.original_result().await?;
        match serde_json::to_string(&terminal) {
            Ok(original) => eprintln!("ORIGINAL_OWNER_PERMIT_TERMINAL_JSON={original}"),
            Err(error) => eprintln!(
                "ORIGINAL_OWNER_PERMIT_TERMINAL_JSON_UNAVAILABLE={error}; typed={terminal:?}"
            ),
        }
        tokio::time::timeout(
            Duration::from_secs(3),
            self.lease.operation_gate.wait_drained(),
        )
        .await
        .map_err(|_| {
            Error::new(
                ErrorCode::Timeout,
                "Matching owner Release did not drain the gate",
            )
            .uncertain()
        })??;
        if closing {
            require(
                !terminal.ok,
                "Closed permit reported a successful application effect",
            )?;
            let error = terminal.error.ok_or_else(|| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "Closed permit lost its typed refusal",
                )
            })?;
            require(
                error.code == ErrorCode::Cancelled,
                "Native permit refusal changed its typed cause",
            )?;
            require(
                !error.outcome_known,
                "Host Failure was incorrectly promoted to a known public outcome",
            )?;
            require(
                !self.data.join("effect").exists(),
                "Closed permit created the effect file",
            )?;
        } else {
            let result = data(terminal)?;
            require(
                result["result"]["data"]["committed"] == json!(true),
                "The original bound Native result was not preserved",
            )?;
            require(
                fs::read(self.data.join("effect"))? == b"effect-once\n",
                "Expected exactly one original application effect",
            )?;
        }
        require(
            !self.cancellation.is_cancelled(),
            "Outer request cancellation was manufactured",
        )?;
        let current = self
            .broker
            .provider(PROVIDER)
            .ok_or_else(|| Error::new(ErrorCode::BackendFailed, "Original provider disappeared"))?;
        require(
            Arc::ptr_eq(&current, &self.lease) && current.active(),
            "Drain came from provider replacement or termination",
        )?;
        let probe_session = unique_id();
        let live = data(self.call(&probe_session, OBSERVE, query()).await)?;
        require(
            live["page"]["version"]["resource"] == json!("fixture:owner-operation"),
            "Original Native process did not answer after Release",
        )?;
        require(
            self.lease.active() && self.lease.operation_gate.drained()?,
            "Live transport did not retain the completed Release state",
        )?;
        Ok(())
    }
    async fn cleanup(&mut self) -> Result<()> {
        let physical = self.provider.shutdown().await;
        let original_terminal = if let Some(mut task) = self.request.take() {
            match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(error)) => Err(Error::new(
                    ErrorCode::Internal,
                    format!("Original request task failed during cleanup: {error}"),
                )
                .uncertain()),
                Err(_) => {
                    task.abort();
                    if tokio::time::timeout(Duration::from_secs(1), &mut task)
                        .await
                        .is_err()
                    {
                        eprintln!("ORIGINAL_OWNER_PERMIT_WAITER_ABORT_UNCONFIRMED");
                        self.request = Some(task);
                    }
                    Err(Error::new(
                        ErrorCode::Timeout,
                        "Original request waiter remained uncertain after cleanup",
                    )
                    .uncertain())
                }
            }
        } else {
            Ok(())
        };
        if let Err(error) = physical {
            eprintln!("physical fixture cleanup failed: {error:?}; request: {original_terminal:?}");
            return Err(error);
        }
        original_terminal
    }
}
async fn run_case(closing: bool) {
    let mut fixture = Fixture::new().await.expect("real Linux fixture setup");
    let scenario =
        match tokio::time::timeout(Duration::from_secs(20), fixture.scenario(closing)).await {
            Ok(result) => result,
            Err(_) => Err(Error::new(
                ErrorCode::Timeout,
                "Real owner-permit scenario exceeded its deadline",
            )
            .uncertain()),
        };
    let cleanup = fixture.cleanup().await;
    let passed = scenario.is_ok() && cleanup.is_ok();
    if passed {
        fixture._root.remove_on_drop = true;
    }
    assert!(
        passed,
        "real owner-permit scenario: {scenario:?}; physical cleanup: {cleanup:?}"
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires actual Linux Bubblewrap/Landlock and the built NativeSDK fixture"]
async fn linux_owner_permit_native_success_releases_while_process_stays_live() {
    run_case(false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires actual Linux Bubblewrap/Landlock and the built NativeSDK fixture"]
async fn linux_owner_permit_session_close_keeps_guard_until_real_release() {
    run_case(true).await;
}
