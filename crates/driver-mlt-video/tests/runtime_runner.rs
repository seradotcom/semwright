#![cfg(all(target_os = "linux", feature = "test-tools"))]

use serde_json::Value;
use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::Command,
};

fn runner() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_semwright-mlt-runtime-runner"))
}

fn fake_melt() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake-melt"))
}

fn runtime_bundle(entrypoint: &Path) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("runtime bundle");
    let bin = root.path().join("bin");
    fs::create_dir(&bin).expect("runtime bin");
    fs::copy(entrypoint, bin.join("melt-real")).expect("copy runtime entrypoint");
    symlink("melt-real", bin.join("melt")).expect("runtime entrypoint symlink");
    root
}

#[test]
fn runtime_runner_discovers_bounded_catalog_from_explicit_dependency() {
    let sealed = fake_melt();
    let bundle = runtime_bundle(&sealed);
    let output = Command::new(runner())
        .args(["discover", "--runtime-root"])
        .arg(bundle.path())
        .args(["--melt-sealed"])
        .arg(&sealed)
        .output()
        .expect("run MLT runtime runner");
    assert!(
        output.status.success(),
        "runner stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let value: Value = serde_json::from_slice(&output.stdout).expect("runner JSON");
    assert_eq!(value["schema"], 1);
    assert_eq!(value["operation"], "discover");
    assert_eq!(value["version"], "melt 7.32.0 (Semwright fixture)");
    assert_eq!(
        value["groups"]["producers"],
        serde_json::json!(["avformat", "color", "pixbuf"])
    );
    assert_eq!(
        value["groups"]["filters"],
        serde_json::json!(["brightness", "volume"])
    );
    assert_eq!(
        value["groups"]["transitions"],
        serde_json::json!(["luma", "mix"])
    );
    assert_eq!(
        value["groups"]["consumers"],
        serde_json::json!(["avformat"])
    );
    assert_eq!(
        value["groups"]["video_codecs"],
        serde_json::json!(["ffv1", "libx264"])
    );
    assert_eq!(
        value["groups"]["audio_codecs"],
        serde_json::json!(["aac", "pcm_s16le"])
    );
}

#[test]
fn runtime_runner_rejects_unmaterialized_relative_dependencies() {
    let sealed = fake_melt();
    let bundle = runtime_bundle(&sealed);
    let output = Command::new(runner())
        .args(["discover", "--runtime-root"])
        .arg(bundle.path())
        .args(["--melt-sealed", "melt"])
        .output()
        .expect("run invalid dependency fixture");
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("runner error JSON");
    assert_eq!(value["schema"], 1);
    assert_eq!(value["operation"], "error");
    assert!(
        value["error"]
            .as_str()
            .is_some_and(|message| message.contains("absolute Host-materialized path"))
    );
}

#[test]
fn runtime_runner_rejects_bundle_entrypoint_that_does_not_match_sealed_tool() {
    let sealed = fake_melt();
    let bundle = runtime_bundle(&sealed);
    fs::write(
        bundle.path().join("bin/melt-real"),
        b"not the sealed executable",
    )
    .unwrap();
    let output = Command::new(runner())
        .args(["discover", "--runtime-root"])
        .arg(bundle.path())
        .args(["--melt-sealed"])
        .arg(&sealed)
        .output()
        .expect("run mismatched runtime entrypoint");
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("runner error JSON");
    assert_eq!(value["operation"], "error");
    assert!(
        value["error"]
            .as_str()
            .is_some_and(|message| message.contains("does not match the Host-sealed melt bytes"))
    );
}

#[test]
fn runtime_runner_rejects_entrypoint_symlink_that_escapes_runtime_root() {
    let sealed = fake_melt();
    let root = tempfile::tempdir().expect("runtime root");
    let bin = root.path().join("bin");
    fs::create_dir(&bin).expect("runtime bin");
    symlink(&sealed, bin.join("melt")).expect("escaping runtime entrypoint symlink");
    let output = Command::new(runner())
        .args(["discover", "--runtime-root"])
        .arg(root.path())
        .args(["--melt-sealed"])
        .arg(&sealed)
        .output()
        .expect("run escaping runtime entrypoint");
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("runner error JSON");
    assert_eq!(value["operation"], "error");
    assert!(
        value["error"]
            .as_str()
            .is_some_and(|message| message.contains("escaped the delegated runtime root"))
    );
}

#[test]
fn runtime_runner_rejects_unknown_operations() {
    let output = Command::new(runner())
        .arg("shell")
        .output()
        .expect("run unsupported runner operation");
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("runner error JSON");
    assert_eq!(value["schema"], 1);
    assert_eq!(value["operation"], "error");
    assert!(
        value["error"]
            .as_str()
            .is_some_and(|message| message.contains("unsupported"))
    );
}
