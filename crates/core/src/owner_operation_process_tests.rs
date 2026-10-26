//! Real Linux subprocess test. No fake Provider, capsule, Host or authority.
//! The original fixed-file cases remain; optional external Motionwright cases below
//! exercise actual SQLite/Graph through this same real Core/Host9 fixture.
use super::{Broker, NoApprover, audit};
use crate::providers::ProviderLease;
use semwright_backend_api::Provider;
use semwright_driver_host::DriverProvider;
use semwright_policy::{
    FilesystemGrant, NativeProviderReadPolicy, NativeResourceReadPolicy, Policy, PolicyConfig,
};
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
/// Diagnostic custody only, populated after a genuine Host connection and
/// exactly at a Broker registration refusal. It never supplies authority.
struct RegistrationRefusal {
    root: PathBuf,
    original: Error,
    shutdown_confirmed: bool,
    evidence_retained: bool,
}
impl Fixture {
    async fn new() -> Result<Self> {
        Ok(Self::new_program(None).await?.0)
    }
    async fn new_program(graph: Option<GraphProgram>) -> Result<(Self, Option<GraphRuntime>)> {
        Self::new_program_recording_registration(graph, None).await
    }
    async fn new_program_recording_registration(
        graph: Option<GraphProgram>,
        registration_refusal: Option<&mut Option<RegistrationRefusal>>,
    ) -> Result<(Self, Option<GraphRuntime>)> {
        let (driver_env, provider_key, application_id) = if graph.is_some() {
            (
                "SEMWRIGHT_TEST_MOTIONWRIGHT_NATIVE_PROVIDER",
                GRAPH_PROVIDER,
                "motionwright",
            )
        } else {
            (
                "SEMWRIGHT_TEST_OWNER_PERMIT_DRIVER",
                PROVIDER,
                "owner-permit-fixture",
            )
        };
        require(
            Path::new("/usr/bin/bwrap").is_file(),
            "Real Bubblewrap is required",
        )?;
        let source = PathBuf::from(
            std::env::var_os(driver_env)
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
        let root = if graph.is_some() {
            CustodyRoot {
                directory: Some(
                    tempfile::Builder::new()
                        .prefix("graph-host9-")
                        .tempdir_in(graph_output_root()?)?,
                ),
                remove_on_drop: false,
            }
        } else {
            CustodyRoot::new()?
        };
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
        if graph.is_some() {
            graph_pin(
                "SEMWRIGHT_TEST_MOTIONWRIGHT_NATIVE_PROVIDER_SHA256",
                &fs::read(&executable)?,
            )?;
        }
        let data_root = data_root.canonicalize()?;
        let control = control.canonicalize()?;
        let graph_runtime = match graph {
            Some(program) => Some(
                prepare_graph_runtime(root.path(), &bin, &data_root, &control, program, &digest)
                    .await?,
            ),
            None => None,
        };
        let application_version = match &graph_runtime {
            Some(runtime) => graph_field(&runtime.source, "application_version")?,
            None => env!("CARGO_PKG_VERSION"),
        };
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
        let native_reads = match (&graph_runtime, graph) {
            (Some(runtime), Some(program)) => {
                Some(graph_read_setup(root.path(), &roots, runtime, program.read_policy).await?)
            }
            _ => None,
        };
        let policy = Policy::new(PolicyConfig {
            allow: [provider_key.into(), "providers.execute_bound".into()].into(),
            filesystem: roots.clone(),
            native_resource_reads: native_reads
                .as_ref()
                .map(|setup| setup.policies.clone())
                .unwrap_or_default(),
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
        if let Some(setup) = native_reads {
            broker.configure_project_graphs(&setup.directory, setup.principal)?;
        }
        let manifest = serde_json::from_value(json!({
            "manifest_version":1,"protocol":9,"id":application_id,
            "version":application_version,"publisher":"semwright-tests",
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
            let shutdown = provider.shutdown().await;
            if let Some(slot) = registration_refusal {
                let retained = graph_write_json(
                    &root.path().join("REGISTRATION_REFUSAL.original.json"),
                    &json!({
                        "schema":"motionwright.sec07-registration-refusal/1",
                        "native_context":"REAL_CORE_HOST9","provider":provider_key,
                        "host_connected":true,"registration_error":error,
                        "original_provider_shutdown":match &shutdown {
                            Ok(())=>json!({"ok":true}), Err(close)=>json!({"ok":false,"error":close})
                        },
                        "automatic_retry":false,"formal_acceptance":false
                    }),
                );
                *slot = Some(RegistrationRefusal {
                    root: root.path().to_path_buf(),
                    original: error.clone(),
                    shutdown_confirmed: shutdown.is_ok(),
                    evidence_retained: retained.is_ok(),
                });
                if let Err(secondary) = retained {
                    eprintln!(
                        "SEC07 registration refusal custody failed: {secondary:?}; original={error:?}"
                    );
                }
            }
            if let Err(close) = shutdown {
                eprintln!("fixture mount failed: {error:?}; physical cleanup failed: {close:?}");
                return Err(error.uncertain());
            }
            return Err(error);
        }
        let lease = match broker.provider(provider_key) {
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
        Ok((
            Self {
                broker,
                provider,
                lease,
                session: unique_id(),
                data: data_root,
                control,
                request: None,
                cancellation: CancellationToken::new(),
                _root: root,
            },
            graph_runtime,
        ))
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

// These tests reuse the two real owner-permit scenarios and Fixture above.
// No Motionwright dependency is linked into Core and no provider/context is faked.
const GRAPH_PROVIDER: &str = "driver:motionwright";
const GRAPH_OBSERVE: &str = "driver.motionwright.observe";
const GRAPH_UPSERT: &str = "driver.motionwright.graph.upsert";

#[derive(Clone, Copy)]
struct GraphProgram {
    database: &'static str,
    checkpoint: Option<&'static str>,
    security: bool,
    read_policy: &'static str,
}
struct GraphRuntime {
    tool: PathBuf,
    source: Value,
}

struct GraphReadSetup {
    directory: PathBuf,
    principal: String,
    policies: Vec<NativeProviderReadPolicy>,
}

/// Create the real canonical Graph boundary through the public owner command.
/// The private Graph state is outside every application-granted filesystem root.
/// No fixture JSON, copied project UUID, or fabricated prj_ID creates authority.
async fn graph_read_setup(
    root: &Path,
    roots: &[FilesystemGrant],
    runtime: &GraphRuntime,
    mode: &'static str,
) -> Result<GraphReadSetup> {
    require(
        matches!(mode, "owned" | "both" | "empty" | "omitted"),
        "Unknown closed native read policy scenario",
    )?;
    let directory = root.join("graph-state");
    private_directory(&directory)?;
    let principal = semwright_platform_services::current_user_principal()?;
    let bootstrap = Broker::new(
        Policy::new(PolicyConfig {
            allow: ["project.manage".into()].into(),
            filesystem: roots.to_vec(),
            ..Default::default()
        })?,
        vec![],
        audit::Audit::open(&root.join("graph-bootstrap-audit"), 65_536, 2)?,
        Arc::new(NoApprover),
        None,
        json!({}),
        false,
    )?;
    bootstrap.configure_project_graphs(&directory, principal.clone())?;
    let terminal = bootstrap
        .clone()
        .execute(
            unique_id(),
            unique_id(),
            request("project.create", json!({"root":DATA})),
            CancellationToken::new(),
        )
        .await;
    let created = data(graph_terminal_custody(
        root,
        "graph-read-bootstrap",
        terminal,
    )?)?;
    let project = graph_field(&created, "project")?.to_owned();
    require(
        project.starts_with("prj_") && project.len() == 36,
        "Public Graph creation did not return a canonical project identity",
    )?;
    let resource = graph_field(&runtime.source, "resource")?.to_owned();
    let mut resources = vec![NativeResourceReadPolicy {
        resource: resource.clone(),
        root: DATA.into(),
        project: project.clone(),
    }];
    if mode == "both" {
        // A second real Graph project makes this an independently authorized
        // resource, so a later ref/query mismatch cannot pass merely because
        // the first resource check denies the query.
        let terminal = bootstrap
            .clone()
            .execute(
                unique_id(),
                unique_id(),
                request("project.create", json!({"root":DATA})),
                CancellationToken::new(),
            )
            .await;
        let created = data(graph_terminal_custody(
            root,
            "graph-read-bootstrap-second",
            terminal,
        )?)?;
        let second = graph_field(&created, "project")?.to_owned();
        require(
            second.starts_with("prj_") && second.len() == 36 && second != project,
            "Second public Graph creation did not return a distinct canonical project",
        )?;
        resources.push(NativeResourceReadPolicy {
            resource: graph_field(&runtime.source["security"], "foreign_resource")?.to_owned(),
            root: DATA.into(),
            project: second,
        });
    }
    drop(bootstrap);
    let policies = match mode {
        "owned" | "both" => vec![NativeProviderReadPolicy {
            provider: GRAPH_PROVIDER.into(),
            resources,
        }],
        "empty" => vec![NativeProviderReadPolicy {
            provider: GRAPH_PROVIDER.into(),
            resources: vec![],
        }],
        "omitted" => vec![],
        _ => unreachable!("closed policy mode was validated"),
    };
    graph_write_json(
        &root.join("native-read-owner-selection.original.json"),
        &json!({
            "schema":"motionwright.sec07-owner-selection/1",
            "provider":GRAPH_PROVIDER,"mode":mode,"resource":resource,
            "root":DATA,"canonical_project":project,"principal":principal,
            "project_created_by":"project.create","policies":policies,
            "native_authority":false
        }),
    )?;
    Ok(GraphReadSetup {
        directory,
        principal,
        policies,
    })
}

fn graph_write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
        .map_err(|error| Error::from(error).uncertain())?;
    file.sync_all()
        .map_err(|error| Error::from(error).uncertain())?;
    drop(file);
    require(
        fs::read(path)? == bytes,
        "Graph fixture retained bytes differ",
    )?;
    Ok(())
}
fn graph_write_json(path: &Path, value: &Value) -> Result<()> {
    graph_write_new(path, &serde_json::to_vec_pretty(value)?)
}
fn graph_pin(variable: &str, bytes: &[u8]) -> Result<String> {
    let expected = std::env::var(variable)
        .map_err(|_| Error::invalid(format!("Set the built binary pin: {variable}")))?;
    let actual = hex::encode(Sha256::digest(bytes));
    require(
        expected.len() == 64
            && expected
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && expected == actual,
        "Graph fixture executable differs from the owner-pinned original",
    )?;
    Ok(actual)
}
fn graph_output_root() -> Result<PathBuf> {
    let path = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_GRAPH_OUTPUT_ROOT")
            .ok_or_else(|| Error::invalid("Set the new retained Graph evidence parent"))?,
    )
    .canonicalize()?;
    require(
        path.is_dir(),
        "Graph evidence parent must be an existing directory",
    )?;
    Ok(path)
}
fn graph_field<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(format!("Graph fixture field is absent: {field}")))
}
fn graph_query(source: &Value) -> Result<Value> {
    Ok(json!({"resource":graph_field(source, "resource")?,"scope":"summary","limit":1}))
}
fn graph_revision(value: &Value) -> Result<u64> {
    value["project_revision"]
        .as_u64()
        .ok_or_else(|| Error::invalid("Graph SQLite readback revision is absent"))
}
fn graph_contains_source(value: &Value, source: &Value) -> bool {
    if value == source {
        return true;
    }
    match value {
        Value::Array(values) => values
            .iter()
            .any(|value| graph_contains_source(value, source)),
        Value::Object(values) => values
            .values()
            .any(|value| graph_contains_source(value, source)),
        _ => false,
    }
}
fn graph_bound_data(envelope: Envelope) -> Result<Value> {
    let wrapper = data(envelope)?;
    wrapper["result"]
        .get("data")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| {
            Error::new(
                ErrorCode::BackendFailed,
                "Original Graph bound result is absent",
            )
        })
}
fn graph_release(control: &Path, name: &str) -> Result<()> {
    require(
        matches!(name, "continue-enter" | "continue-store" | "release-guard"),
        "Unknown Graph fixture release name",
    )?;
    let staging = control.join(format!("{name}.pending"));
    graph_write_new(&staging, b"go\n")?;
    fs::hard_link(&staging, control.join(name)).map_err(|error| Error::from(error).uncertain())?;
    fs::remove_file(staging).map_err(|error| Error::from(error).uncertain())?;
    Ok(())
}
const GRAPH_TOOL_PREFIX_LIMIT: usize = 64 * 1024;

struct GraphToolStream {
    path: PathBuf,
    file: fs::File,
    opened: fs::Metadata,
    prefix: Vec<u8>,
    observed_bytes: Option<u64>,
    written_bytes: usize,
    read_started: bool,
    read_finished: bool,
    eof_observed: bool,
    read_error: Option<String>,
    custody_error: Option<String>,
    sync_confirmed: bool,
}

impl GraphToolStream {
    fn reserve(root: &Path, label: &str, stream: &str) -> Result<Self> {
        let path = root.join(format!("{label}.{stream}.original"));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let opened = file.metadata()?;
        require(
            opened.is_file() && opened.len() == 0,
            "Graph tool capture is not a new regular file",
        )?;
        Ok(Self {
            path,
            file,
            opened,
            prefix: Vec::with_capacity(GRAPH_TOOL_PREFIX_LIMIT),
            observed_bytes: Some(0),
            written_bytes: 0,
            read_started: false,
            read_finished: false,
            eof_observed: false,
            read_error: None,
            custody_error: None,
            sync_confirmed: false,
        })
    }

    fn truncated(&self) -> bool {
        self.observed_bytes
            .is_none_or(|count| count > GRAPH_TOOL_PREFIX_LIMIT as u64)
    }

    fn receive(&mut self, bytes: &[u8]) -> Result<()> {
        self.observed_bytes = self
            .observed_bytes
            .and_then(|count| count.checked_add(bytes.len() as u64));
        let keep = bytes.len().min(GRAPH_TOOL_PREFIX_LIMIT - self.prefix.len());
        self.prefix.extend_from_slice(&bytes[..keep]);
        // Each successful write count belongs to this exact original prefix.
        // After an error no write to this stream is retried or moved elsewhere.
        if self.custody_error.is_none() {
            while self.written_bytes < self.prefix.len() {
                match self.file.write(&self.prefix[self.written_bytes..]) {
                    Ok(0) => {
                        self.custody_error = Some("original prefix write returned zero".into());
                        return Err(Error::new(
                            ErrorCode::Internal,
                            "Graph tool original prefix write made no progress",
                        )
                        .uncertain());
                    }
                    Ok(count) => self.written_bytes += count,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        self.custody_error = Some(format!("{error:?}"));
                        return Err(Error::from(error).uncertain());
                    }
                }
            }
        }
        if self.truncated() {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Graph tool original stream exceeded its retained prefix budget",
            )
            .uncertain());
        }
        Ok(())
    }

    fn finish_custody(&mut self) {
        use std::os::unix::fs::MetadataExt;
        if self.custody_error.is_some() {
            return;
        }
        // These synchronous calls are observed by the external owner, not
        // claimed to be bounded by the asynchronous process deadline.
        if let Err(error) = self.file.sync_all() {
            self.custody_error = Some(format!("original prefix sync failed: {error:?}"));
            return;
        }
        let checked = (|| -> std::io::Result<bool> {
            let retained = self.file.metadata()?;
            let named = fs::symlink_metadata(&self.path)?;
            Ok([&retained, &named].iter().all(|row| {
                row.is_file()
                    && row.dev() == self.opened.dev()
                    && row.ino() == self.opened.ino()
                    && row.len() == self.written_bytes as u64
            }))
        })();
        match checked {
            Ok(true) => self.sync_confirmed = true,
            Ok(false) => {
                self.custody_error = Some("original prefix pathname/descriptor changed".into())
            }
            Err(error) => {
                self.custody_error = Some(format!("original prefix metadata failed: {error:?}"))
            }
        }
    }

    fn complete(&self) -> bool {
        self.read_started
            && self.eof_observed
            && self.read_error.is_none()
            && self.custody_error.is_none()
            && !self.truncated()
            && self.written_bytes == self.prefix.len()
            && self.sync_confirmed
    }

    fn report(&self) -> Value {
        json!({
            "file":self.path.file_name().and_then(|name| name.to_str()),
            "prefix_limit":GRAPH_TOOL_PREFIX_LIMIT,
            "read_started":self.read_started,"eof_observed":self.eof_observed,
            "observed_bytes":self.observed_bytes,"count_overflow":self.observed_bytes.is_none(),
            "prefix_bytes":self.prefix.len(),"prefix_sha256":hex::encode(Sha256::digest(&self.prefix)),
            "prefix_truncated":self.truncated(),"written_bytes":self.written_bytes,
            "written_sha256":hex::encode(Sha256::digest(&self.prefix[..self.written_bytes])),
            "sync_confirmed":self.sync_confirmed,"original_complete":self.complete(),
            "read_error":self.read_error,"custody_error":self.custody_error
        })
    }
}

struct GraphToolCapture {
    root: PathBuf,
    label: String,
    request: Value,
    stdout: GraphToolStream,
    stderr: GraphToolStream,
    pid: Option<u32>,
    spawn_returned_child: bool,
    status: Option<std::process::ExitStatus>,
    original_error: Option<Error>,
    cleanup_started: bool,
    kill_attempted: bool,
    kill_requested: bool,
    kill_error: Option<String>,
    wait_errors: Vec<String>,
    execution_deadline_expired: bool,
    cleanup_deadline_expired: bool,
}

impl GraphToolCapture {
    fn reserve(tool: &Path, args: &[String], root: &Path, label: &str) -> Result<Self> {
        let executable = serde_json::to_value(tool)?;
        let request = json!({
            "schema":"motionwright.graph-fixture-process-request/1",
            "executable":executable,"argv":args,"stdin":"null",
            "execution_wait_ms":6000,"cleanup_wait_ms":2000,
            "stream_prefix_limit":GRAPH_TOOL_PREFIX_LIMIT,"automatic_retry":false
        });
        graph_write_json(
            &root.join(format!("{label}.request.original.json")),
            &request,
        )?;
        // Reserve both original byte destinations before there is a child.
        let stdout = GraphToolStream::reserve(root, label, "stdout")?;
        let stderr = GraphToolStream::reserve(root, label, "stderr")?;
        Ok(Self {
            root: root.into(),
            label: label.into(),
            request,
            stdout,
            stderr,
            pid: None,
            spawn_returned_child: false,
            status: None,
            original_error: None,
            cleanup_started: false,
            kill_attempted: false,
            kill_requested: false,
            kill_error: None,
            wait_errors: Vec::new(),
            execution_deadline_expired: false,
            cleanup_deadline_expired: false,
        })
    }

    fn fail(&mut self, original: Error) {
        if self.original_error.is_none() {
            self.original_error = Some(original);
        }
    }

    fn observe_status(&mut self, status: std::process::ExitStatus) {
        if !status.success() {
            self.fail(
                Error::new(
                    ErrorCode::BackendFailed,
                    format!("Original Graph source tool exited {status}; no retry"),
                )
                .uncertain(),
            );
        }
        self.status = Some(status);
    }

    fn begin_cleanup(&mut self, child: &mut tokio::process::Child) {
        self.cleanup_started = true;
        if self.status.is_none() && !self.kill_attempted {
            self.kill_attempted = true;
            match child.start_kill() {
                Ok(()) => self.kill_requested = true,
                Err(error) => self.kill_error = Some(format!("{error:?}")),
            }
        }
    }

    fn finish(mut self) -> Result<()> {
        self.stdout.finish_custody();
        self.stderr.finish_custody();
        if self.status.is_none() && self.spawn_returned_child {
            self.fail(
                Error::new(
                    ErrorCode::Timeout,
                    "Original Graph source process closure remains unconfirmed",
                )
                .uncertain(),
            );
        }
        if !self.stdout.complete() || !self.stderr.complete() {
            self.fail(
                Error::new(
                    ErrorCode::BackendFailed,
                    "Original Graph source stream custody is incomplete",
                )
                .uncertain(),
            );
        }
        let report = json!({
            "schema":"motionwright.graph-fixture-process/2",
            "request":self.request,"pid":self.pid,"spawn_returned_child":self.spawn_returned_child,
            "process_exit":self.status.as_ref().map(|status| json!({"code":status.code(),"success":status.success()})),
            "process_exit_observed":self.status.is_some(),
            "stdout":self.stdout.report(),"stderr":self.stderr.report(),
            "streams_complete":self.stdout.complete() && self.stderr.complete(),
            "original_error":self.original_error,
            "cleanup_started":self.cleanup_started,"kill_attempted":self.kill_attempted,
            "kill_requested":self.kill_requested,"kill_error":self.kill_error,"wait_errors":self.wait_errors,
            "execution_deadline_expired":self.execution_deadline_expired,
            "cleanup_deadline_expired":self.cleanup_deadline_expired,
            "synchronous_storage_wall_time_bounded":false,
            "external_owner_observation_required":true,"retry":false
        });
        let publication = graph_write_json(
            &self.root.join(format!("{}.terminal.json", self.label)),
            &report,
        );
        if publication.is_err() {
            let mut diagnostic = std::io::stderr().lock();
            let _ = serde_json::to_writer(&mut diagnostic, &report);
            let _ = writeln!(
                diagnostic,
                "\nGraph process terminal custody failed: {publication:?}; no retry"
            );
        }
        match self.original_error {
            Some(original) => Err(original),
            None => publication.map_err(Error::uncertain),
        }
    }
}

async fn graph_run_tool(tool: &Path, args: &[String], root: &Path, label: &str) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let mut capture = GraphToolCapture::reserve(tool, args, root, label)?;
    let mut command = tokio::process::Command::new(tool);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let execution_deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            capture.fail(Error::from(error));
            return capture.finish();
        }
    };
    capture.spawn_returned_child = true;
    capture.pid = child.id();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    capture.stdout.read_started = stdout.is_some();
    capture.stderr.read_started = stderr.is_some();
    for (present, name) in [(stdout.is_some(), "stdout"), (stderr.is_some(), "stderr")] {
        if !present {
            capture.fail(
                Error::new(
                    ErrorCode::Internal,
                    format!("Original Graph source process has no {name} pipe"),
                )
                .uncertain(),
            );
        }
    }
    if let Err(error) = graph_write_json(
        &root.join(format!("{label}.started.original.json")),
        &json!({
            "schema":"motionwright.graph-fixture-process-started/1","pid":capture.pid,
            "request":capture.request,"terminal_observed":false,"automatic_retry":false
        }),
    ) {
        capture.fail(error.uncertain());
    }
    let mut out_buffer = [0_u8; 8192];
    let mut err_buffer = [0_u8; 8192];
    let mut cleanup_deadline = None;
    let mut wait_allowed = true;
    loop {
        // Observe the original clock before accepting even already-ready EOF
        // or exit results after a delayed read/write event.
        if cleanup_deadline.is_none() && tokio::time::Instant::now() >= execution_deadline {
            capture.execution_deadline_expired = true;
            capture.fail(
                Error::new(
                    ErrorCode::Timeout,
                    "Original Graph fixture source process deadline expired",
                )
                .uncertain(),
            );
        }
        let stdout_finished = stdout.is_none() || capture.stdout.read_finished;
        let stderr_finished = stderr.is_none() || capture.stderr.read_finished;
        if capture.original_error.is_some() && cleanup_deadline.is_none() {
            capture.begin_cleanup(&mut child);
            cleanup_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(2));
            // One cleanup wait may still reap the same Child after a wait error.
            wait_allowed = true;
        }
        let deadline = cleanup_deadline.unwrap_or(execution_deadline);
        if tokio::time::Instant::now() >= deadline {
            capture.cleanup_deadline_expired = cleanup_deadline.is_some();
            break;
        }
        if capture.status.is_some() && stdout_finished && stderr_finished {
            break;
        }
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(deadline) => {
                if cleanup_deadline.is_some() {
                    capture.cleanup_deadline_expired = true;
                    break;
                }
                capture.execution_deadline_expired = true;
                capture.fail(Error::new(ErrorCode::Timeout,
                    "Original Graph fixture source process deadline expired").uncertain());
            }
            result = child.wait(), if wait_allowed && capture.status.is_none() => {
                match result {
                    Ok(status) => capture.observe_status(status),
                    Err(error) => {
                        capture.wait_errors.push(format!("{error:?}"));
                        capture.fail(Error::from(error).uncertain());
                        wait_allowed = false;
                    }
                }
            }
            result = async {
                match stdout.as_mut() {
                    Some(pipe) => pipe.read(&mut out_buffer).await,
                    None => std::future::pending().await,
                }
            }, if !stdout_finished => {
                match result {
                    Ok(0) => { capture.stdout.eof_observed = true; capture.stdout.read_finished = true; }
                    Ok(count) => {
                        if let Err(error) = capture.stdout.receive(&out_buffer[..count]) { capture.fail(error); }
                    }
                    Err(error) => {
                        capture.stdout.read_error = Some(format!("{error:?}"));
                        capture.stdout.read_finished = true;
                        capture.fail(Error::from(error).uncertain());
                    }
                }
            }
            result = async {
                match stderr.as_mut() {
                    Some(pipe) => pipe.read(&mut err_buffer).await,
                    None => std::future::pending().await,
                }
            }, if !stderr_finished => {
                match result {
                    Ok(0) => { capture.stderr.eof_observed = true; capture.stderr.read_finished = true; }
                    Ok(count) => {
                        if let Err(error) = capture.stderr.receive(&err_buffer[..count]) { capture.fail(error); }
                    }
                    Err(error) => {
                        capture.stderr.read_error = Some(format!("{error:?}"));
                        capture.stderr.read_finished = true;
                        capture.fail(Error::from(error).uncertain());
                    }
                }
            }
        }
    }
    // Dropping a pipe is not EOF evidence, and kill-on-drop is not a confirmed
    // process exit. Those observations stay separate in the retained terminal.
    drop((stdout, stderr));
    drop(child);
    capture.finish()
}
async fn prepare_graph_runtime(
    root: &Path,
    bin: &Path,
    data_root: &Path,
    control: &Path,
    program: GraphProgram,
    provider_digest: &str,
) -> Result<GraphRuntime> {
    require(
        matches!(program.database, "default" | "equivalent" | "copied"),
        "Unknown closed Graph database scenario",
    )?;
    require(
        matches!(program.checkpoint, None | Some("before-enter" | "commit")),
        "Unknown closed Graph checkpoint scenario",
    )?;
    let original_tool = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_MOTIONWRIGHT_GRAPH_FIXTURE")
            .ok_or_else(|| Error::invalid("Set the built Motionwright Graph source fixture"))?,
    )
    .canonicalize()?;
    let tool = bin.join("graph-source-tool");
    fs::copy(&original_tool, &tool)?;
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o500))?;
    let tool_digest = graph_pin(
        "SEMWRIGHT_TEST_MOTIONWRIGHT_GRAPH_FIXTURE_SHA256",
        &fs::read(&tool)?,
    )?;
    graph_write_new(
        &control.join("provider-mode"),
        format!("{}\n", program.database).as_bytes(),
    )?;
    if let Some(checkpoint) = program.checkpoint {
        graph_write_new(
            &control.join("checkpoint-mode"),
            format!("{checkpoint}\n").as_bytes(),
        )?;
    }
    if program.security {
        graph_write_new(&control.join("native-read-mode"), b"trace\n")?;
    }
    let source_path = root.join("source.original.json");
    graph_run_tool(
        &tool,
        &[
            if program.security {
                "seed-security"
            } else {
                "seed"
            }
            .into(),
            data_root.join("motionwright.sqlite3").display().to_string(),
            source_path.display().to_string(),
        ],
        root,
        "seed",
    )
    .await?;
    let source: Value = serde_json::from_slice(&fs::read(&source_path)?)?;
    require(
        source["schema"] == "motionwright.graph-owner-fixture-source/1"
            && source["application"] == "motionwright"
            && source["graph_persisted"] == false
            && source["native_authority"] == false,
        "Original Motionwright source fixture identity differs",
    )?;
    let _ = graph_field(&source, "application_version")?;
    let _ = graph_field(&source, "resource")?;
    require(
        source["graph"].is_object() && source["project"].is_object(),
        "Original typed Graph/Project is absent",
    )?;
    if program.security {
        let security = &source["security"];
        let foreign = graph_field(security, "foreign_resource")?;
        let absent = graph_field(security, "absent_resource")?;
        let own = graph_field(&source, "resource")?;
        let foreign_id = graph_field(&security["foreign_project"], "id")?;
        let marker = graph_field(security, "foreign_marker")?;
        require(
            security["schema"] == "motionwright.sec07-fixture-source/1"
                && foreign == format!("project:{foreign_id}")
                && foreign != own
                && absent != own
                && absent != foreign
                && absent.starts_with("project:")
                && marker.len() >= 16
                && marker.len() <= 128,
            "SEC07 source does not contain distinct owned, foreign and absent resources",
        )?;
    }
    if program.database == "copied" {
        // The seed producer has exited and closed SQLite before this exact copy.
        fs::copy(
            data_root.join("motionwright.sqlite3"),
            data_root.join("override-copy.sqlite3"),
        )?;
    }
    graph_write_json(
        &root.join("runtime.original.json"),
        &json!({
            "schema":"motionwright.graph-owner-runtime/1","database_mode":program.database,
            "checkpoint":program.checkpoint,"provider_sha256":provider_digest,
            "security_fixture":program.security,"native_read_policy":program.read_policy,
            "source_tool_sha256":tool_digest,"source_tool_original":original_tool,
            "sdk_package_version":env!("CARGO_PKG_VERSION"),
            "source_original_sha256":hex::encode(Sha256::digest(fs::read(&source_path)?)),
            "native_context":"REAL_CORE_HOST9",
            "context_factory":false,"permit_mock":false
        }),
    )?;
    Ok(GraphRuntime { tool, source })
}

// This is custody of an already observed terminal, not a second execution or
// a substitute for the actual Core/Host/owner admission used by these cases.
fn graph_retain_terminal(
    terminal: Envelope,
    retain: impl FnOnce(&Envelope) -> Result<()>,
) -> Result<Envelope> {
    if let Err(secondary) = retain(&terminal) {
        // A failed diagnostic cannot replace the original typed outcome either.
        let _ = writeln!(
            std::io::stderr().lock(),
            "ORIGINAL_GRAPH_TERMINAL_UNRETAINED={terminal:?}; custody={secondary:?}; no retry"
        );
        if let Some(original) = terminal.error {
            return Err(original);
        }
        return Err(secondary.uncertain());
    }
    Ok(terminal)
}

fn graph_terminal_custody(root: &Path, label: &str, terminal: Envelope) -> Result<Envelope> {
    graph_retain_terminal(terminal, |original| {
        // Serialization belongs to the same fallible custody operation as the
        // exclusive publication. Neither may overwrite an observed native Error.
        let original = serde_json::to_value(original)?;
        graph_write_json(
            &root.join(format!("{label}.terminal.original.json")),
            &original,
        )
    })
}

struct GraphFixture {
    base: Fixture,
    runtime: GraphRuntime,
    mode: &'static str,
    readback_number: u32,
}
impl GraphFixture {
    async fn new(database: &'static str, checkpoint: Option<&'static str>) -> Result<Self> {
        Self::from_program(GraphProgram {
            database,
            checkpoint,
            security: false,
            read_policy: "owned",
        })
        .await
    }
    async fn new_security(read_policy: &'static str) -> Result<Self> {
        Self::from_program(GraphProgram {
            database: "default",
            checkpoint: None,
            security: true,
            read_policy,
        })
        .await
    }
    async fn from_program(program: GraphProgram) -> Result<Self> {
        let (base, runtime) = Fixture::new_program(Some(program)).await?;
        let runtime =
            runtime.ok_or_else(|| Error::new(ErrorCode::Internal, "Graph runtime was lost"))?;
        Ok(Self {
            base,
            runtime,
            mode: program.database,
            readback_number: 0,
        })
    }
    fn root(&self) -> &Path {
        self.base._root.path()
    }
    async fn readback(&mut self, database: &str) -> Result<Value> {
        let project_id = graph_field(&self.runtime.source["project"], "id")?.to_owned();
        self.readback_project(database, &project_id).await
    }
    async fn readback_project(&mut self, database: &str, project_id: &str) -> Result<Value> {
        require(
            matches!(database, "motionwright.sqlite3" | "override-copy.sqlite3"),
            "Unknown Graph readback database",
        )?;
        self.readback_number += 1;
        let label = format!("readback-{:04}", self.readback_number);
        let target = self.root().join(format!("{label}.json"));
        graph_run_tool(
            &self.runtime.tool,
            &[
                "readback".into(),
                self.base.data.join(database).display().to_string(),
                project_id.to_owned(),
                target.display().to_string(),
            ],
            self.root(),
            &label,
        )
        .await?;
        let value: Value = serde_json::from_slice(&fs::read(target)?)?;
        require(
            value["schema"] == "motionwright.graph-owner-fixture-readback/1"
                && value["project"].is_object()
                && value["history"].is_array(),
            "Original SQLite readback is malformed",
        )?;
        Ok(value)
    }
    async fn current_mount(&self, session: &str) -> Result<String> {
        let mount = data(
            self.base
                .call(
                    session,
                    "providers.mount.observe",
                    json!({"provider":GRAPH_PROVIDER,"root":DATA}),
                )
                .await,
        )?;
        require(
            mount["mount"]["root"] == DATA && mount["mount"]["write"] == true,
            "Motionwright did not receive the actual writable data capsule",
        )?;
        Ok(graph_field(&mount, "binding_ref")?.to_owned())
    }
    async fn observe_bound(&self, session: &str, binding: &str, args: Value) -> Envelope {
        self.base
            .call(
                session,
                "providers.execute_bound",
                json!({
                    "binding_ref":binding,
                    "request":{"command":GRAPH_OBSERVE,"args":args,"dry_run":false}
                }),
            )
            .await
    }
    async fn current_refs(&self, session: &str) -> Result<(String, String, Value)> {
        let binding = self.current_mount(session).await?;
        let observed = graph_bound_data(
            self.observe_bound(session, &binding, graph_query(&self.runtime.source)?)
                .await,
        )?;
        let native = graph_field(&observed, "ref")?.to_owned();
        require(
            native.starts_with("native:"),
            "Graph observation did not return a genuine Native Ref",
        )?;
        Ok((binding, native, observed))
    }
    async fn start_public(&mut self, command: &str, mut args: Value) -> Result<String> {
        require(
            self.base.request.is_none(),
            "An original Graph request is still retained",
        )?;
        let (binding, native, observed) = self.current_refs(&self.base.session).await?;
        args.as_object_mut()
            .ok_or_else(|| Error::invalid("Graph command arguments are not an object"))?
            .insert("ref".into(), json!(native));
        let outer = unique_id();
        let call = request(
            "providers.execute_bound",
            json!({
                "binding_ref":binding,"request":{"command":command,"args":args,"dry_run":false}
            }),
        );
        graph_write_json(
            &self.root().join(format!("request-{outer}.original.json")),
            &json!({
                "outer_request_id":outer,"observation":observed,"request":call
            }),
        )?;
        self.base.request = Some(tokio::spawn(self.base.broker.clone().execute(
            self.base.session.clone(),
            outer.clone(),
            call,
            self.base.cancellation.clone(),
        )));
        Ok(outer)
    }
    async fn terminal(&mut self, label: &str) -> Result<Envelope> {
        let terminal = self.base.original_result().await?;
        graph_terminal_custody(self.root(), label, terminal)
    }
    async fn cleanup(&mut self) -> Result<()> {
        // The same single shutdown and bounded waiter used by the original
        // fixture. Graph cases additionally retain every Envelope actually
        // received here instead of discarding a pending native outcome.
        let physical = self.base.provider.shutdown().await;
        let original_terminal = if let Some(mut task) = self.base.request.take() {
            match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
                Ok(Ok(terminal)) => {
                    graph_terminal_custody(self.root(), "cleanup-request", terminal).map(|_| ())
                }
                Ok(Err(error)) => Err(Error::new(
                    ErrorCode::Internal,
                    format!("Original Graph request task failed during cleanup: {error}"),
                )
                .uncertain()),
                Err(_) => {
                    let original = Error::new(
                        ErrorCode::Timeout,
                        "Original Graph request waiter remained uncertain after cleanup",
                    )
                    .uncertain();
                    task.abort();
                    match tokio::time::timeout(Duration::from_secs(1), &mut task).await {
                        Ok(Ok(terminal)) => {
                            // Completion can win the abort race. Retain that
                            // actual late terminal separately; do not turn the
                            // already observed wait deadline into success.
                            if let Err(secondary) = graph_terminal_custody(
                                self.root(),
                                "cleanup-request-late",
                                terminal,
                            ) {
                                let _ = writeln!(
                                    std::io::stderr().lock(),
                                    "Graph late terminal custody failed: {secondary:?}; original wait={original:?}; no retry"
                                );
                            }
                        }
                        Ok(Err(join_error)) => {
                            let _ = writeln!(
                                std::io::stderr().lock(),
                                "Graph original waiter aborted or failed: {join_error:?}; original wait={original:?}; native terminal unavailable"
                            );
                        }
                        Err(_) => {
                            let _ = writeln!(
                                std::io::stderr().lock(),
                                "ORIGINAL_GRAPH_WAITER_ABORT_UNCONFIRMED; original wait={original:?}"
                            );
                            self.base.request = Some(task);
                        }
                    }
                    Err(original)
                }
            }
        } else {
            Ok(())
        };
        match (original_terminal, physical) {
            (Err(original), physical) => {
                if let Err(secondary) = physical {
                    let _ = writeln!(
                        std::io::stderr().lock(),
                        "Secondary Graph physical cleanup failure: {secondary:?}; original request={original:?}; no retry"
                    );
                }
                Err(original)
            }
            (Ok(()), physical) => physical,
        }
    }
    async fn checkpoint(&self, name: &str, phase: &str) -> Result<Value> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
        loop {
            match fs::read(self.base.control.join(name)) {
                Ok(bytes) => {
                    let value: Value = serde_json::from_slice(&bytes)?;
                    require(
                        value["schema"] == "motionwright.graph-owner-checkpoint/1"
                            && value["phase"] == phase
                            && value["native_authority"] == false
                            && value["provider_process_id"]
                                .as_u64()
                                .is_some_and(|pid| pid > 0),
                        "Original Graph checkpoint differs",
                    )?;
                    let _ = graph_field(&value, "request_id")?;
                    return Ok(value);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::new(
                    ErrorCode::Timeout,
                    "Original Graph checkpoint is unconfirmed",
                )
                .uncertain());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    fn retained_guard(&self) -> Result<()> {
        require(
            !self.base.lease.operation_gate.drained()?,
            "Actual Graph owner reservation was acknowledged as drained before release",
        )?;
        require(
            self.base.lease.active(),
            "Original Motionwright provider was terminated",
        )?;
        require(
            !self.base.cancellation.is_cancelled(),
            "Outer Graph cancellation was manufactured",
        )?;
        let current = self.base.broker.provider(GRAPH_PROVIDER).ok_or_else(|| {
            Error::new(
                ErrorCode::BackendFailed,
                "Original Graph provider disappeared",
            )
        })?;
        require(
            Arc::ptr_eq(&current, &self.base.lease),
            "Graph provider was replaced",
        )?;
        Ok(())
    }
    async fn drained(&self) -> Result<()> {
        tokio::time::timeout(
            Duration::from_secs(4),
            self.base.lease.operation_gate.wait_drained(),
        )
        .await
        .map_err(|_| {
            Error::new(
                ErrorCode::Timeout,
                "Actual Graph guard release is unconfirmed",
            )
            .uncertain()
        })??;
        Ok(())
    }
    async fn remains_live(&self) -> Result<()> {
        let session = unique_id();
        let (_, _, observed) = self.current_refs(&session).await?;
        require(
            observed["page"]["version"]["resource"] == self.runtime.source["resource"],
            "Original provider did not observe the same SQLite project after release",
        )?;
        let current = self.base.broker.provider(GRAPH_PROVIDER).ok_or_else(|| {
            Error::new(
                ErrorCode::BackendFailed,
                "Graph provider disappeared after release",
            )
        })?;
        require(
            Arc::ptr_eq(&current, &self.base.lease)
                && current.active()
                && current.operation_gate.drained()?,
            "Graph release depended on provider replacement or termination",
        )?;
        Ok(())
    }
    fn committed_once(&self, before: &Value, after: &Value) -> Result<()> {
        require(
            graph_revision(after)? == graph_revision(before)? + 1,
            "Original Graph mutation did not advance SQLite exactly once",
        )?;
        require(
            after["history"].as_array().unwrap().len()
                == before["history"].as_array().unwrap().len() + 1,
            "Original Graph mutation did not append exactly one application event",
        )?;
        require(
            graph_contains_source(&after["project"], &self.runtime.source["graph"]),
            "SQLite does not retain the exact original editable Graph source",
        )?;
        Ok(())
    }
    async fn successful_write_and_historical_replay(&mut self) -> Result<()> {
        let before = self.readback("motionwright.sqlite3").await?;
        require(
            before["project"] == self.runtime.source["project"],
            "Actual mounted database differs from the original source",
        )?;
        self.start_public(GRAPH_UPSERT, json!({"graph":self.runtime.source["graph"]}))
            .await?;
        let entered = self.checkpoint("entered.json", "entered").await?;
        let checked = self
            .checkpoint("current-checked.json", "current-checked")
            .await?;
        require(
            entered["request_id"] == checked["request_id"]
                && entered["provider_process_id"] == checked["provider_process_id"],
            "Guard checkpoints crossed native invocations",
        )?;
        self.retained_guard()?;
        let pending = self.readback("motionwright.sqlite3").await?;
        require(
            pending == before,
            "SQLite mutated before the admitted Store checkpoint was released",
        )?;
        graph_release(&self.base.control, "continue-store")?;
        let returned = self
            .checkpoint("store-returned.json", "store-returned-guard-retained")
            .await?;
        require(
            returned["request_id"] == entered["request_id"]
                && returned["provider_process_id"] == entered["provider_process_id"],
            "Store checkpoint changed the original invocation/process",
        )?;
        let committed = self.readback("motionwright.sqlite3").await?;
        self.committed_once(&before, &committed)?;
        self.retained_guard()?;
        graph_release(&self.base.control, "release-guard")?;
        let original = graph_bound_data(self.terminal("graph-original").await?)?;
        self.drained().await?;
        require(
            original["replayed"] == false
                && original["project"] == committed["project"]
                && original["request_id"] == entered["request_id"],
            "Original native Graph return differs from its independent SQLite commit",
        )?;
        self.remains_live().await?;
        fs::remove_file(self.base.control.join("checkpoint-mode"))?;

        // Advance actual application state through another existing native command.
        // The following Graph replay must return the older authenticated receipt,
        // while its authorization observes the newer current project.
        self.start_public(
            "driver.motionwright.project.rename",
            json!({"title":"Current state after the original Graph receipt"}),
        )
        .await?;
        let renamed = graph_bound_data(self.terminal("rename-current").await?)?;
        let current = self.readback("motionwright.sqlite3").await?;
        require(
            graph_revision(&current)? == graph_revision(&committed)? + 1
                && renamed["project"] == current["project"]
                && current["project"] != committed["project"],
            "Historical replay fixture did not acquire a distinct later current state",
        )?;
        let original_request = graph_field(&original, "request_id")?.to_owned();
        let (binding, native, observed) = self.current_refs(&self.base.session).await?;
        require(
            observed["page"]["version"]["revision"] == graph_revision(&current)?.to_string(),
            "Replay Native Ref is not current",
        )?;
        let bound = self
            .base
            .broker
            .resolve_bound_provider_for_owner_operation_test(&self.base.session, &binding)?;
        let replay = request(
            GRAPH_UPSERT,
            json!({
                "ref":native,"graph":self.runtime.source["graph"]
            }),
        );
        graph_write_json(
            &self.root().join("replay-internal.original.json"),
            &json!({
                "scope":"CANONICAL_CORE_INTERNAL_DURABLE_REQUEST_TEST",
                "original_child_request_id":original_request,"current_observation":observed,
                "request":replay,"public_wrapper_replay":false,
                "reason":"providers.execute_bound generates a distinct child request ID on every call"
            }),
        )?;
        // Reuse the exact private canonical path called by execute_provider_bound.
        // The bound handle is resolved now from a genuine current mount reference.
        // execute_inner still performs real policy/ref/Core/Host9 authorization.
        let broker = self.base.broker.clone();
        let session = self.base.session.clone();
        let cancellation = self.base.cancellation.clone();
        self.base.request = Some(tokio::spawn(async move {
            broker
                .execute_inner(session, original_request, replay, cancellation, Some(bound))
                .await
        }));
        let replayed = data(self.terminal("graph-replay-internal").await?)?;
        self.drained().await?;
        require(
            replayed["replayed"] == true
                && replayed["project"] == original["project"]
                && replayed["revision"] == original["revision"]
                && replayed["request_id"] == original["request_id"],
            "Genuine current access did not return the exact historical Graph receipt",
        )?;
        let after_replay = self.readback("motionwright.sqlite3").await?;
        require(
            after_replay == current,
            "Historical replay changed the current SQLite source/history",
        )?;
        self.base.broker.revoke_session(&self.base.session);
        require(
            self.base
                .broker
                .resolve_bound_provider_for_owner_operation_test(&self.base.session, &binding)
                .is_err(),
            "Revoked access remained capable of resolving a replay operation",
        )?;
        self.drained().await?;
        self.remains_live().await?;
        graph_write_json(
            &self.root().join("RESULT.json"),
            &json!({
                "schema":"motionwright.graph-host9-physical-case/1","case":"write-and-historical-replay",
                "database_mode":self.mode,"graph_writes":1,"ordinary_later_writes":1,
                "sqlite_commit_observed_while_guard_retained":true,
                "observer_same_service_state":true,"historical_replay_with_current_access":true,
                "revoked_binding_denied":true,"internal_canonical_replay":true,
                "public_wrapper_replay":false,"native_context":"REAL_CORE_HOST9",
                "provider_replaced":false,"automatic_retry":false,"formal_acceptance":false
            }),
        )
    }
    async fn copied_database_denied(&mut self) -> Result<()> {
        let original = self.readback("motionwright.sqlite3").await?;
        let copied = self.readback("override-copy.sqlite3").await?;
        require(
            original == copied && copied["project"] == self.runtime.source["project"],
            "Copied SQLite fixture does not retain the same original identity/state",
        )?;
        let (_, _, observed) = self.current_refs(&self.base.session).await?;
        require(
            observed["page"]["version"]["generation"] == copied["project"]["generation"]
                && observed["page"]["version"]["revision"] == graph_revision(&copied)?.to_string(),
            "Ordinary copied-DB observation was not preserved",
        )?;
        self.start_public(GRAPH_UPSERT, json!({"graph":self.runtime.source["graph"]}))
            .await?;
        let terminal = self.terminal("copied-graph-denied").await?;
        require(
            !terminal.ok
                && terminal
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == ErrorCode::PermissionDenied),
            "Arbitrary copied DB did not receive the dedicated Graph denial",
        )?;
        self.drained().await?;
        require(
            self.readback("motionwright.sqlite3").await? == original
                && self.readback("override-copy.sqlite3").await? == copied,
            "Denied copied DB operation mutated a project or its history",
        )?;
        self.remains_live().await?;
        graph_write_json(
            &self.root().join("RESULT.json"),
            &json!({
                "schema":"motionwright.graph-host9-physical-case/1","case":"copied-database-denied",
                "same_uuid_generation_revision":true,"ordinary_read_preserved":true,
                "graph_writes":0,"native_context":"REAL_CORE_HOST9",
                "original_typed_terminal_retained":true,"outcome_reclassified":false,
                "automatic_retry":false,"formal_acceptance":false
            }),
        )
    }
    async fn revoke_before_enter(&mut self) -> Result<()> {
        let before = self.readback("motionwright.sqlite3").await?;
        self.start_public(GRAPH_UPSERT, json!({"graph":self.runtime.source["graph"]}))
            .await?;
        let before_enter = self.checkpoint("before-enter.json", "before-enter").await?;
        self.retained_guard()?;
        // This reservation has not been borrowed. Confirm actual matching release
        // after revocation before allowing any local enter attempt.
        self.base.broker.revoke_session(&self.base.session);
        self.drained().await?;
        require(
            !self.base.control.join("entered.json").exists(),
            "Graph entered its local guard before the confirmed revoke checkpoint",
        )?;
        graph_release(&self.base.control, "continue-enter")?;
        let terminal = self.terminal("revoked-before-enter").await?;
        require(
            !terminal.ok
                && terminal
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == ErrorCode::Cancelled),
            "Real SDK enter did not preserve its cancellation after confirmed revocation",
        )?;
        require(
            !self.base.control.join("entered.json").exists(),
            "Revoked reservation entered a local guard",
        )?;
        require(
            self.readback("motionwright.sqlite3").await? == before,
            "Confirmed revoke-before-enter changed SQLite source/history",
        )?;
        self.drained().await?;
        self.remains_live().await?;
        graph_write_json(
            &self.root().join("RESULT.json"),
            &json!({
                "schema":"motionwright.graph-host9-physical-case/1","case":"revoke-confirmed-before-enter",
                "original_child_request_id":before_enter["request_id"],"graph_writes":0,
                "actual_owner_release_confirmed_before_enter":true,"actual_enter_denied":true,
                "native_context":"REAL_CORE_HOST9","outcome_reclassified":false,
                "automatic_retry":false,"formal_acceptance":false
            }),
        )
    }
    async fn revoke_requested_after_current_check(&mut self) -> Result<()> {
        let before = self.readback("motionwright.sqlite3").await?;
        self.start_public(GRAPH_UPSERT, json!({"graph":self.runtime.source["graph"]}))
            .await?;
        let entered = self.checkpoint("entered.json", "entered").await?;
        let checked = self
            .checkpoint("current-checked.json", "current-checked")
            .await?;
        require(
            entered["request_id"] == checked["request_id"]
                && entered["provider_process_id"] == checked["provider_process_id"],
            "Revocation checkpoints crossed original invocations",
        )?;
        self.retained_guard()?;
        self.base.broker.revoke_session(&self.base.session);
        self.retained_guard()?;
        // Revocation is REQUESTED, not complete: the admitted operation is
        // allowed to finish while its actual guard and owner reservation live.
        graph_release(&self.base.control, "continue-store")?;
        let returned = self
            .checkpoint("store-returned.json", "store-returned-guard-retained")
            .await?;
        require(
            returned["request_id"] == checked["request_id"]
                && returned["provider_process_id"] == checked["provider_process_id"],
            "Commit checkpoint lost its real native invocation",
        )?;
        let committed = self.readback("motionwright.sqlite3").await?;
        self.committed_once(&before, &committed)?;
        self.retained_guard()?;
        graph_release(&self.base.control, "release-guard")?;
        let terminal = self
            .terminal("revoke-requested-after-current-check")
            .await?;
        self.drained().await?;
        // C00 rejects the now-historical outer completion after revocation.
        // Its uncertainty must coexist with the independent proof of commit.
        require(
            !terminal.ok
                && terminal.error.as_ref().is_some_and(|error| {
                    error.code == ErrorCode::StaleReference && !error.outcome_known
                }),
            "Revoked completion lost its original uncertain historical-result status",
        )?;
        require(
            self.readback("motionwright.sqlite3").await? == committed,
            "Committed source changed while completing the original revoked invocation",
        )?;
        self.remains_live().await?;
        graph_write_json(
            &self.root().join("RESULT.json"),
            &json!({
                "schema":"motionwright.graph-host9-physical-case/1","case":"revoke-requested-after-current-check",
                "graph_writes":1,"revocation_requested_after_real_current_check":true,
                "sqlite_commit_before_revocation_completed":true,
                "real_guard_retained_through_store_apply":true,"matching_release_confirmed":true,
                "outer_terminal_uncertain":true,"uncertainty_normalized_to_replay":false,
                "native_context":"REAL_CORE_HOST9","automatic_retry":false,"formal_acceptance":false
            }),
        )
    }
}

async fn run_graph_case(
    database: &'static str,
    checkpoint: Option<&'static str>,
    scenario: &'static str,
) {
    let mut fixture = GraphFixture::new(database, checkpoint)
        .await
        .expect("real Graph Host9 fixture setup");
    let scenario_result = match tokio::time::timeout(Duration::from_secs(50), async {
        match scenario {
            "success" => fixture.successful_write_and_historical_replay().await,
            "copied" => fixture.copied_database_denied().await,
            "before-enter" => fixture.revoke_before_enter().await,
            "after-current" => fixture.revoke_requested_after_current_check().await,
            _ => Err(Error::invalid("Unknown closed Graph scenario")),
        }
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(Error::new(
            ErrorCode::Timeout,
            "Original Graph Host9 scenario exceeded its deadline",
        )
        .uncertain()),
    };
    let cleanup = fixture.cleanup().await;
    // Preserve physical source, all original terminals and SQLite on success too.
    // The caller uploads SEMWRIGHT_TEST_GRAPH_OUTPUT_ROOT as the evidence artifact.
    let verdict = graph_write_json(
        &fixture.root().join("CASE_TERMINAL.json"),
        &json!({
            "schema":"motionwright.graph-host9-case-terminal/1","scenario":scenario,
            "database_mode":database,"scenario_result":match &scenario_result {
                Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"error":error})},
            "cleanup_result":match &cleanup {
                Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"error":error})},
            "automatic_retry":false,"formal_acceptance":false
        }),
    );
    assert!(
        scenario_result.is_ok() && cleanup.is_ok() && verdict.is_ok(),
        "real Graph Host9 scenario: {scenario_result:?}; cleanup: {cleanup:?}; custody: {verdict:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Linux real Host9, Bubblewrap/Landlock and the pinned Motionwright provider/source tool"]
async fn linux_motionwright_graph_native_store_commit_and_current_access_replay() {
    run_graph_case("default", Some("commit"), "success").await;
    run_graph_case("equivalent", Some("commit"), "success").await;
    run_graph_case("copied", None, "copied").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Linux real Host9, Bubblewrap/Landlock and the pinned Motionwright provider/source tool"]
async fn linux_motionwright_graph_native_revoke_before_enter_and_retained_commit() {
    run_graph_case("default", Some("before-enter"), "before-enter").await;
    run_graph_case("default", Some("commit"), "after-current").await;
}

// SEC07 controls retain the actual Host9 path and the same application Store.
// The source tool's JSON is fixture data only; Core creates all live grants.
fn security_query(resource: &str, scope: &str, reference: Option<&str>) -> Value {
    let mut query = json!({"resource":resource,"scope":scope,"limit":1});
    if let Some(reference) = reference {
        query["ref"] = json!(reference);
    }
    query
}

fn security_denial(terminal: Envelope) -> Result<Error> {
    require(
        !terminal.ok && terminal.data.is_none(),
        "Denied native read returned application data or a successful envelope",
    )?;
    let error = terminal
        .error
        .ok_or_else(|| Error::invalid("Denied native read has no typed error"))?;
    require(
        error.code == ErrorCode::PolicyDenied,
        "Native read was not denied by the canonical owner policy boundary",
    )?;
    Ok(error)
}

impl GraphFixture {
    fn native_read_entries(&self) -> Result<Vec<(String, Value)>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&self.base.control)? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| Error::invalid("Fixture entry name is not UTF-8"))?;
            if !name.starts_with("read-entry-") {
                continue;
            }
            let serial = name
                .strip_prefix("read-entry-")
                .and_then(|value| value.strip_suffix(".json"))
                .and_then(|value| value.parse::<u64>().ok());
            let metadata = fs::symlink_metadata(entry.path())?;
            require(
                serial.is_some_and(|value| value > 0)
                    && metadata.file_type().is_file()
                    && metadata.len() <= 4096
                    && entries.len() < 256,
                "Observer entry trace exceeded its finite original-file contract",
            )?;
            let bytes = fs::read(entry.path())?;
            require(
                bytes.len() <= 4096,
                "Observer entry trace grew beyond budget",
            )?;
            let value: Value = serde_json::from_slice(&bytes)?;
            require(
                value["schema"] == "motionwright.sec07-observer-entry/1"
                    && value["native_authority"] == false
                    && value["provider_process_id"]
                        .as_u64()
                        .is_some_and(|pid| pid > 0),
                "Observer entry trace lost its actual application invocation",
            )?;
            for field in ["request_id", "resource", "scope"] {
                require(
                    !graph_field(&value, field)?.is_empty(),
                    "Observer trace invocation field is empty",
                )?;
            }
            entries.push((name, value));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(entries)
    }

    async fn security_own_foreign_absent(&mut self) -> Result<()> {
        let source = self.runtime.source.clone();
        let own = graph_field(&source, "resource")?;
        let foreign = graph_field(&source["security"], "foreign_resource")?;
        let absent = graph_field(&source["security"], "absent_resource")?;
        let marker = graph_field(&source["security"], "foreign_marker")?;
        let foreign_id = graph_field(&source["security"]["foreign_project"], "id")?;
        // The existing foreign project is proved by independent production
        // Store reopening, not by a random UUID or the seed document alone.
        let foreign_before = self
            .readback_project("motionwright.sqlite3", foreign_id)
            .await?;
        require(
            foreign_before["project"]["id"] == source["security"]["foreign_project"]["id"]
                && foreign_before["project"]["title"] == marker,
            "Foreign-existing SEC07 fixture was not independently reopened from the same Store",
        )?;
        let own_before = self.readback("motionwright.sqlite3").await?;
        let session = self.base.session.clone();
        let binding = self.current_mount(&session).await?;
        let scopes = [
            "summary",
            "branches",
            "reviews",
            "history",
            "history-recent",
            "production-receipts",
            "production-jobs",
            "asset-integrity",
        ];
        let mut own_reference = None;
        let mut history_cursor = None;
        for (index, scope) in scopes.iter().enumerate() {
            let before = self.native_read_entries()?;
            let terminal = self
                .observe_bound(&session, &binding, security_query(own, scope, None))
                .await;
            let terminal =
                graph_terminal_custody(self.root(), &format!("sec07-own-{index}"), terminal)?;
            let observed = graph_bound_data(terminal)?;
            require(
                observed["page"]["version"]["resource"] == own
                    && observed["page"]["scope"] == *scope
                    && self.native_read_entries()?.len() == before.len() + 1,
                "Own-resource positive did not enter the actual observer exactly once",
            )?;
            let reference = graph_field(&observed, "ref")?.to_owned();
            require(
                reference.starts_with("native:"),
                "Own observation did not return a genuine NativeRef",
            )?;
            if *scope == "summary" {
                own_reference = Some(reference);
            }
            if *scope == "history" && !observed["page"]["next"].is_null() {
                history_cursor = Some(observed["page"]["next"].clone());
            }
            let before_denials = self.native_read_entries()?;
            let foreign_terminal = self
                .observe_bound(&session, &binding, security_query(foreign, scope, None))
                .await;
            let foreign_terminal = graph_terminal_custody(
                self.root(),
                &format!("sec07-foreign-{index}"),
                foreign_terminal,
            )?;
            let foreign_error = security_denial(foreign_terminal)?;
            let absent_terminal = self
                .observe_bound(&session, &binding, security_query(absent, scope, None))
                .await;
            let absent_terminal = graph_terminal_custody(
                self.root(),
                &format!("sec07-absent-{index}"),
                absent_terminal,
            )?;
            let absent_error = security_denial(absent_terminal)?;
            require(
                serde_json::to_value(&foreign_error)? == serde_json::to_value(&absent_error)?
                    && !foreign_error.message.contains(marker)
                    && self.native_read_entries()? == before_denials,
                "Foreign/absent denial leaked existence or reached the application observer",
            )?;
        }
        let reference =
            own_reference.ok_or_else(|| Error::invalid("Own NativeRef was not retained"))?;
        let before_mismatch = self.native_read_entries()?;
        let mismatch = self
            .observe_bound(
                &session,
                &binding,
                security_query(foreign, "summary", Some(&reference)),
            )
            .await;
        security_denial(graph_terminal_custody(
            self.root(),
            "sec07-ref-resource-mismatch",
            mismatch,
        )?)?;
        require(
            self.native_read_entries()? == before_mismatch,
            "Mismatched resource/ref performed an implicit freshness read",
        )?;
        let before_unbound = self.native_read_entries()?;
        let unbound = self
            .base
            .call(
                &session,
                GRAPH_OBSERVE,
                security_query(own, "summary", None),
            )
            .await;
        security_denial(graph_terminal_custody(
            self.root(),
            "sec07-unbound-own-denied",
            unbound,
        )?)?;
        require(
            self.native_read_entries()? == before_unbound,
            "Protected unbound read reached the application observer",
        )?;
        let cursor = history_cursor
            .ok_or_else(|| Error::invalid("Real own history did not yield a continuation"))?;
        let mut next = security_query(own, "history", Some(&reference));
        next["cursor"] = cursor.clone();
        let continuation = self.observe_bound(&session, &binding, next.clone()).await;
        let continuation = graph_bound_data(graph_terminal_custody(
            self.root(),
            "sec07-own-history-continuation",
            continuation,
        )?)?;
        require(
            continuation["page"]["version"]["resource"] == own,
            "Own history continuation lost its resource",
        )?;
        let before_foreign_cursor = self.native_read_entries()?;
        next["resource"] = json!(foreign);
        let foreign_cursor = self.observe_bound(&session, &binding, next).await;
        security_denial(graph_terminal_custody(
            self.root(),
            "sec07-foreign-cursor-denied",
            foreign_cursor,
        )?)?;
        require(
            self.native_read_entries()? == before_foreign_cursor,
            "Cursor or NativeRef bypassed the resource read grant",
        )?;
        require(
            self.readback("motionwright.sqlite3").await? == own_before,
            "Read controls modified the owned project/history",
        )?;
        require(
            self.readback_project("motionwright.sqlite3", foreign_id)
                .await?
                == foreign_before,
            "Read controls modified the foreign project/history",
        )?;
        // An operation payload's resource is a subresource, not a replacement
        // for the native project's read selector. Exercise that distinction via
        // the real public mutation, then independently reopen its exact result.
        let scene = graph_field(&source["project"]["scenes"][0], "id")?;
        let subresource = format!("scene:{scene}");
        let review_body = "SEC07 own-project subresource mutation control";
        self.start_public(
            "driver.motionwright.review.add",
            json!({
                "kind":"technical","resource":subresource,"body":review_body
            }),
        )
        .await?;
        let review = self.terminal("sec07-own-subresource-review").await?;
        let _ = graph_bound_data(review)?;
        let reviewed = self.readback("motionwright.sqlite3").await?;
        require(
            graph_revision(&reviewed)? == graph_revision(&own_before)? + 1
                && reviewed["history"].as_array().is_some_and(|history| {
                    history.len() == own_before["history"].as_array().map_or(0, Vec::len) + 1
                })
                && reviewed["project"]["reviews"]
                    .as_array()
                    .is_some_and(|reviews| {
                        reviews.iter().any(|review| {
                            review["anchor"]["resource"] == subresource
                                && review["body"] == review_body
                        })
                    }),
            "Legitimate review subresource was mistaken for a native project read selector",
        )?;
        require(
            self.readback_project("motionwright.sqlite3", foreign_id)
                .await?
                == foreign_before,
            "Own-project review changed the foreign project",
        )?;
        graph_write_json(
            &self.root().join("SEC07_RESULT.json"),
            &json!({
                "schema":"motionwright.sec07-host9-case/1","case":"own-foreign-absent",
                "native_context":"REAL_CORE_HOST9","same_store_foreign_readback":true,
                "own_scopes":scopes,"foreign_and_absent_denied_before_observer":true,
                "cursor_and_ref_do_not_grant_authority":true,"automatic_retry":false,
                "real_own_subresource_review_commits_once":true,
                "formal_acceptance":false
            }),
        )
    }

    async fn security_two_live_grants_reject_crossed_refs(&mut self) -> Result<()> {
        let source = self.runtime.source.clone();
        let first = graph_field(&source, "resource")?;
        let second = graph_field(&source["security"], "foreign_resource")?;
        let second_id = graph_field(&source["security"]["foreign_project"], "id")?;
        let first_before = self.readback("motionwright.sqlite3").await?;
        let second_before = self
            .readback_project("motionwright.sqlite3", second_id)
            .await?;
        require(
            second_before["project"] == source["security"]["foreign_project"],
            "Second authorized project is not the real same-Store seed",
        )?;
        let session = self.base.session.clone();
        let binding = self.current_mount(&session).await?;
        let mut observations = Vec::new();
        for (index, resource) in [first, second].into_iter().enumerate() {
            let before = self.native_read_entries()?;
            let terminal = self
                .observe_bound(
                    &session,
                    &binding,
                    security_query(resource, "history", None),
                )
                .await;
            let observed = graph_bound_data(graph_terminal_custody(
                self.root(),
                &format!("sec07-two-grants-initial-{index}"),
                terminal,
            )?)?;
            require(
                observed["page"]["version"]["resource"] == resource
                    && self.native_read_entries()?.len() == before.len() + 1,
                "Both independently mapped resources must reach the real observer",
            )?;
            let reference = graph_field(&observed, "ref")?.to_owned();
            let cursor = observed["page"]["next"].clone();
            require(
                reference.starts_with("native:") && !cursor.is_null(),
                "Two-grant control requires genuine live refs and actual continuations",
            )?;
            observations.push((reference, cursor));
        }
        require(
            observations[0].0 != observations[1].0,
            "Distinct resources unexpectedly shared a single native reference",
        )?;
        let before_denials = self.native_read_entries()?;
        for (index, (resource, crossed_reference)) in [
            (first, observations[1].0.as_str()),
            (second, observations[0].0.as_str()),
        ]
        .into_iter()
        .enumerate()
        {
            let terminal = self
                .observe_bound(
                    &session,
                    &binding,
                    security_query(resource, "summary", Some(crossed_reference)),
                )
                .await;
            security_denial(graph_terminal_custody(
                self.root(),
                &format!("sec07-two-grants-crossed-ref-{index}"),
                terminal,
            )?)?;
            require(
                self.native_read_entries()? == before_denials,
                "Authorized query plus another authorized resource's ref reached implicit fresh",
            )?;
        }
        let mut crossed_cursor = security_query(first, "history", Some(&observations[1].0));
        crossed_cursor["cursor"] = observations[1].1.clone();
        let terminal = self.observe_bound(&session, &binding, crossed_cursor).await;
        security_denial(graph_terminal_custody(
            self.root(),
            "sec07-two-grants-crossed-cursor",
            terminal,
        )?)?;
        require(
            self.native_read_entries()? == before_denials,
            "A genuine second-project cursor/ref crossed the first project's live grant",
        )?;
        // This time the query AND ref both name authorized A, so Core's ref
        // scope join passes. Only B's genuine cursor crosses the resource.
        // NativeSDK must validate that cursor before any implicit fresh read.
        let mut cursor_only = security_query(first, "history", Some(&observations[0].0));
        cursor_only["cursor"] = observations[1].1.clone();
        let terminal = self.observe_bound(&session, &binding, cursor_only).await;
        let terminal = graph_terminal_custody(
            self.root(),
            "sec07-two-grants-cursor-only-mismatch",
            terminal,
        )?;
        require(
            !terminal.ok
                && terminal.data.is_none()
                && terminal
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == ErrorCode::StaleReference),
            "Foreign cursor with matching authorized ref did not retain its original stale outcome",
        )?;
        let cursor_outcome_known = terminal
            .error
            .as_ref()
            .expect("typed stale outcome checked")
            .outcome_known;
        require(
            self.native_read_entries()? == before_denials,
            "Foreign cursor validation happened only after NativeSDK fresh reached the application",
        )?;
        // Preserve the Host's actual outcome_known value; source proof of no
        // observer entry does not rewrite a transport's uncertain terminal.
        // Matching continuations still work with the same live bindings and refs;
        // denial above cannot be attributed to expired or fabricated admissions.
        for (index, resource) in [first, second].into_iter().enumerate() {
            let mut query = security_query(resource, "history", Some(&observations[index].0));
            query["cursor"] = observations[index].1.clone();
            let terminal = self.observe_bound(&session, &binding, query).await;
            let observed = graph_bound_data(graph_terminal_custody(
                self.root(),
                &format!("sec07-two-grants-matched-continuation-{index}"),
                terminal,
            )?)?;
            require(
                observed["page"]["version"]["resource"] == resource,
                "A matching authorized continuation stopped working after crossed-ref denial",
            )?;
        }
        require(
            self.readback("motionwright.sqlite3").await? == first_before
                && self
                    .readback_project("motionwright.sqlite3", second_id)
                    .await?
                    == second_before,
            "Two-grant read controls modified either same-Store project",
        )?;
        graph_write_json(
            &self.root().join("SEC07_RESULT.json"),
            &json!({
                "schema":"motionwright.sec07-host9-case/1","case":"two-live-grants-crossed-refs",
                "native_context":"REAL_CORE_HOST9","both_resources_independently_authorized":true,
                "both_matching_continuations_succeeded":true,"crossed_ref_observer_calls":0,
                "crossed_cursor_observer_calls":0,"original_cursor_outcome_known":cursor_outcome_known,
                "automatic_retry":false,"formal_acceptance":false
            }),
        )
    }

    async fn security_default_denied(&mut self, mode: &str) -> Result<()> {
        require(
            mode == "empty",
            "Only an explicit empty protected policy can register without a read mapping",
        )?;
        let source = self.runtime.source.clone();
        let own = graph_field(&source, "resource")?;
        let session = self.base.session.clone();
        let binding = self.current_mount(&session).await?;
        let before = self.native_read_entries()?;
        let bound = self
            .observe_bound(&session, &binding, security_query(own, "summary", None))
            .await;
        security_denial(graph_terminal_custody(
            self.root(),
            "sec07-default-bound-denied",
            bound,
        )?)?;
        let unbound = self
            .base
            .call(
                &session,
                GRAPH_OBSERVE,
                security_query(own, "summary", None),
            )
            .await;
        security_denial(graph_terminal_custody(
            self.root(),
            "sec07-default-unbound-denied",
            unbound,
        )?)?;
        require(
            self.native_read_entries()? == before,
            "Protected provider with no read grant reached the application",
        )?;
        graph_write_json(
            &self.root().join("SEC07_RESULT.json"),
            &json!({
                "schema":"motionwright.sec07-host9-case/1","case":"default-deny","policy_mode":mode,
                "native_context":"REAL_CORE_HOST9","driver_capability_granted":true,
                "explicit_empty_mapping_does_not_unprotect_provider":true,
                "automatic_retry":false,"formal_acceptance":false
            }),
        )
    }

    async fn security_origin_expiry_before_fresh(&mut self) -> Result<()> {
        // These are genuine RefStore admissions with a deliberately finite TTL,
        // not reconstructed refs or a mocked clock. The native ref is issued
        // later and remains within its own TTL when its originating mount expires.
        *self
            .base
            .broker
            .references
            .lock()
            .map_err(|_| Error::invalid("Reference lock poisoned"))? =
            semwright_types::RefStore::new(Duration::from_secs(10), 64);
        let session = self.base.session.clone();
        let own = graph_field(&self.runtime.source, "resource")?.to_owned();
        let origin = self.current_mount(&session).await?;
        let origin_expires = self
            .base
            .broker
            .references
            .lock()
            .map_err(|_| Error::invalid("Reference lock poisoned"))?
            .get_provider_mount(&origin, &session)?
            .expires;
        tokio::time::sleep_until(tokio::time::Instant::from_std(
            origin_expires - Duration::from_secs(5),
        ))
        .await;
        let observed = self
            .observe_bound(&session, &origin, security_query(&own, "summary", None))
            .await;
        let observed = graph_bound_data(graph_terminal_custody(
            self.root(),
            "sec07-origin-own-positive",
            observed,
        )?)?;
        let native = graph_field(&observed, "ref")?.to_owned();
        let native_expires = self
            .base
            .broker
            .references
            .lock()
            .map_err(|_| Error::invalid("Reference lock poisoned"))?
            .get(&native, &session)?
            .expires;
        require(
            native_expires > origin_expires + Duration::from_secs(2),
            "NativeRef lifetime did not separate from its originating grant",
        )?;
        tokio::time::sleep_until(tokio::time::Instant::from_std(
            origin_expires + Duration::from_millis(20),
        ))
        .await;
        require(
            native_expires > std::time::Instant::now(),
            "NativeRef itself expired before the origin-scope control",
        )?;
        let fresh_binding = self.current_mount(&session).await?;
        let before = self.native_read_entries()?;
        let denied = self
            .observe_bound(
                &session,
                &fresh_binding,
                security_query(&own, "summary", Some(&native)),
            )
            .await;
        let denied =
            graph_terminal_custody(self.root(), "sec07-expired-origin-before-fresh", denied)?;
        require(
            !denied.ok
                && denied.data.is_none()
                && denied.error.as_ref().is_some_and(|error| {
                    matches!(
                        error.code,
                        ErrorCode::StaleReference | ErrorCode::PolicyDenied
                    )
                }),
            "A fresh binding revived a NativeRef whose original read admission expired",
        )?;
        require(
            self.native_read_entries()? == before,
            "Expired origin reached SDK fresh before refusal",
        )?;
        let positive = self
            .observe_bound(
                &session,
                &fresh_binding,
                security_query(&own, "summary", None),
            )
            .await;
        let positive = graph_bound_data(graph_terminal_custody(
            self.root(),
            "sec07-fresh-origin-positive",
            positive,
        )?)?;
        require(
            positive["page"]["version"]["resource"] == own
                && self.native_read_entries()?.len() == before.len() + 1,
            "Fresh legitimate initial read did not remain available after origin expiry",
        )?;
        graph_write_json(
            &self.root().join("SEC07_RESULT.json"),
            &json!({
                "schema":"motionwright.sec07-host9-case/1","case":"origin-expiry-before-fresh",
                "native_context":"REAL_CORE_HOST9","native_ref_own_ttl_still_live":true,
                "old_origin_cannot_be_rebound":true,"implicit_observer_calls_on_denial":0,
                "automatic_retry":false,"formal_acceptance":false
            }),
        )
    }

    async fn security_invalidate_after_provider_await(
        &mut self,
        expire: bool,
        outer: bool,
    ) -> Result<()> {
        require(
            !outer || expire,
            "The wrapper-completion control uses actual origin expiry",
        )?;
        if expire {
            *self
                .base
                .broker
                .references
                .lock()
                .map_err(|_| Error::invalid("Reference lock poisoned"))? =
                semwright_types::RefStore::new(Duration::from_secs(10), 64);
        }
        let session = self.base.session.clone();
        let own = graph_field(&self.runtime.source, "resource")?.to_owned();
        let binding = self.current_mount(&session).await?;
        let origin_expires = self
            .base
            .broker
            .references
            .lock()
            .map_err(|_| Error::invalid("Reference lock poisoned"))?
            .get_provider_mount(&binding, &session)?
            .expires;
        let before = self.native_read_entries()?;
        let pause = if outer {
            self.base.broker.pause_next_native_bound_read_completion()?
        } else {
            self.base.broker.pause_next_native_read_completion()?
        };
        let entered = pause.entered.notified();
        tokio::pin!(entered);
        entered.as_mut().enable();
        let outer_request = unique_id();
        let call = request(
            "providers.execute_bound",
            json!({
                "binding_ref":binding,
                "request":{"command":GRAPH_OBSERVE,"args":security_query(&own, "summary", None),"dry_run":false}
            }),
        );
        graph_write_json(
            &self.root().join("sec07-paused-request.original.json"),
            &json!({"outer_request_id":outer_request,"request":call,"wrapper_completion":outer}),
        )?;
        self.base.request = Some(tokio::spawn(self.base.broker.clone().execute(
            session.clone(),
            outer_request,
            call,
            self.base.cancellation.clone(),
        )));
        tokio::time::timeout(Duration::from_secs(6), &mut entered)
            .await
            .map_err(|_| {
                Error::new(
                    ErrorCode::Timeout,
                    "Actual provider completion pause was not reached",
                )
                .uncertain()
            })?;
        require(
            self.native_read_entries()?.len() == before.len() + 1,
            "Post-await pause was reached without the actual provider observation",
        )?;
        if outer {
            // The inner read guard has legitimately drained. The wrapper must
            // independently reject disclosure after its own nested await.
            require(
                self.base.lease.active(),
                "Original provider stopped before the wrapper completion control",
            )?;
        } else {
            self.retained_guard()?;
        }
        if expire {
            tokio::time::sleep_until(tokio::time::Instant::from_std(
                origin_expires + Duration::from_millis(20),
            ))
            .await;
            require(
                origin_expires <= std::time::Instant::now(),
                "Original read admission did not actually expire",
            )?;
        } else {
            self.base.broker.revoke_session(&session);
        }
        // The original provider result exists, but no page or NativeRef has yet
        // been returned to the caller. Do not cancel or replace the child.
        pause.proceed.notify_one();
        let label = if outer {
            "sec07-expired-after-bound-wrapper-await"
        } else if expire {
            "sec07-expired-after-provider-await"
        } else {
            "sec07-revoked-after-provider-await"
        };
        let terminal = self.terminal(label).await?;
        require(
            !terminal.ok
                && terminal.data.is_none()
                && terminal
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == ErrorCode::PolicyDenied),
            "Revoked post-await native read exposed a successful page or reference",
        )?;
        self.drained().await?;
        self.remains_live().await?;
        graph_write_json(
            &self.root().join("SEC07_RESULT.json"),
            &json!({
                "schema":"motionwright.sec07-host9-case/1",
                "case":if outer {"expiry-after-bound-wrapper-await"}
                    else if expire {"expiry-after-provider-await"} else {"revoke-after-provider-await"},
                "native_context":"REAL_CORE_HOST9","actual_provider_returned_before_invalidation":true,
                "actual_origin_expiry":expire,"wrapper_completion":outer,
                "success_data_released":false,"original_read_hold_drained":true,
                "same_provider_fresh_authorized_session_succeeded":true,
                "automatic_retry":false,"formal_acceptance":false
            }),
        )
    }
}

async fn run_security_case(mode: &'static str, scenario: &'static str) {
    let mut fixture = GraphFixture::new_security(mode)
        .await
        .expect("real SEC07 Core/Host9 fixture setup");
    let scenario_result = match tokio::time::timeout(Duration::from_secs(50), async {
        match scenario {
            "own-foreign-absent" => fixture.security_own_foreign_absent().await,
            "two-live-grants" => fixture.security_two_live_grants_reject_crossed_refs().await,
            "default-deny" => fixture.security_default_denied(mode).await,
            "origin-expiry" => fixture.security_origin_expiry_before_fresh().await,
            "post-await-revoke" => {
                fixture
                    .security_invalidate_after_provider_await(false, false)
                    .await
            }
            "post-await-expiry" => {
                fixture
                    .security_invalidate_after_provider_await(true, false)
                    .await
            }
            "wrapper-await-expiry" => {
                fixture
                    .security_invalidate_after_provider_await(true, true)
                    .await
            }
            _ => Err(Error::invalid("Unknown closed SEC07 scenario")),
        }
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(Error::new(
            ErrorCode::Timeout,
            "Original SEC07 scenario exceeded its deadline",
        )
        .uncertain()),
    };
    let cleanup = fixture.cleanup().await;
    let retained = graph_write_json(
        &fixture.root().join("CASE_TERMINAL.json"),
        &json!({
            "schema":"motionwright.sec07-host9-case-terminal/1","scenario":scenario,"policy_mode":mode,
            "scenario_result":match &scenario_result {Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"error":error})},
            "cleanup_result":match &cleanup {Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"error":error})},
            "automatic_retry":false,"formal_acceptance":false
        }),
    );
    assert!(
        scenario_result.is_ok() && cleanup.is_ok() && retained.is_ok(),
        "real SEC07 scenario: {scenario_result:?}; physical cleanup: {cleanup:?}; custody: {retained:?}"
    );
}

async fn run_security_missing_policy_registration_case() {
    let mut refusal = None;
    let result = Fixture::new_program_recording_registration(
        Some(GraphProgram {
            database: "default",
            checkpoint: None,
            security: true,
            read_policy: "omitted",
        }),
        Some(&mut refusal),
    )
    .await;
    let original = match result {
        Err(error) => error,
        Ok((mut unexpected, _)) => {
            let cleanup = unexpected.cleanup().await;
            let retained = graph_write_json(
                &unexpected._root.path().join("CASE_TERMINAL.json"),
                &json!({
                    "schema":"motionwright.sec07-host9-case-terminal/1",
                    "scenario":"missing-policy-registration","unexpected_registration_success":true,
                    "cleanup_result":match &cleanup {Ok(())=>json!({"ok":true}),Err(error)=>json!({"ok":false,"error":error})},
                    "automatic_retry":false,"formal_acceptance":false
                }),
            );
            panic!(
                "Protected provider unexpectedly registered without its owner policy entry; original cleanup: {cleanup:?}; custody: {retained:?}"
            );
        }
    };
    let observed = refusal.expect("Omitted-policy control failed before genuine Host connection and exact Broker registration refusal");
    // Only a recorded error at the actual mount boundary is admissible here.
    // Setup, sandbox, executable, bootstrap and shutdown failures cannot count
    // as the expected policy refusal or be normalized into a fake Envelope.
    let denied = observed.original.code == ErrorCode::PolicyDenied
        && observed.original.message == "Native resource access denied"
        && original.code == observed.original.code
        && original.message == observed.original.message
        && original.outcome_known == observed.original.outcome_known;
    let no_observer = fs::read_dir(observed.root.join("control"))
        .and_then(|entries| entries.collect::<std::io::Result<Vec<_>>>())
        .map(|entries| {
            !entries.into_iter().any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("read-entry-")
            })
        });
    let retained = graph_write_json(
        &observed.root.join("CASE_TERMINAL.json"),
        &json!({
            "schema":"motionwright.sec07-host9-case-terminal/1",
            "scenario":"missing-policy-registration","native_context":"REAL_CORE_HOST9",
            "original_registration_error":observed.original,"returned_original_error":original,
            "host_connected":true,"specific_registration_refusal":denied,
            "original_provider_shutdown_confirmed":observed.shutdown_confirmed,
            "original_refusal_custody_retained":observed.evidence_retained,
            "application_observer_not_entered":matches!(&no_observer, Ok(true)),
            "automatic_retry":false,"formal_acceptance":false
        }),
    );
    assert!(
        denied
            && observed.shutdown_confirmed
            && observed.evidence_retained
            && matches!(&no_observer, Ok(true))
            && retained.is_ok(),
        "Actual protected registration refusal: {original:?}; original: {:?}; shutdown: {}; refusal custody: {}; observer: {no_observer:?}; verdict custody: {retained:?}",
        observed.original,
        observed.shutdown_confirmed,
        observed.evidence_retained
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Linux real Host9, protected NativeSDK/Motionwright source, Bubblewrap/Landlock and pinned seed-security tool"]
async fn linux_motionwright_native_read_scope_own_foreign_absent_and_continuations() {
    run_security_case("owned", "own-foreign-absent").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Linux real Host9, protected NativeSDK/Motionwright source, Bubblewrap/Landlock and pinned seed-security tool"]
async fn linux_motionwright_native_read_scope_empty_and_missing_policy_are_denied() {
    run_security_case("empty", "default-deny").await;
    run_security_missing_policy_registration_case().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Linux real Host9, protected NativeSDK/Motionwright source, Bubblewrap/Landlock and pinned seed-security tool"]
async fn linux_motionwright_native_read_scope_two_live_grants_reject_crossed_refs() {
    run_security_case("both", "two-live-grants").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Linux real Host9, protected NativeSDK/Motionwright source, Bubblewrap/Landlock and pinned seed-security tool"]
async fn linux_motionwright_native_read_scope_expired_origin_is_checked_before_fresh() {
    run_security_case("owned", "origin-expiry").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Linux real Host9, protected NativeSDK/Motionwright source, Bubblewrap/Landlock and pinned seed-security tool"]
async fn linux_motionwright_native_read_scope_revoke_after_actual_provider_completion() {
    run_security_case("owned", "post-await-revoke").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Linux real Host9, protected NativeSDK/Motionwright source, Bubblewrap/Landlock and pinned seed-security tool"]
async fn linux_motionwright_native_read_scope_expiry_after_actual_provider_completion() {
    run_security_case("owned", "post-await-expiry").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Linux real Host9, protected NativeSDK/Motionwright source, Bubblewrap/Landlock and pinned seed-security tool"]
async fn linux_motionwright_native_read_scope_expiry_after_bound_wrapper_completion() {
    run_security_case("owned", "wrapper-await-expiry").await;
}

// These unit controls exercise terminal custody only. They neither instantiate
// nor stand in for a provider, CallContext, permit, Core/Host or application write.
#[test]
fn graph_terminal_serialization_failure_keeps_the_original_typed_error() -> Result<()> {
    struct RejectSerialization;
    impl serde::Serialize for RejectSerialization {
        fn serialize<S: serde::Serializer>(
            &self,
            _serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("terminal serialization control"))
        }
    }

    let terminal = Envelope::finish(
        "11111111111111111111111111111111".into(),
        GRAPH_UPSERT.into(),
        GRAPH_PROVIDER.into(),
        Duration::from_millis(3),
        false,
        Err(Error::new(
            ErrorCode::StaleReference,
            "Original historical Graph result remains uncertain",
        )
        .uncertain()),
    );
    let expected = serde_json::to_value(terminal.error.as_ref().unwrap())?;
    let calls = std::cell::Cell::new(0);
    let actual = graph_retain_terminal(terminal, |_| {
        calls.set(calls.get() + 1);
        let _ = serde_json::to_value(RejectSerialization)?;
        Ok(())
    })
    .expect_err("serialization failure must not accept a terminal without custody");
    assert_eq!(calls.get(), 1, "custody is never retried");
    assert_eq!(serde_json::to_value(&actual)?, expected);
    assert_eq!(actual.code, ErrorCode::StaleReference);
    assert!(!actual.outcome_known);
    Ok(())
}

#[test]
fn graph_terminal_exclusive_publication_failure_keeps_the_original_outcome() -> Result<()> {
    let root = tempfile::tempdir()?;
    let target = root.path().join("original.terminal.original.json");
    let retained = b"original custody slot must not be overwritten\n";
    graph_write_new(&target, retained)?;

    let terminal = Envelope::finish(
        "22222222222222222222222222222222".into(),
        GRAPH_UPSERT.into(),
        GRAPH_PROVIDER.into(),
        Duration::from_millis(5),
        false,
        Err(Error::new(
            ErrorCode::Cancelled,
            "Original Graph invocation remains uncertain after cancellation",
        )
        .uncertain()),
    );
    let expected = serde_json::to_value(terminal.error.as_ref().unwrap())?;
    let actual = graph_terminal_custody(root.path(), "original", terminal)
        .expect_err("exclusive publication must preserve the original typed refusal");
    assert_eq!(serde_json::to_value(&actual)?, expected);
    assert_eq!(actual.code, ErrorCode::Cancelled);
    assert!(!actual.outcome_known);
    assert_eq!(fs::read(&target)?, retained);

    // A different observed successful terminal with the same occupied slot
    // must fail custody as uncertain instead of being reported as accepted.
    let success = Envelope::finish(
        "33333333333333333333333333333333".into(),
        GRAPH_UPSERT.into(),
        GRAPH_PROVIDER.into(),
        Duration::from_millis(7),
        false,
        Ok(json!({"unit_custody_control":true})),
    );
    let custody = graph_terminal_custody(root.path(), "original", success)
        .expect_err("success without terminal custody is not accepted");
    assert!(!custody.outcome_known);
    assert_eq!(fs::read(&target)?, retained);
    Ok(())
}

// Byte/terminal custody controls only. The two subprocess controls run an inert
// fixed shell command through graph_run_tool itself; no Provider/Host/Graph
// permission or application mutation is constructed or inferred from them.
#[test]
fn graph_tool_binary_prefix_overflow_retains_original_bytes_without_eof() -> Result<()> {
    let root = tempfile::tempdir()?;
    let mut stream = GraphToolStream::reserve(root.path(), "overflow", "stdout")?;
    stream.read_started = true;
    let bytes: Vec<_> = (0..GRAPH_TOOL_PREFIX_LIMIT + 37)
        .map(|at| (at % 256) as u8)
        .collect();
    let error = stream.receive(&bytes).unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourceExhausted);
    assert!(!error.outcome_known);
    stream.finish_custody();
    assert_eq!(fs::read(&stream.path)?, bytes[..GRAPH_TOOL_PREFIX_LIMIT]);
    assert_eq!(stream.observed_bytes, Some(bytes.len() as u64));
    assert!(stream.truncated());
    assert!(stream.sync_confirmed);
    assert!(!stream.eof_observed);
    assert!(!stream.complete());
    Ok(())
}

#[tokio::test]
async fn graph_tool_known_nonzero_exit_retains_both_original_binary_streams() -> Result<()> {
    let root = tempfile::tempdir()?;
    let args = vec![
        "-c".into(),
        "printf '\\001\\377OUT'; printf '\\002\\376ERR' >&2; exit 7".into(),
    ];
    let error = graph_run_tool(Path::new("/bin/sh"), &args, root.path(), "nonzero")
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::BackendFailed);
    assert!(!error.outcome_known);
    assert_eq!(
        fs::read(root.path().join("nonzero.stdout.original"))?,
        b"\x01\xffOUT"
    );
    assert_eq!(
        fs::read(root.path().join("nonzero.stderr.original"))?,
        b"\x02\xfeERR"
    );
    let terminal: Value =
        serde_json::from_slice(&fs::read(root.path().join("nonzero.terminal.json"))?)?;
    assert_eq!(terminal["process_exit"], json!({"code":7,"success":false}));
    assert_eq!(terminal["streams_complete"], true);
    assert_eq!(terminal["stdout"]["eof_observed"], true);
    assert_eq!(terminal["stderr"]["eof_observed"], true);
    assert_eq!(terminal["kill_attempted"], false);
    assert_eq!(terminal["original_error"], serde_json::to_value(&error)?);
    assert!(terminal["pid"].as_u64().is_some());
    Ok(())
}

#[tokio::test]
async fn graph_tool_timeout_retains_prefixes_and_the_same_child_cleanup_terminal() -> Result<()> {
    let root = tempfile::tempdir()?;
    // exec replaces the shell in this same child PID; it does not create a
    // descendant holding a pipe. The real six-second deadline is unchanged.
    let args = vec![
        "-c".into(),
        "printf 'OUT-before-deadline'; printf 'ERR-before-deadline' >&2; exec sleep 30".into(),
    ];
    let error = graph_run_tool(Path::new("/bin/sh"), &args, root.path(), "timeout")
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Timeout);
    assert!(!error.outcome_known);
    assert_eq!(
        fs::read(root.path().join("timeout.stdout.original"))?,
        b"OUT-before-deadline"
    );
    assert_eq!(
        fs::read(root.path().join("timeout.stderr.original"))?,
        b"ERR-before-deadline"
    );
    let terminal: Value =
        serde_json::from_slice(&fs::read(root.path().join("timeout.terminal.json"))?)?;
    let started: Value = serde_json::from_slice(&fs::read(
        root.path().join("timeout.started.original.json"),
    )?)?;
    assert_eq!(terminal["pid"], started["pid"]);
    assert_eq!(terminal["execution_deadline_expired"], true);
    assert_eq!(terminal["cleanup_started"], true);
    assert_eq!(terminal["kill_attempted"], true);
    assert_eq!(terminal["kill_requested"], true);
    assert_eq!(terminal["process_exit_observed"], true);
    assert_eq!(terminal["stdout"]["eof_observed"], true);
    assert_eq!(terminal["stderr"]["eof_observed"], true);
    assert_eq!(terminal["original_error"], serde_json::to_value(&error)?);
    assert_eq!(terminal["retry"], false);
    Ok(())
}
