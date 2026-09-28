//! Production CLI regressions: untrusted paths and metadata are never terminal controls.
#![cfg(unix)]
use serde_json::Value;
use std::process::Command;
use tempfile::TempDir;

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_semwright"))
        .args(args)
        .output()
        .expect("run CLI fixture")
}

#[test]
fn human_diagnostic_cannot_clear_terminal_or_forge_status_lines() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("display-fixture");
    let sdk = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../driver-sdk");
    let scaffold = cli(&[
        "driver",
        "scaffold",
        "display-fixture",
        project.to_str().unwrap(),
        "--sdk-path",
        sdk.to_str().unwrap(),
    ]);
    assert!(
        scaffold.status.success(),
        "{}",
        String::from_utf8_lossy(&scaffold.stderr)
    );
    let list = tmp.path().join("list-\u{1b}[2J\nFORGED_PASS\u{202e}.txt");
    std::fs::write(&list, "not-a-companion-mapping\n").unwrap();
    let manifest = project.join("driver.manifest.example.json");
    let output = tmp.path().join("must-not-exist.swdp");
    let result = cli(&[
        "driver",
        "package",
        "create",
        manifest.to_str().unwrap(),
        output.to_str().unwrap(),
        "--companion-list",
        list.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert!(stderr.contains("Invalid companion mapping"), "{stderr:?}");
    assert!(!stderr.contains('\u{1b}'));
    assert!(!stderr.contains('\u{202e}'));
    assert_eq!(stderr.lines().count(), 1, "{stderr:?}");
    assert!(stderr.contains("\\nFORGED_PASS"), "{stderr:?}");
    assert!(!output.exists());
}

#[test]
fn json_metadata_escapes_c1_and_bidi_without_changing_values() {
    let tmp = TempDir::new().unwrap();
    let skill = tmp.path().join("json-display");
    assert!(
        cli(&["skill", "scaffold", "json-display", skill.to_str().unwrap()])
            .status
            .success()
    );
    let filename = "asset-\u{9b}2J-\u{202e}-\u{200f}.txt";
    std::fs::write(skill.join(filename), "disposable fixture").unwrap();
    for machine in [false, true] {
        let mut args = vec!["skill", "inspect", skill.to_str().unwrap()];
        if machine {
            args.insert(0, "--json");
        }
        let result = cli(&args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let stdout = String::from_utf8(result.stdout).unwrap();
        assert!(!stdout.contains('\u{9b}'), "{stdout:?}");
        assert!(!stdout.contains('\u{202e}'), "{stdout:?}");
        assert!(!stdout.contains('\u{200f}'), "{stdout:?}");
        let value: Value = serde_json::from_str(&stdout).unwrap();
        assert!(
            value["resources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["path"] == filename)
        );
    }
}
