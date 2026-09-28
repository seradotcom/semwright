use serde_json::Value;
use std::process::Command;
use tempfile::TempDir;

fn semwright(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_semwright"))
        .args(args)
        .output()
        .expect("run semwright")
}

#[test]
fn static_skill_cli_has_human_and_json_surfaces_and_portable_bundle() {
    let temp = TempDir::new().unwrap();
    let skill = temp.path().join("cli-fixture");
    let skill_text = skill.to_str().unwrap();

    let created = semwright(&["--json", "skill", "scaffold", "cli-fixture", skill_text]);
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );

    let validated = semwright(&["--json", "skill", "validate", skill_text]);
    assert!(validated.status.success());
    let value: Value = serde_json::from_slice(&validated.stdout).unwrap();
    assert_eq!(value["standard_valid"], true);
    assert_eq!(value["skill"], "cli-fixture");
    assert_eq!(value["script_execution"], "disabled");

    let human = semwright(&["skill", "validate", skill_text]);
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.starts_with("PASS\n"));
    assert!(human.contains("Script execution:\n  disabled"));

    let inspected = semwright(&["--json", "skill", "inspect", skill_text]);
    assert!(inspected.status.success());
    let value: Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(value["skill_instructions"]["authority"], "none");
    assert_eq!(value["script_execution"], "disabled");

    let archive = temp.path().join("cli-fixture.zip");
    let archived = semwright(&[
        "--json",
        "skill",
        "bundle",
        skill_text,
        archive.to_str().unwrap(),
    ]);
    assert!(
        archived.status.success(),
        "{}",
        String::from_utf8_lossy(&archived.stderr)
    );
    let value: Value = serde_json::from_slice(&archived.stdout).unwrap();
    assert_eq!(value["format"], "zip-store-v1");
    assert!(archive.metadata().unwrap().len() > 0);
}
