#![cfg(all(target_os = "windows", feature = "windows-launch-test-fixture"))]
//! Owner-only causal control. Tightens this disposable test process, never the Host.
#[path = "../src/bin/session_runner/windows_activation_readback.rs"]
mod activation_readback;
#[path = "../src/bin/session_runner/windows_cpu_payload.rs"]
mod cpu_payload;
#[path = "../src/bin/session_runner/windows_cpu_resource_activation.rs"]
mod cpu_resource_activation;
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessMitigationPolicy, ProcessSystemCallDisablePolicy,
    SetProcessMitigationPolicy,
};
fn fsctl_policy() -> Result<u32, i32> {
    let mut flags = 0u32;
    // SAFETY: exact DWORD policy output and current-process pseudo-handle.
    if unsafe {
        GetProcessMitigationPolicy(
            GetCurrentProcess(),
            ProcessSystemCallDisablePolicy,
            (&mut flags as *mut u32).cast(),
            std::mem::size_of::<u32>(),
        )
    } == 0
    {
        Err(std::io::Error::last_os_error().raw_os_error().unwrap_or(-1))
    } else {
        Ok(flags)
    }
}
#[test]
#[ignore = "Tightens only this fresh owner test process; no Blender execution or sandbox modification"]
fn owner_fsctl_restriction_control() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.iter().any(|x| x == "--exact")
            && args.iter().any(|x| x == "owner_fsctl_restriction_control")
            && args.iter().any(|x| x == "--test-threads=1"),
        "Run this mitigation-changing control alone with --exact --test-threads=1"
    );
    let runtime = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_BLENDER_RUNTIME").expect("explicit owner-pinned runtime"),
    );
    assert!(runtime.is_absolute() && runtime.is_dir());
    let entries = cpu_payload::entries().expect("fixed payload metadata");
    let entry = entries
        .iter()
        .find(|x| x.relative == cpu_payload::CPU_DLL)
        .expect("fixed CPU payload");
    let bytes = cpu_payload::read_checked(&runtime, entry).expect("pinned CPU bytes");
    let source_sha = hex::encode(Sha256::digest(&bytes));
    // Separate new owner-owned paths avoid deliberately reusing a previously queried path.
    // This does not claim a cold system SxS cache.
    let control_root = tempfile::tempdir().unwrap();
    let restricted_root = tempfile::tempdir().unwrap();
    for root in [&control_root, &restricted_root] {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.path().join(cpu_payload::CPU_DLL))
            .unwrap();
        file.write_all(&bytes).unwrap();
        file.sync_all().unwrap();
    }
    drop(bytes);
    let before_flags = fsctl_policy().expect("baseline policy query");
    assert_eq!(
        before_flags, 0,
        "this control requires an unmodified fresh owner process"
    );
    let control = cpu_resource_activation::probe(control_root.path());
    let requested_flags = before_flags | 4; // Documented DisallowFsctlSystemCalls.
    // SAFETY: adds one restriction to this disposable process only. No clearing call exists.
    let set_ok = unsafe {
        SetProcessMitigationPolicy(
            ProcessSystemCallDisablePolicy,
            (&requested_flags as *const u32).cast(),
            std::mem::size_of::<u32>(),
        )
    } != 0;
    let set_error = if set_ok {
        None
    } else {
        std::io::Error::last_os_error().raw_os_error()
    };
    let after_flags = fsctl_policy();
    let restricted = if set_ok && after_flags == Ok(4) {
        Some(cpu_resource_activation::probe(restricted_root.path()))
    } else {
        None
    };
    let final_flags = fsctl_policy();
    let original_after =
        cpu_payload::read_checked(&runtime, entry).map(|b| hex::encode(Sha256::digest(b)));
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.local/logs")
        .join(format!(
            "windows-fsctl-control-{timestamp}-{}.json",
            std::process::id()
        ));
    let mut receipt = serde_json::Map::new();
    for (key, value) in [
        ("schema_version", serde_json::json!(1)),
        (
            "probe",
            serde_json::json!("owner_fsctl_restriction_control"),
        ),
        ("pid", serde_json::json!(std::process::id())),
        ("before_flags", serde_json::json!(before_flags)),
        ("requested_flags", serde_json::json!(requested_flags)),
        ("set_ok", serde_json::json!(set_ok)),
        ("set_error", serde_json::json!(set_error)),
        ("after_flags", serde_json::json!(after_flags)),
        ("final_flags", serde_json::json!(final_flags)),
        ("control", control.clone()),
        ("restricted", serde_json::json!(restricted)),
        ("runtime_cpu_sha256_before", serde_json::json!(source_sha)),
        (
            "runtime_cpu_sha256_after",
            serde_json::json!(original_after.as_ref().ok()),
        ),
        ("cold_cache_claim", serde_json::json!(false)),
        ("distinct_new_fixture_paths", serde_json::json!(true)),
        ("global_policy_changed", serde_json::json!(false)),
        ("host_policy_changed", serde_json::json!(false)),
        ("capability_added", serde_json::json!(false)),
        (
            "policy_tightened_in_disposable_process",
            serde_json::json!(set_ok),
        ),
        ("policy_relaxation_requested", serde_json::json!(false)),
        ("broker_used", serde_json::json!(false)),
        ("blender_executed", serde_json::json!(false)),
        ("dll_initialization_requested", serde_json::json!(false)),
        ("A05", serde_json::json!("BLOCKED")),
        (
            "harness_executable",
            serde_json::json!(std::env::current_exe().unwrap()),
        ),
        (
            "test_source_sha256",
            serde_json::json!(hex::encode(Sha256::digest(include_bytes!(
                "windows_fsctl_control.rs"
            )))),
        ),
        ("receipt_path", serde_json::json!(path)),
    ] {
        receipt.insert(key.into(), value);
    }
    let receipt = serde_json::Value::Object(receipt);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    writeln!(file, "{receipt}").unwrap();
    file.sync_all().unwrap();
    eprintln!("FSCTL_RESTRICTION_CONTROL {receipt}");
    assert_eq!(control["ok"], true, "baseline owner control must pass");
    assert!(
        set_ok && after_flags == Ok(4) && final_flags == Ok(4),
        "restriction must be confirmed, never inferred"
    );
    assert_eq!(
        original_after.unwrap(),
        source_sha,
        "original runtime must be unchanged"
    );
    assert!(
        restricted
            .as_ref()
            .is_some_and(|v| v["image_verified"] == true && v["create_attempted"] == true),
        "restricted observation must be complete"
    );
    // Outcome is evidence, not a pass assertion about Blender or the LPAC hypothesis.
}
