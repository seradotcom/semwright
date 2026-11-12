//! Closed Windows Blender 5.2.1 profile. This module has no driver-supplied code or flags.
use super::*;
use semwright_platform_api::launch::{
    HostToolArgRef, SandboxChildControl, encode_host_tool_arg_ref,
};
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    sync::atomic::{AtomicBool, Ordering},
};
use tokio::io::AsyncReadExt;

#[path = "windows_blender52_stage_recovery.rs"]
mod stage_recovery;

struct PrivateTree(stage_recovery::Lease, AtomicBool);
impl Drop for PrivateTree {
    fn drop(&mut self) {
        if self.1.load(Ordering::Acquire) {
            self.0.retain_unconfirmed_exit();
            eprintln!(
                "{}",
                serde_json::json!({"component":"driver-host","phase":"native_assets_retained_unconfirmed_exit"})
            );
        }
    }
}
pub(super) struct Prepared {
    root: PrivateTree,
    workspace: Mount,
    runtime: Mount,
    code: Mount,
    executable_sha256: String,
    runtime_inventory_sha256: String,
    bootstrap_sha256: String,
}
fn new_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn disjoint(a: &Path, b: &Path) -> Result<bool> {
    Ok(!semwright_platform_services::windows_native_paths_overlap(
        a, b,
    )?)
}

pub(super) fn prepare(
    state: &Path,
    tool: &SealedTool,
    workspace: Mount,
    runtime: Mount,
    cancel: Arc<AtomicBool>,
) -> Result<Arc<Prepared>> {
    let preparation_started = Instant::now();
    eprintln!(
        "{}",
        serde_json::json!({"component":"driver-host",
        "phase":"native_blender_profile_preparing","profile":"blender_5_2_1_initial",
        "owner_granted":true,"staging_deadline_seconds":180})
    );
    if workspace.read_only
        || workspace.execute
        || !runtime.read_only
        || !runtime.execute
        || workspace.class != MountClass::Workspace
        || runtime.class != MountClass::Workspace
        || tool.name != "blender"
        || tool.source != runtime.source.join("blender.exe")
        || !disjoint(&workspace.source, &runtime.source)?
        || !disjoint(&workspace.source, state)?
    {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Native Blender requires exact disjoint owner grants",
        ));
    }
    let root = PrivateTree(stage_recovery::new_stage(state)?, AtomicBool::new(false));
    let snapshot = root.0.path().join("runtime");
    let inventory = semwright_platform_services::stage_blender52_runtime(
        &runtime.source,
        &snapshot,
        &tool.sha256,
        &cancel,
    )?;
    let runtime_receipt = serde_json::to_vec(&inventory)?;
    new_file(
        &root.0.path().join("runtime-inventory.json"),
        &runtime_receipt,
    )?;
    let code = root.0.path().join("code");
    std::fs::create_dir(&code)?;
    std::fs::create_dir(code.join("semwright_blender_runtime"))?;
    // Canonical first-party sources are compiled into the Host; no workspace script is trusted.
    let sources = [
        (
            "bootstrap.py",
            include_str!("windows_blender52_bootstrap.py"),
        ),
        (
            "bridge.py",
            include_str!("windows_blender52_runtime/bridge.py"),
        ),
        ("semwright_blender_runtime/__init__.py", ""),
        (
            "semwright_blender_runtime/commands.py",
            include_str!("windows_blender52_runtime/commands.py"),
        ),
        (
            "semwright_blender_runtime/commands.json",
            include_str!("windows_blender52_runtime/commands.json"),
        ),
        (
            "semwright_blender_runtime/validation.py",
            include_str!("windows_blender52_runtime/validation.py"),
        ),
        (
            "semwright_blender_runtime/semantic.py",
            include_str!("windows_blender52_runtime/semantic.py"),
        ),
        (
            "semwright_blender_runtime/authoring_native.py",
            include_str!("windows_blender52_runtime/authoring_native.py"),
        ),
        (
            "semwright_blender_runtime/export_scope.py",
            include_str!("windows_blender52_runtime/export_scope.py"),
        ),
    ];
    let mut pins = BTreeMap::new();
    for (name, source) in sources {
        new_file(&code.join(name), source.as_bytes())?;
        pins.insert(name, hex::encode(Sha256::digest(source.as_bytes())));
    }
    let encoded = serde_json::to_vec(&pins)?;
    new_file(&root.0.path().join("bootstrap-inventory.json"), &encoded)?;
    // Only a completely prepared, source-bound stage is eligible for future
    // crash reconciliation. The lease remains held throughout native custody.
    root.0.publish(&runtime_receipt, &encoded, &tool.sha256)?;
    let prepared = Prepared {
        root,
        workspace,
        runtime: Mount {
            source: snapshot,
            ..runtime
        },
        code: Mount {
            source: code,
            class: MountClass::Workspace,
            logical_name: "native-bootstrap".into(),
            read_only: true,
            execute: false,
        },
        executable_sha256: tool.sha256.clone(),
        runtime_inventory_sha256: inventory["inventory_sha256"]
            .as_str()
            .ok_or_else(|| Error::invalid("Native runtime receipt"))?
            .into(),
        bootstrap_sha256: hex::encode(Sha256::digest(&encoded)),
    };
    eprintln!(
        "{}",
        serde_json::json!({"component":"driver-host","phase":"native_blender_profile_prepared",
        "profile":"blender_5_2_1_initial","owner_granted":true,
        "executable_sha256":prepared.executable_sha256,
        "runtime_inventory_sha256":prepared.runtime_inventory_sha256,
        "runtime_bytes":inventory["bytes"],"runtime_entries":inventory["entries"],
        "preparation_ms":preparation_started.elapsed().as_millis(),
        "transient_snapshot_no_crash_durability_claim":true,
        "bootstrap_inventory_sha256":prepared.bootstrap_sha256,
        "network":false,"process_limit":1,"requested_capabilities":["registryRead"]})
    );
    Ok(Arc::new(prepared))
}
fn mount_arg(name: &str) -> Result<String> {
    encode_host_tool_arg_ref(&HostToolArgRef::MountPath {
        mount: name.into(),
        relative: String::new(),
    })
}
async fn terminate_owned(process: &mut SandboxProcess, phase: &str, nonce: &str) -> bool {
    let pid = process.id();
    let killed = tokio::time::timeout(Duration::from_secs(5), process.kill()).await;
    let waited = tokio::time::timeout(Duration::from_secs(5), process.wait_exit_code()).await;
    eprintln!(
        "{}",
        serde_json::json!({"component":"driver-host","phase":phase,"pid":pid,"nonce":nonce,
        "kill_completed":matches!(killed,Ok(Ok(()))),
        "exit_observed":matches!(waited,Ok(Ok(Some(_))))})
    );
    matches!(waited, Ok(Ok(Some(_))))
}
struct HeldNativeProcess {
    // Original platform process, Job and launcher guard are retained without replacement.
    process: SandboxProcess,
    _prepared: Arc<Prepared>,
    exit: Option<i32>,
}
#[async_trait::async_trait]
impl SandboxChildControl for HeldNativeProcess {
    fn id(&self) -> Option<u32> {
        self.process.id()
    }
    async fn kill(&mut self) -> Result<()> {
        if self.exit.is_none() {
            self.process.kill().await?;
            self.exit = self.process.wait_exit_code().await?;
        }
        Ok(())
    }
    async fn wait(&mut self) -> Result<()> {
        if self.exit.is_none() {
            self.exit = self.process.wait_exit_code().await?;
        }
        Ok(())
    }
    fn exit_code(&self) -> Option<i32> {
        self.exit
    }
}
impl Drop for HeldNativeProcess {
    fn drop(&mut self) {
        if self.exit.is_none() {
            self._prepared.root.1.store(true, Ordering::Release);
        }
    }
}
pub(super) async fn start(
    prepared: Arc<Prepared>,
    mut spec: SandboxSpec,
    cancellation: CancellationToken,
) -> Result<SandboxProcess> {
    if cancellation.is_cancelled() {
        return Err(Error::new(
            ErrorCode::Cancelled,
            "Native session parent cancelled before launch",
        ));
    }
    let _retained_private_tree = &prepared.root;
    let nonce = unique_id().replace('-', "");
    let marker = format!("\0SEMWRIGHT-BLENDER-STDIO-v1:{nonce}\0").into_bytes();
    spec.staged_executable = prepared.runtime.source.join("blender.exe");
    spec.mounts = vec![
        prepared.workspace.clone(),
        prepared.runtime.clone(),
        prepared.code.clone(),
    ];
    spec.network = false;
    spec.nvidia_gpu = false;
    spec.sealed_tools.clear();
    let limits = spec
        .limits
        .as_mut()
        .ok_or_else(|| Error::invalid("Native Blender requires finite owner limits"))?;
    limits.processes = 1;
    spec.environment = vec![
        (
            semwright_platform_api::launch::SANDBOX_HOST_TOOL_CHILD_ENV.into(),
            "1".into(),
        ),
        (SANDBOX_HOST_TOOL_TYPED_ARGS_ENV.into(), "1".into()),
        (
            SANDBOX_HOST_TOOL_CWD_ENV.into(),
            prepared.code.source.to_string_lossy().into_owned(),
        ),
    ];
    spec.args = vec![
        "--background".into(),
        "--factory-startup".into(),
        "--disable-autoexec".into(),
        "--offline-mode".into(),
        "-q".into(),
        "--python-exit-code".into(),
        "1".into(),
        "--python".into(),
        "bootstrap.py".into(),
        "--".into(),
        format!("stdio:{nonce}"),
        mount_arg("workspace")?,
        mount_arg("native-bootstrap")?,
        mount_arg("blender-runtime")?,
    ];
    // Mount ACL propagation and executable verification are blocking filesystem
    // work. Keep the protocol reader and parent cancellation runnable, and retain
    // the private tree until the launch finishes even if the caller has ended.
    let launch_started = Instant::now();
    let launch_prepared = prepared.clone();
    let launch_cancellation = cancellation.clone();
    let mut process = tokio::task::spawn_blocking(move || {
        if launch_cancellation.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Native session parent cancelled before platform launch",
            ));
        }
        semwright_platform_services::sandbox_spawn_blender52_initial(
            &spec,
            &launch_prepared.runtime.source,
            &launch_prepared.executable_sha256,
        )
    })
    .await
    .map_err(|_| Error::unavailable("Native Blender launch task did not complete"))??;
    eprintln!(
        "{}",
        serde_json::json!({"component":"driver-host","phase":"native_launch_finished",
        "pid":process.id(),"launch_ms":launch_started.elapsed().as_millis(),
        "parent_cancelled":cancellation.is_cancelled()})
    );
    let input = match process.take_stdin() {
        Ok(stream) => stream,
        Err(error) => {
            if !terminate_owned(&mut process, "native_stdin_failure", &nonce).await {
                prepared.root.1.store(true, Ordering::Release);
            }
            return Err(error);
        }
    };
    let mut output = match process.take_stdout() {
        Ok(stream) => stream,
        Err(error) => {
            if !terminate_owned(&mut process, "native_stdout_failure", &nonce).await {
                prepared.root.1.store(true, Ordering::Release);
            }
            return Err(error);
        }
    };
    let readiness_started = Instant::now();
    eprintln!(
        "{}",
        serde_json::json!({"component":"driver-host",
        "phase":"native_readiness_wait_started","pid":process.id(),"nonce":nonce,
        "cancelled_at_entry":cancellation.is_cancelled()})
    );
    let readiness = async {
        let mut prefix = Vec::new();
        for _ in 0..65_536 + marker.len() {
            prefix.push(output.read_u8().await?);
            if prefix.ends_with(&marker) {
                return Ok::<usize, std::io::Error>(prefix.len());
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Native readiness exceeds bounds",
        ))
    };
    let observed: Result<usize> = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(Error::new(
            ErrorCode::Cancelled, "Native Blender parent cancelled during readiness")),
        value = tokio::time::timeout(Duration::from_secs(20), readiness) => match value {
            Ok(Ok(length)) => Ok(length),
            Ok(Err(_)) => Err(Error::unavailable("Native Blender readiness stream failed")),
            Err(_) => Err(Error::new(ErrorCode::Timeout, "Native Blender readiness timed out")),
        },
    };
    let observed = observed.and_then(|length| {
        if cancellation.is_cancelled() {
            Err(Error::new(
                ErrorCode::Cancelled,
                "Native parent cancelled before publication",
            ))
        } else {
            Ok(length)
        }
    });
    let prefix_bytes = match observed {
        Ok(length) => length,
        Err(error) => {
            eprintln!(
                "{}",
                serde_json::json!({"component":"driver-host",
                "phase":"native_readiness_terminal","pid":process.id(),"nonce":nonce,
                "reason":error.code,"wait_ms":readiness_started.elapsed().as_millis(),
                "parent_cancelled":cancellation.is_cancelled()})
            );
            if !terminate_owned(&mut process, "native_readiness_failed", &nonce).await {
                prepared.root.1.store(true, Ordering::Release);
            }
            return Err(error);
        }
    };
    eprintln!(
        "{}",
        serde_json::json!({"component":"driver-host","phase":"native_blender_ready",
        "profile":"blender_5_2_1_initial","nonce":nonce,"prefix_bytes":prefix_bytes,
        "pid":process.id(),"bootstrap_inventory_sha256":prepared.bootstrap_sha256,
        "runtime_inventory_sha256":prepared.runtime_inventory_sha256})
    );
    let accounting = process.cpu_accounting();
    let control = Box::new(HeldNativeProcess {
        process,
        _prepared: prepared,
        exit: None,
    });
    Ok(match accounting {
        Some(accounting) => {
            SandboxProcess::from_parts_with_cpu_accounting(input, output, control, accounting)
        }
        None => SandboxProcess::from_parts(input, output, control),
    })
}

#[cfg(test)]
mod pinned_blender_assets_tests {
    use super::*;

    #[test]
    fn canonical_first_party_native_bootstrap_matches_pinned_manifest() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("windows_blender52_runtime/manifest.json")).unwrap();
        let sources: [(&str, &[u8]); 7] = [
            (
                "bridge.py",
                include_bytes!("windows_blender52_runtime/bridge.py"),
            ),
            (
                "commands.py",
                include_bytes!("windows_blender52_runtime/commands.py"),
            ),
            (
                "commands.json",
                include_bytes!("windows_blender52_runtime/commands.json"),
            ),
            (
                "validation.py",
                include_bytes!("windows_blender52_runtime/validation.py"),
            ),
            (
                "semantic.py",
                include_bytes!("windows_blender52_runtime/semantic.py"),
            ),
            (
                "authoring_native.py",
                include_bytes!("windows_blender52_runtime/authoring_native.py"),
            ),
            (
                "export_scope.py",
                include_bytes!("windows_blender52_runtime/export_scope.py"),
            ),
        ];
        assert_eq!(manifest["files"].as_array().unwrap().len(), sources.len());
        for (name, source) in sources {
            let descriptor = manifest["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|candidate| candidate["file"] == name)
                .unwrap();
            assert_eq!(
                descriptor["bytes"].as_u64().unwrap(),
                source.len() as u64,
                "{name}"
            );
            assert_eq!(
                descriptor["sha256"],
                hex::encode(Sha256::digest(source)),
                "{name}"
            );
        }
        let bootstrap = include_bytes!("windows_blender52_bootstrap.py");
        assert_eq!(
            manifest["host_local_bootstrap"]["sha256"],
            hex::encode(Sha256::digest(bootstrap))
        );
        assert_eq!(
            manifest["host_local_bootstrap"]["bytes"],
            bootstrap.len() as u64
        );
        assert_eq!(
            manifest["empty_package"]["sha256"],
            hex::encode(Sha256::digest(b""))
        );
    }

    #[test]
    fn native_snapshot_denies_unbound_executable_before_writing_any_state() {
        let base = tempfile::tempdir().unwrap();
        let workspace = base.path().join("workspace");
        let runtime = base.path().join("runtime");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&runtime).unwrap();
        let workspace = Mount {
            source: workspace,
            class: MountClass::Workspace,
            logical_name: "workspace".into(),
            read_only: false,
            execute: false,
        };
        let runtime = Mount {
            source: runtime,
            class: MountClass::Workspace,
            logical_name: "blender-runtime".into(),
            read_only: true,
            execute: true,
        };
        let tool = SealedTool {
            name: "blender".into(),
            source: base.path().join("different-file.exe"),
            staged: None,
            sha256: "0".repeat(64),
        };
        assert_eq!(
            tool.staged_path().unwrap_err().code,
            ErrorCode::PolicyDenied
        );
        let state = base.path().join("state");
        let attempt = prepare(
            &state,
            &tool,
            workspace,
            runtime,
            Arc::new(AtomicBool::new(false)),
        );
        assert!(attempt.is_err(), "Native snapshot accepted unbound tool");
        assert!(
            !state.exists(),
            "Rejected native tool created private state"
        );
    }
}
