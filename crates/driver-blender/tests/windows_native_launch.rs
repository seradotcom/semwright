#![cfg(all(target_os = "windows", feature = "windows-launch-test-fixture"))]
use semwright_platform_api::launch::{
    HostToolArgRef, Mount, MountClass, ResourceLimits, SANDBOX_HOST_TOOL_CHILD_ENV,
    SANDBOX_HOST_TOOL_CWD_ENV, SANDBOX_HOST_TOOL_TYPED_ARGS_ENV, SandboxKind, SandboxSpec,
    SealedToolMount, SealedToolSource, encode_host_tool_arg_ref,
};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
fn harden(path: &Path) {
    let principal = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap(),
        std::env::var("USERNAME").unwrap()
    );
    let status = std::process::Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{principal}:(F)"))
        .status()
        .unwrap();
    assert!(status.success());
}
#[test]
fn launch_fixture_refuses_an_owner_token() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_semwright-blender-launch-fixture"))
        .arg("--child-probe")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
#[tokio::test]
async fn exact_native_launcher_creates_only_a_same_lpac_child() {
    run_case("--launch-probe").await;
}
#[tokio::test]
async fn no_stdio_diagnostic_preserves_the_same_lpac_identity() {
    run_case("--launch-no-stdio-probe").await;
}
async fn run_case(mode: &str) {
    let binaries = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let parent = binaries.path().join("parent.exe");
    let dependency = binaries.path().join("child.exe");
    for target in [&parent, &dependency] {
        std::fs::copy(
            env!("CARGO_BIN_EXE_semwright-blender-launch-fixture"),
            target,
        )
        .unwrap();
        harden(target);
    }
    let digest = hex::encode(Sha256::digest(std::fs::read(&dependency).unwrap()));
    let spec = SandboxSpec {
        kind: SandboxKind::Driver,
        staged_executable: parent,
        helper: std::env::current_exe().unwrap(),
        mounts: vec![Mount {
            source: workspace.path().to_path_buf(),
            class: MountClass::Workspace,
            logical_name: "work".into(),
            read_only: false,
            execute: false,
        }],
        args: vec![
            mode.into(),
            encode_host_tool_arg_ref(&HostToolArgRef::ToolPath {
                tool: "fixture-child".into(),
            })
            .unwrap(),
            encode_host_tool_arg_ref(&HostToolArgRef::MountPath {
                mount: "work".into(),
                relative: String::new(),
            })
            .unwrap(),
        ],
        environment: vec![
            (SANDBOX_HOST_TOOL_CHILD_ENV.into(), "1".into()),
            (SANDBOX_HOST_TOOL_TYPED_ARGS_ENV.into(), "1".into()),
            (
                SANDBOX_HOST_TOOL_CWD_ENV.into(),
                workspace.path().to_string_lossy().into_owned(),
            ),
        ],
        sealed_tools: vec![SealedToolMount {
            name: "fixture-child".into(),
            source: SealedToolSource::VerifiedFile {
                path: dependency,
                sha256: digest,
            },
        }],
        network: false,
        nvidia_gpu: false,
        limits: Some(ResourceLimits {
            open_files: 32,
            processes: 8,
            cpu_seconds: 5,
            address_space_bytes: 256 * 1024 * 1024,
            file_size_bytes: 1024 * 1024,
        }),
    };
    let mut process =
        semwright_platform_services::sandbox_spawn(&spec).expect("isolated fixture spawn");
    let mut input = process.take_stdin().unwrap();
    input.shutdown().await.unwrap();
    drop(input);
    let mut output = process.take_stdout().unwrap().take(32769);
    let mut bytes = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(10), output.read_to_end(&mut bytes)).await;
    if !matches!(read, Ok(Ok(_))) {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    assert!(matches!(read, Ok(Ok(_))), "bounded fixture response");
    assert!(bytes.len() <= 32768);
    let waited = tokio::time::timeout(Duration::from_secs(5), process.wait_exit_code()).await;
    if waited.is_err() {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    let exit = waited
        .expect("fixture bounded exit")
        .expect("fixture exit query");
    let text = String::from_utf8(bytes).unwrap();
    eprintln!("isolated fixture result: {text}");
    let row: serde_json::Value =
        serde_json::from_str(text.trim()).expect("structured fixture result");
    assert_eq!(row["native_blender"], false);
    assert_eq!(
        exit,
        Some(0),
        "fixture must execute under the same confinement"
    );
    assert_eq!(row["ok"], true);
}

#[tokio::test]
#[ignore = "Requires the explicitly pinned private Blender 5.2.1 runtime on the Windows test host"]
async fn direct_host_isolated_blender_version_probe() {
    let runtime = std::path::PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_BLENDER_RUNTIME").expect("explicit runtime"),
    );
    let executable = runtime.join("blender.exe");
    let expected = "284f4041f98e113f3dc10654a7193ffaaa9bfdfec8b87fa116620a48b5f6d4cb";
    let bytes = semwright_platform_services::verify_sealed_tool_executable(&executable, expected)
        .expect("owner pinned application image");
    assert_eq!(hex::encode(Sha256::digest(&bytes)), expected);
    drop(bytes);
    let spec = SandboxSpec {
        kind: SandboxKind::Driver,
        staged_executable: executable,
        helper: std::env::current_exe().unwrap(),
        mounts: vec![Mount {
            source: runtime.clone(),
            class: MountClass::Workspace,
            logical_name: "runtime".into(),
            read_only: true,
            execute: true,
        }],
        args: vec!["--version".into()],
        environment: vec![
            (SANDBOX_HOST_TOOL_CHILD_ENV.into(), "1".into()),
            (SANDBOX_HOST_TOOL_TYPED_ARGS_ENV.into(), "1".into()),
            (
                SANDBOX_HOST_TOOL_CWD_ENV.into(),
                runtime.to_string_lossy().into_owned(),
            ),
        ],
        sealed_tools: vec![],
        network: false,
        nvidia_gpu: false,
        limits: Some(ResourceLimits {
            open_files: 256,
            processes: 8,
            cpu_seconds: 30,
            address_space_bytes: 4 * 1024 * 1024 * 1024,
            file_size_bytes: 1024 * 1024,
        }),
    };
    let mut process =
        semwright_platform_services::sandbox_spawn(&spec).expect("direct contained Blender");
    let mut input = process.take_stdin().unwrap();
    input.shutdown().await.unwrap();
    drop(input);
    let mut output = process.take_stdout().unwrap().take(16385);
    let mut bytes = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(30), output.read_to_end(&mut bytes)).await;
    if !matches!(read, Ok(Ok(_))) {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    assert!(matches!(read, Ok(Ok(_))), "bounded native version response");
    assert!(bytes.len() <= 16384);
    let exit = process.wait_exit_code().await.expect("native exit");
    let text = String::from_utf8(bytes).unwrap();
    eprintln!(
        "direct Host Blender version exit={exit:?}, broker_used=false, scene_operation=false:\n{text}"
    );
    assert_eq!(exit, Some(0));
    assert!(text.starts_with("Blender 5.2.1"));
}

#[tokio::test]
#[ignore = "Requires exact owner-pinned Blender runtime; read-only SxS resolution, no Blender execution"]
async fn isolated_blender_activation_context_probe() {
    let runtime = std::path::PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_BLENDER_RUNTIME").expect("explicit runtime"),
    );
    let binaries = tempfile::tempdir().unwrap();
    let parent = binaries.path().join("actctx-probe.exe");
    std::fs::copy(
        env!("CARGO_BIN_EXE_semwright-blender-launch-fixture"),
        &parent,
    )
    .unwrap();
    harden(&parent);
    let fixture = tempfile::tempdir().unwrap();
    let minimal = fixture.path().join("minimal.manifest");
    let fixed=br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<assemblyIdentity type="win32" name="Semwright.ReadOnly.ActCtxProbe" version="1.0.0.0" processorArchitecture="amd64"/>
</assembly>
"#;
    std::fs::write(&minimal, fixed).unwrap();
    let expected = hex::encode(Sha256::digest(fixed));
    owner_minimal_context(&minimal, None);
    owner_minimal_context(&minimal, Some(0x0409));
    eprintln!("owner minimal context: ok=true, sha256={expected}, broker_used=false");
    for relative in [
        "blender.shared/blender.shared.manifest",
        "blender.crt/blender.crt.manifest",
    ] {
        let bytes = std::fs::read(runtime.join(relative)).unwrap();
        eprintln!(
            "owner private manifest: {relative}, bytes={}, sha256={}",
            bytes.len(),
            hex::encode(Sha256::digest(&bytes))
        );
    }
    let spec = SandboxSpec {
        kind: SandboxKind::Driver,
        staged_executable: parent,
        helper: std::env::current_exe().unwrap(),
        mounts: vec![
            Mount {
                source: runtime.clone(),
                class: MountClass::Workspace,
                logical_name: "runtime".into(),
                read_only: true,
                execute: true,
            },
            Mount {
                source: fixture.path().to_path_buf(),
                class: MountClass::Workspace,
                logical_name: "actctx-probe".into(),
                read_only: true,
                execute: false,
            },
        ],
        args: vec![
            "--activation-context-probe".into(),
            encode_host_tool_arg_ref(&HostToolArgRef::MountPath {
                mount: "runtime".into(),
                relative: String::new(),
            })
            .unwrap(),
            encode_host_tool_arg_ref(&HostToolArgRef::MountPath {
                mount: "actctx-probe".into(),
                relative: String::new(),
            })
            .unwrap(),
        ],
        environment: vec![
            (SANDBOX_HOST_TOOL_CHILD_ENV.into(), "1".into()),
            (SANDBOX_HOST_TOOL_TYPED_ARGS_ENV.into(), "1".into()),
            (
                SANDBOX_HOST_TOOL_CWD_ENV.into(),
                runtime.to_string_lossy().into_owned(),
            ),
        ],
        sealed_tools: vec![],
        network: false,
        nvidia_gpu: false,
        limits: Some(ResourceLimits {
            open_files: 128,
            processes: 8,
            cpu_seconds: 15,
            address_space_bytes: 1024 * 1024 * 1024,
            file_size_bytes: 1024 * 1024,
        }),
    };
    let mut process = semwright_platform_services::sandbox_spawn(&spec)
        .expect("isolated read-only activation probe");
    let mut input = process.take_stdin().unwrap();
    input.shutdown().await.unwrap();
    drop(input);
    let mut output = process.take_stdout().unwrap().take(8193);
    let mut bytes = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(25), output.read_to_end(&mut bytes)).await;
    if !matches!(read, Ok(Ok(_))) {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    assert!(matches!(read, Ok(Ok(_))), "bounded activation response");
    assert!(bytes.len() <= 8192);
    let waited = tokio::time::timeout(Duration::from_secs(5), process.wait_exit_code()).await;
    if waited.is_err() {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    let exit = waited
        .expect("bounded fixture exit")
        .expect("fixture exit query");
    let text = String::from_utf8(bytes).unwrap();
    eprintln!("isolated activation probe: {text}");
    let row: serde_json::Value =
        serde_json::from_str(text.trim()).expect("structured activation result");
    assert_eq!(row["native_blender"], false);
    assert_eq!(exit, Some(0));
    assert_eq!(row["ok"], true);
}

fn owner_minimal_context(path: &Path, language: Option<u16>) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::ApplicationInstallationAndServicing::{ACTCTXW, CreateActCtxW, ReleaseActCtx},
    };
    let source: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut context: ACTCTXW = unsafe { std::mem::zeroed() };
    context.cbSize = std::mem::size_of::<ACTCTXW>() as u32;
    context.lpSource = source.as_ptr();
    if let Some(language) = language {
        context.dwFlags |= windows_sys::Win32::System::WindowsProgramming::ACTCTX_FLAG_LANGID_VALID;
        context.wLangId = language;
    }
    let handle = unsafe { CreateActCtxW(&context) };
    let error = if handle == INVALID_HANDLE_VALUE {
        Some(std::io::Error::last_os_error())
    } else {
        None
    };
    if error.is_none() {
        unsafe { ReleaseActCtx(handle) };
    }
    eprintln!("owner context language={language:?} ok={}", error.is_none());
    assert!(
        error.is_none(),
        "Owner minimal activation fixture invalid: {error:?}"
    );
}

#[path = "../src/bin/session_runner/windows_activation_readback.rs"]
mod activation_readback;
#[path = "../src/bin/session_runner/windows_cpu_images.rs"]
mod cpu_images;
#[path = "../src/bin/session_runner/windows_cpu_payload.rs"]
mod cpu_payload;
#[path = "../src/bin/session_runner/windows_cpu_resource_activation.rs"]
mod cpu_resource_activation;

#[tokio::test]
async fn initial_host_embedded_manifest_activation_probe() {
    run_initial_manifest_case(false, false, false, false, false, false).await;
}
#[tokio::test]
async fn initial_host_common_controls_probe() {
    run_initial_manifest_case(true, false, false, false, false, false).await;
}
#[tokio::test]
#[ignore = "Requires exact owner-pinned Blender manifests; no Blender DLL execution"]
async fn initial_host_private_assemblies_probe() {
    run_initial_manifest_case(true, true, false, false, false, false).await;
}
#[tokio::test]
#[ignore = "Requires exact pinned Blender manifests; no Blender DLL execution"]
async fn initial_host_exact_blender_manifest_probe() {
    run_initial_manifest_case(true, true, true, false, false, false).await;
}
#[tokio::test]
#[ignore = "Requires exact pinned DLL payload; executes CPU component code inside LPAC only"]
async fn initial_host_cpu_component_probe() {
    run_initial_manifest_case(true, true, true, true, false, false).await;
}
#[tokio::test]
#[ignore = "Requires pinned CPU resource; query-only LPAC first, owner control afterward"]
async fn initial_host_cpu_resource_activation_probe() {
    run_initial_manifest_case(true, true, true, false, true, false).await;
}
#[tokio::test]
#[ignore = "Explicit own-fixture debugger rendezvous; preserves LPAC/Job/zero capabilities"]
async fn initial_host_cpu_resource_debug_probe() {
    run_initial_manifest_case(true, true, true, false, true, true).await;
}
#[test]
#[ignore = "Owner-only policy/resource comparison; not isolated Blender acceptance"]
fn owner_cpu_resource_policy_probe() {
    use std::{
        io::Write,
        path::PathBuf,
        time::{Instant, SystemTime, UNIX_EPOCH},
    };
    let started = Instant::now();
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();
    // SAFETY: query of this test-harness thread's identity only.
    let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    let runtime: Result<PathBuf, String> = std::env::var_os("SEMWRIGHT_TEST_BLENDER_RUNTIME")
        .map(PathBuf::from)
        .ok_or_else(|| "SEMWRIGHT_TEST_BLENDER_RUNTIME is required".to_owned())
        .and_then(|path| {
            if !path.is_absolute() || !path.is_dir() {
                Err("Runtime must be an existing absolute owner-approved directory".to_owned())
            } else {
                Ok(path)
            }
        });
    // Reads the single pinned CPU image; no explicit mapping or loading.
    let cpu_digest = |root: &Path| -> Result<String, String> {
        let entries = cpu_payload::entries().map_err(|e| e.to_string())?;
        let entry = entries
            .iter()
            .find(|entry| entry.relative == cpu_payload::CPU_DLL)
            .ok_or_else(|| "Pinned CPU entry missing".to_owned())?;
        let bytes = cpu_payload::read_checked(root, entry).map_err(|e| e.to_string())?;
        Ok(hex::encode(Sha256::digest(&bytes)))
    };
    let digest_before = runtime
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|root| cpu_digest(root));
    let policies_before = cpu_images::policies();
    // Same helper as the historical owner_cpu_after, in the current harness.
    // The authorized runtime replaces the old, already removed TempDir path.
    let resource_observation = match (&runtime, &digest_before) {
        (Ok(root), Ok(_)) => Some(cpu_resource_activation::probe(root)),
        _ => None,
    };
    let policies_after = cpu_images::policies();
    let digest_after = runtime
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|root| cpu_digest(root));
    let names = [
        "binary_signature",
        "dynamic_code",
        "extension_points",
        "system_call_disable",
        "image_load",
    ];
    let complete = |value: &serde_json::Value| {
        value.as_object().is_some_and(|rows| {
            rows.len() == names.len()
                && names.iter().all(|name| {
                    rows.get(*name).is_some_and(|row| {
                        row["query_ok"] == true && row["flags"].as_u64().is_some()
                    })
                })
        })
    };
    let queries_ok = complete(&policies_before) && complete(&policies_after);
    let policies_stable = queries_ok && policies_before == policies_after;
    let runtime_unchanged = matches!(
        (&digest_before, &digest_after),
        (Ok(before), Ok(after))
            if before == after && before == cpu_payload::CPU_SHA256
    );
    let resource_ok = resource_observation
        .as_ref()
        .is_some_and(|row| row["ok"] == true);
    let diagnostic_ok = queries_ok && policies_stable && runtime_unchanged && resource_ok;
    // Source bytes embedded in the actual compiled test harness.
    let mut compiled_sources = serde_json::Map::new();
    for (name, bytes) in [
        (
            "tests/windows_native_launch.rs",
            include_bytes!("windows_native_launch.rs").as_slice(),
        ),
        (
            "src/bin/session_runner/windows_cpu_images.rs",
            include_bytes!("../src/bin/session_runner/windows_cpu_images.rs").as_slice(),
        ),
        (
            "src/bin/session_runner/windows_cpu_resource_activation.rs",
            include_bytes!("../src/bin/session_runner/windows_cpu_resource_activation.rs")
                .as_slice(),
        ),
        (
            "src/bin/session_runner/windows_cpu_payload.rs",
            include_bytes!("../src/bin/session_runner/windows_cpu_payload.rs").as_slice(),
        ),
        (
            "src/bin/session_runner/windows_activation_readback.rs",
            include_bytes!("../src/bin/session_runner/windows_activation_readback.rs").as_slice(),
        ),
        (
            "tests/fixtures/windows_cpu_probe_payload.json",
            include_bytes!("fixtures/windows_cpu_probe_payload.json").as_slice(),
        ),
        (
            "Cargo.lock",
            include_bytes!("../../../Cargo.lock").as_slice(),
        ),
    ] {
        compiled_sources.insert(
            name.to_owned(),
            serde_json::Value::String(hex::encode(Sha256::digest(bytes))),
        );
    }
    let receipt_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/logs");
    let receipt_path = receipt_directory.join(format!(
        "windows-owner-cpu-resource-policy-{timestamp_ms}-{pid}-{tid}.json"
    ));
    let mut receipt_fields = serde_json::Map::new();
    for part in [
        serde_json::json!({
            "schema_version": 1,
            "probe": "owner_only_cpu_resource_policy_comparison",
            "diagnostic_ok": diagnostic_ok,
            "process_id": pid,
            "thread_id": tid,
            "unix_timestamp_ms": timestamp_ms,
            "elapsed_ms": started.elapsed().as_millis(),
            "harness_executable": std::env::current_exe().ok()
                .map(|p| p.to_string_lossy().into_owned()),
            "owner_control_in_current_harness": true,
            "extra_process_for_getters": false,
            "child_processes_spawned_by_test": 0,
            "new_lpac_processes": 0,
            "sandbox_spawn_called": false,
            "broker_used": false,
            "policies_before": policies_before,
            "policies_after": policies_after,
            "all_five_queries_succeeded": queries_ok,
            "policies_stable": policies_stable
        }),
        serde_json::json!({
            "runtime_root": runtime.as_ref().ok().map(|p| p.to_string_lossy().into_owned()),
            "runtime_preflight_error": runtime.as_ref().err(),
            "runtime_location_scope":
                "authorized pinned runtime; not the previous temporary fixture directory",
            "runtime_cpu_relative": cpu_payload::CPU_DLL,
            "runtime_cpu_expected_sha256": cpu_payload::CPU_SHA256,
            "runtime_cpu_sha256_before": digest_before.as_ref().ok(),
            "runtime_cpu_sha256_after": digest_after.as_ref().ok(),
            "runtime_cpu_hash_error_before": digest_before.as_ref().err(),
            "runtime_cpu_hash_error_after": digest_after.as_ref().err(),
            "runtime_cpu_unchanged": runtime_unchanged,
            "cpu_resource_probe_calls": usize::from(resource_observation.is_some()),
            "cpu_resource_activation": resource_observation,
            "explicit_image_mapping_requested": false,
            "dll_initialization_requested": false,
            "thread_activation_requested": false,
            "exports_called": false,
            "policy_changed": false,
            "token_changed": false,
            "runtime_file_data_written": false,
            "compiled_source_sha256": compiled_sources
        }),
        serde_json::json!({
            "historical_comparison": {
                "checkpoint": "8b6269223f2fb7e13dd1f8af3d41636a1531ffc7",
                "lpac_log":
                    "crates/driver-blender/evidence/windows-activation-20261028/windows-initial-cpu-closure-order-resume.log",
                "lpac_receipt_field": "cpu_component.image_probe.policies",
                "lpac_system_call_disable_flags": 4,
                "same_process_as_current_owner": false,
                "same_run_as_resource2_comparison": false,
                "previous_owner_after_policies_recorded": false,
                "previous_owner_after_flags_inferred": false
            },
            "acceptance_scope": "owner diagnostic only",
            "P0_evaluated": false,
            "A05": "BLOCKED",
            "isolated_blender_support_claim": false,
            "receipt_path": receipt_path.to_string_lossy()
        }),
    ] {
        if let serde_json::Value::Object(fields) = part {
            receipt_fields.extend(fields);
        }
    }
    let receipt = serde_json::Value::Object(receipt_fields);

    // Save every observation before checking the outcome; never overwrite.
    let encoded = receipt.to_string();
    let saved = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&receipt_directory)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&receipt_path)?;
        file.write_all(encoded.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()
    })();
    eprintln!("OWNER_CPU_RESOURCE_POLICY_RECEIPT {encoded}");
    if let Err(error) = &saved {
        eprintln!("OWNER_CPU_RESOURCE_POLICY_RECEIPT_SAVE_ERROR {error}");
    }
    assert!(
        saved.is_ok(),
        "Owner diagnostic receipt could not be preserved"
    );
    assert!(queries_ok, "A policy query failed; receipt preserved");
    assert!(
        runtime_unchanged,
        "Pinned CPU input was not verified unchanged"
    );
    assert!(
        resource_ok,
        "Owner resource control did not pass; receipt preserved"
    );
    assert!(policies_stable, "Policies changed across the owner control");
}

async fn run_initial_manifest_case(
    common_controls: bool,
    private_assemblies: bool,
    full_blender_manifest: bool,
    cpu_component: bool,
    cpu_actctx: bool,
    debug_rendezvous: bool,
) {
    use std::os::windows::ffi::OsStrExt;
    use tokio::io::AsyncBufReadExt;
    assert!(!debug_rendezvous || (cpu_actctx && !cpu_component));
    use windows_sys::Win32::{
        Foundation::{FreeLibrary, GetLastError, INVALID_HANDLE_VALUE},
        System::{
            ApplicationInstallationAndServicing::{ACTCTXW, CreateActCtxW, ReleaseActCtx},
            LibraryLoader::*,
            WindowsProgramming::ACTCTX_FLAG_RESOURCE_NAME_VALID,
        },
    };
    let binaries = tempfile::tempdir().unwrap();
    let cpu_payload_needed = cpu_component || cpu_actctx;
    if cpu_payload_needed {
        harden_cpu_fixture_root(binaries.path());
    }
    let fixture_mounts = if private_assemblies {
        stage_pinned_private_manifests(binaries.path(), cpu_payload_needed);
        if cpu_payload_needed {
            stage_pinned_cpu_payload(binaries.path());
        }
        vec![Mount {
            source: binaries.path().to_path_buf(),
            class: MountClass::Workspace,
            logical_name: "activation-fixture".into(),
            read_only: true,
            execute: true,
        }]
    } else {
        vec![]
    };
    let parent = binaries.path().join("fixture-embedded-manifest.exe");
    std::fs::copy(
        env!("CARGO_BIN_EXE_semwright-blender-launch-fixture"),
        &parent,
    )
    .unwrap();
    let path: Vec<u16> = parent.as_os_str().encode_wide().chain(Some(0)).collect();
    // Only the test copy is edited; production and pinned Blender images are untouched.
    unsafe extern "system" fn language(
        _m: windows_sys::Win32::Foundation::HMODULE,
        _t: *const u16,
        _n: *const u16,
        l: u16,
        p: isize,
    ) -> windows_sys::core::BOOL {
        // SAFETY: synchronous enumeration receives the live Vec passed below.
        let values = unsafe { &mut *(p as *mut Vec<u16>) };
        if values.len() >= 16 {
            return 0;
        }
        values.push(l);
        1
    }
    let mut languages = Vec::<u16>::new();
    let module = unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    };
    assert!(!module.is_null(), "resource-only image inspection");
    let enumerated = unsafe {
        EnumResourceLanguagesW(
            module,
            24usize as *const u16,
            1usize as *const u16,
            Some(language),
            &mut languages as *mut _ as isize,
        )
    };
    let error = if enumerated == 0 {
        unsafe { GetLastError() }
    } else {
        0
    };
    assert_ne!(unsafe { FreeLibrary(module) }, 0);
    assert!(
        enumerated != 0 || [1813, 1814, 1815].contains(&error),
        "manifest enumeration failed: {error}"
    );
    assert!(
        languages.is_empty() || languages == [0],
        "Refuse ambiguous manifest languages: {languages:?}"
    );
    let fixed = if full_blender_manifest {
        activation_readback::fixture_blender_manifest()
    } else if private_assemblies {
        activation_readback::fixture_private_manifest()
    } else {
        activation_readback::fixture_manifest(common_controls)
    };
    if full_blender_manifest {
        assert_eq!(
            hex::encode(Sha256::digest(&fixed)),
            "e7101d6abbe3d11a3ccb0cf88664ceffb2df9e2fbc7d23273edd274ddfa1e1e8"
        );
    }
    let update = unsafe { BeginUpdateResourceW(path.as_ptr(), 0) };
    assert!(!update.is_null());
    let ok = unsafe {
        UpdateResourceW(
            update,
            24usize as *const u16,
            1usize as *const u16,
            0,
            fixed.as_ptr().cast(),
            fixed.len() as u32,
        )
    };
    if ok == 0 {
        let error = std::io::Error::last_os_error();
        unsafe { EndUpdateResourceW(update, 1) };
        panic!("test-copy resource update: {error}");
    }
    assert_ne!(
        unsafe { EndUpdateResourceW(update, 0) },
        0,
        "commit test-copy resource"
    );
    harden(&parent);
    let digest = hex::encode(Sha256::digest(std::fs::read(&parent).unwrap()));
    semwright_platform_services::verify_sealed_tool_executable(&parent, &digest).unwrap();
    let owner_root_readback = || -> std::io::Result<serde_json::Value> {
        // New resource mode calls this only after isolated exit.
        // SAFETY: zeroed SDK POD with live fixed source/resource.
        let mut context: ACTCTXW = unsafe { std::mem::zeroed() };
        context.cbSize = std::mem::size_of::<ACTCTXW>() as u32;
        context.dwFlags = ACTCTX_FLAG_RESOURCE_NAME_VALID;
        context.lpSource = path.as_ptr();
        context.lpResourceName = 1usize as *const u16;
        // SAFETY: parse exact test-copy manifest; no DLL initialization or activation.
        let handle = unsafe { CreateActCtxW(&context) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let result = activation_readback::read_context(0, handle);
        // SAFETY: release created reference even if readback failed.
        unsafe {
            ReleaseActCtx(handle);
        }
        result
    };
    let expected_before = if cpu_actctx {
        None
    } else {
        Some(owner_root_readback().expect("owner embedded context readback"))
    };
    let nonce = &digest[..32];
    eprintln!(
        "initial fixture image_sha256={digest}; owner_context_before={expected_before:?}; broker_used=false"
    );
    let spec = SandboxSpec {
        kind: SandboxKind::Driver,
        staged_executable: parent,
        helper: std::env::current_exe().unwrap(),
        mounts: fixture_mounts,
        args: vec![
            if debug_rendezvous {
                "--embedded-cpu-actctx-debug-probe".into()
            } else if cpu_actctx {
                "--embedded-cpu-actctx-probe".into()
            } else if cpu_component {
                "--embedded-cpu-component-probe".into()
            } else if full_blender_manifest {
                "--embedded-blender-manifest-probe".into()
            } else if private_assemblies {
                "--embedded-private-assemblies-probe".into()
            } else if common_controls {
                "--embedded-common-controls-probe".into()
            } else {
                "--embedded-activation-probe".into()
            },
            nonce.into(),
        ],
        environment: vec![],
        sealed_tools: vec![],
        network: false,
        nvidia_gpu: false,
        limits: Some(ResourceLimits {
            open_files: 32,
            processes: 8,
            cpu_seconds: 5,
            address_space_bytes: 256 * 1024 * 1024,
            file_size_bytes: 1024 * 1024,
        }),
    };
    let mut process =
        semwright_platform_services::sandbox_spawn(&spec).expect("Host initial fixture");
    if cpu_payload_needed {
        print_cpu_mount_acls(binaries.path());
    }
    let mut input = process.take_stdin().unwrap();
    let mut output = tokio::io::BufReader::new(process.take_stdout().unwrap().take(16385));
    if debug_rendezvous {
        let gate = async {
            let mut line = String::new();
            output.read_line(&mut line).await?;
            if line.len() > 4096 {
                return Err(std::io::Error::other("Debugger readiness exceeds bound"));
            }
            let payload = line
                .strip_prefix("LPAC_CPU_DEBUG_READY ")
                .ok_or_else(|| std::io::Error::other("Missing fixed readiness marker"))?;
            let ready: serde_json::Value = serde_json::from_str(payload.trim())?;
            if ready["nonce"] != nonce
                || ready["executable"] != spec.staged_executable.to_string_lossy().as_ref()
            {
                return Err(std::io::Error::other("Debugger fixture identity mismatch"));
            }
            let directory =
                std::path::PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_DEBUG_DIR").ok_or_else(
                    || std::io::Error::other("Explicit private debugger directory required"),
                )?);
            let allowed_parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.local/logs")
                .canonicalize()?;
            let directory = directory.canonicalize()?;
            if directory.parent() != Some(allowed_parent.as_path()) {
                return Err(std::io::Error::other(
                    "Debugger output must be a direct private log child",
                ));
            }
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join("ready.json"))?;
            std::io::Write::write_all(&mut file, ready.to_string().as_bytes())?;
            file.sync_all()?;
            eprintln!("LPAC_DEBUG_RENDEZVOUS {}", directory.display());
            let ack_path = directory.join("debugger-ready.json");
            while !ack_path.exists() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let ack: serde_json::Value = serde_json::from_slice(&std::fs::read(ack_path)?)?;
            if ack["pid"] != ready["pid"]
                || ack["nonce"] != ready["nonce"]
                || ack["breakpoints_ready"] != true
            {
                return Err(std::io::Error::other("Debugger acknowledgment mismatch"));
            }
            input.write_all(b"G").await?;
            Ok::<(), std::io::Error>(())
        };
        let gated = tokio::time::timeout(Duration::from_secs(120), gate).await;
        if !matches!(gated, Ok(Ok(()))) {
            let _ = process.kill().await;
            let _ = process.wait().await;
            panic!("Owned debugger rendezvous failed: {gated:?}");
        }
    }
    input.shutdown().await.unwrap();
    drop(input);
    let mut bytes = Vec::new();
    let read = tokio::time::timeout(
        Duration::from_secs(if debug_rendezvous { 120 } else { 10 }),
        output.read_to_end(&mut bytes),
    )
    .await;
    if !matches!(read, Ok(Ok(_))) {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    assert!(matches!(read, Ok(Ok(_))), "bounded initial fixture");
    assert!(bytes.len() <= 8192);
    let waited = tokio::time::timeout(Duration::from_secs(5), process.wait_exit_code()).await;
    if waited.is_err() {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    let exit = waited.expect("bounded exit").expect("exit query");
    let text = String::from_utf8(bytes).unwrap();
    eprintln!("initial embedded fixture exit={exit:?}: {text}");
    if !cpu_actctx {
        assert_eq!(exit, Some(0));
    }
    let row: serde_json::Value = serde_json::from_str(
        text.lines()
            .rev()
            .find(|line| line.starts_with('{'))
            .expect("structured final receipt"),
    )
    .expect("initial activation receipt");
    // Preserve both controls even after isolated exit 2.
    let owner_cpu_after = if cpu_actctx {
        let observation = cpu_resource_activation::probe(binaries.path());
        eprintln!("owner CPU resource control after isolated exit: {observation}");
        Some(observation)
    } else {
        None
    };
    let expected_result = match expected_before {
        Some(value) => Ok(value),
        None => owner_root_readback(),
    };
    if cpu_actctx {
        let isolated = &row["cpu_resource_activation"];
        let owner = owner_cpu_after.as_ref().unwrap();
        let identities_match = if isolated["query_ok"] == true && owner["query_ok"] == true {
            Some(
                isolated["context"]["assembly_identities"]
                    == owner["context"]["assembly_identities"]
                    && isolated["context"]["root_identity"] == owner["context"]["root_identity"]
                    && isolated["context"]["assembly_count"] == owner["context"]["assembly_count"],
            )
        } else {
            None
        };
        eprintln!(
            "{}",
            serde_json::json!({"probe":"cpu_resource_activation_comparison",
            "isolated_exit_code":exit,"owner_control_after_isolated_exit":true,
            "cold_cache_claim":false,"policy_changed":false,
            "isolated":isolated,"owner_after":owner,
            "owner_initial_context_after":expected_result.as_ref().ok(),
            "owner_initial_context_error":expected_result.as_ref().err().map(ToString::to_string),
            "create_status_matches":isolated["create_ok"]==owner["create_ok"]
                && isolated["create_win32_error"]==owner["create_win32_error"]
                && isolated["create_nt_status"]==owner["create_nt_status"],
            "query_status_matches":isolated["query_ok"]==owner["query_ok"]
                && isolated["query_win32_error"]==owner["query_win32_error"]
                && isolated["query_nt_status"]==owner["query_nt_status"],
            "identity_matches":identities_match,
            "release_call_matches":isolated["release_called"]==owner["release_called"]})
        );
        assert_eq!(
            owner["ok"], true,
            "valid owner control after isolated exit required"
        );
        assert_eq!(isolated["image_verified"], true);
        assert_eq!(isolated["create_ok"], owner["create_ok"]);
        assert_eq!(isolated["create_win32_error"], owner["create_win32_error"]);
        assert_eq!(isolated["create_nt_status"], owner["create_nt_status"]);
        assert_eq!(isolated["query_ok"], owner["query_ok"]);
        assert_eq!(isolated["query_win32_error"], owner["query_win32_error"]);
        assert_eq!(isolated["query_nt_status"], owner["query_nt_status"]);
        assert_eq!(identities_match, Some(true));
        assert_eq!(isolated["release_called"], true);
        assert_eq!(isolated["dll_initialization_requested"], false);
        assert_eq!(isolated["thread_activation_requested"], false);
        assert_eq!(row["blender_component_execution_attempted"], false);
        assert_eq!(row["cpu_component"], serde_json::Value::Null);
        assert_eq!(row["dll"], serde_json::Value::Null);
    }
    let expected = expected_result.expect("owner initial-context readback");
    assert_eq!(exit, Some(0));
    assert_eq!(row["nonce"], nonce);
    assert_eq!(row["verified_lpac"], true);
    assert_eq!(row["ok"], true);
    assert_eq!(row["native_blender"], false);
    assert_eq!(row["broker_used"], false);
    assert_eq!(row["reached_main"], true);
    assert_eq!(row["capability_count"], 0);
    assert_eq!(row["job_verified"], true);
    assert_eq!(row["common_controls"], common_controls);
    assert_eq!(row["private_assemblies"], private_assemblies);
    assert_eq!(row["full_blender_manifest"], full_blender_manifest);
    assert_eq!(row["cpu_component_probe"], cpu_component);
    assert_eq!(row["cpu_actctx_probe"], cpu_actctx);
    assert_eq!(row["blender_executable_started"], false);
    if cpu_component {
        assert_eq!(row["blender_component_execution_attempted"], true);
        let cpu = &row["cpu_component"];
        assert_eq!(cpu["ok"], true);
        assert_eq!(cpu["dll_loaded"], true);
        assert_eq!(cpu["dll_released"], true);
        assert_eq!(cpu["already_loaded"], false);
        assert_eq!(cpu["exports_called"], false);
        assert_eq!(cpu["verified_payload_files"], 51);
        assert_eq!(cpu["verified_private_manifests"], 2);
        assert_eq!(cpu["cpu_component_execution_attempted"], true);
        assert_eq!(cpu["first_failed_relative"], serde_json::Value::Null);
        assert_eq!(cpu["owned_reference_count"], 4);
        assert_eq!(cpu["all_owned_released"], true);
        let loads = cpu["load_sequence"].as_array().unwrap();
        assert_eq!(
            loads
                .iter()
                .map(|v| v["relative"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "blender.crt/vcruntime140.dll",
                "blender.crt/vcruntime140_1.dll",
                "blender.crt/msvcp140.dll",
                "blender_cpu_check.dll"
            ]
        );
        assert!(
            loads
                .iter()
                .all(|v| v["loaded"] == true && v["path_matches"] == true)
        );
        let unloads = cpu["unload_sequence"].as_array().unwrap();
        assert_eq!(
            unloads
                .iter()
                .map(|v| v["relative"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "blender_cpu_check.dll",
                "blender.crt/msvcp140.dll",
                "blender.crt/vcruntime140_1.dll",
                "blender.crt/vcruntime140.dll"
            ]
        );
        assert!(unloads.iter().all(|v| v["released"] == true));
        assert_eq!(cpu["payload_write_denied"], true);
        assert_eq!(cpu["write_probe_win32_error"], 5);
        assert_eq!(cpu["file_data_written"], false);
        assert_eq!(cpu["image_probe"]["ok"], true);
        assert_eq!(cpu["image_probe"]["policy_changed"], false);
        assert_eq!(cpu["image_probe"]["dll_initialization_requested"], false);
        let images = cpu["image_probe"]["images"].as_array().unwrap();
        assert_eq!(images.len(), 4);
        assert!(
            images
                .iter()
                .all(|v| v["ok"] == true && v["code_invoked"] == false)
        );
        assert_eq!(cpu["payload_expected_sha256"], cpu_payload::PAYLOAD_SHA256);
        assert_eq!(cpu["component_expected_sha256"], cpu_payload::CPU_SHA256);
    }
    assert_eq!(
        row["activation"]["assembly_identities"],
        expected["assembly_identities"]
    );
    if private_assemblies {
        let identities = row["activation"]["assembly_identities"].as_array().unwrap();
        for name in ["blender.crt", "blender.shared"] {
            assert!(
                identities
                    .iter()
                    .any(|v| v.as_str().is_some_and(|s| s.starts_with(name))),
                "missing {name}"
            );
        }
    }
    if common_controls && !cpu_actctx {
        assert_eq!(row["dll"]["loaded"], true);
        assert_eq!(row["dll"]["export_called"], false);
    }
    assert_eq!(row["manifest_sha256"], hex::encode(Sha256::digest(&fixed)));
    assert_eq!(
        row["activation"]["root_identity"],
        expected["root_identity"]
    );
    assert_eq!(
        row["activation"]["assembly_count"],
        expected["assembly_count"]
    );
}

fn stage_pinned_private_manifests(destination: &Path, inherit_private_root: bool) {
    use std::io::Read;
    let runtime = std::path::PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_BLENDER_RUNTIME").expect("explicit pinned runtime"),
    );
    assert!(runtime.is_absolute());
    for (name, expected) in [
        (
            "blender.crt",
            "43bdfdc2028539332fed3dc487474407d63e79f67394145397322dda58e455ee",
        ),
        (
            "blender.shared",
            "655ad61bd460854680806f1fce59459daaf7fc309304fa17b3348c58cf745926",
        ),
    ] {
        let relative = format!("{name}/{name}.manifest");
        let mut bytes = Vec::new();
        std::fs::File::open(runtime.join(&relative))
            .unwrap()
            .take(65537)
            .read_to_end(&mut bytes)
            .unwrap();
        assert!(bytes.len() <= 65536);
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            expected,
            "exact private manifest required"
        );
        let directory = destination.join(name);
        std::fs::create_dir(&directory).unwrap();
        let target = directory.join(format!("{name}.manifest"));
        std::fs::write(&target, &bytes).unwrap();
        if !inherit_private_root {
            harden(&target);
        } // CPU copies inherit the same protected root and Host RO grant as their DLLs.
        eprintln!("dependency-only fixture: {relative} sha256={expected}; DLLs copied=false");
    }
}

fn stage_pinned_cpu_payload(destination: &Path) {
    use std::io::Write;
    let runtime = std::path::PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_BLENDER_RUNTIME").expect("explicit pinned runtime"),
    );
    assert!(runtime.is_absolute());
    let entries = cpu_payload::entries().expect("fixed payload manifest");
    for entry in &entries {
        let bytes = cpu_payload::read_checked(&runtime, entry).expect("pinned source DLL bytes");
        let target = destination.join(&entry.relative);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .unwrap();
        file.write_all(&bytes).unwrap();
        file.sync_all().unwrap();
        drop(file);
        // Inherit the protected owner-only fixture root and the Host RO/execute grant.
        drop(cpu_payload::read_checked(destination, entry).expect("staged DLL bytes"));
    }
    eprintln!(
        "CPU-component payload staged: files={}, manifest_sha256={}; owner DLL execution=false",
        entries.len(),
        cpu_payload::PAYLOAD_SHA256
    );
}

fn print_cpu_mount_acls(root: &Path) {
    let entries = cpu_payload::entries().expect("fixed payload identity");
    let first = entries.first().expect("nonempty fixed payload");
    for path in [
        root.to_path_buf(),
        root.join("blender.crt"),
        root.join(&first.relative),
    ] {
        let result = std::process::Command::new("icacls")
            .arg(&path)
            .output()
            .expect("read-only fixture ACL inspection");
        assert!(result.status.success(), "fixture ACL inspection failed");
        assert!(result.stdout.len() <= 16384 && result.stderr.len() <= 4096);
        eprintln!(
            "CPU fixture ACL after Host mount: {}\n{}",
            path.display(),
            String::from_utf8_lossy(&result.stdout)
        );
    }
}

fn harden_cpu_fixture_root(path: &Path) {
    // Only the newly created test directory is changed. Its DLL children must inherit
    // the Host's per-process RO/execute ACE; the executable retains explicit hardening.
    let principal = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap(),
        std::env::var("USERNAME").unwrap()
    );
    let status = std::process::Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{principal}:(OI)(CI)(F)"))
        .status()
        .unwrap();
    assert!(status.success());
}
