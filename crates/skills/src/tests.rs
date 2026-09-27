use super::*;
use proptest::prelude::*;
use semwright_registry::{Metadata, catalog::descriptor_digest};
use semwright_types::{CommandDescriptor, Idempotency, Risk, SourceKind};
use serde_json::json;
use std::fs;
use tempfile::TempDir;

fn write_skill(root: &std::path::Path, name: &str, extra: &str) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: Test Skill for Semwright validation.\n---\n\n# Test\n{extra}\n"
        ),
    )
    .unwrap();
}

fn temp_skill(name: &str) -> (TempDir, std::path::PathBuf) {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join(name);
    write_skill(&root, name, "");
    (temp, root)
}

fn capability(name: &str, tags: &[&str], available: bool) -> CatalogCapability {
    let descriptor = CommandDescriptor {
        name: name.into(),
        version: "1".into(),
        description: "fixture capability".into(),
        input_schema: json!({"type":"object","additionalProperties":false}),
        output_schema: json!({"type":"object"}),
        requires: vec!["filesystem.read".into()],
        risk: Risk::ReadOnly,
        idempotency: Idempotency::ReadOnly,
        timeout_ms: 1000,
        dry_run: true,
        interactive_consent: false,
        backends: vec!["fixture".into()],
    };
    CatalogCapability {
        provenance: Metadata {
            source: SourceKind::Driver,
            provider: "driver:fixture".into(),
            source_version: "1.0.0".into(),
            app: Some("fixture".into()),
            aliases: vec![],
            tags: tags.iter().map(|tag| (*tag).into()).collect(),
            object_types: vec![],
            untrusted_metadata: true,
            descriptor_sha256: descriptor_digest(&descriptor).unwrap(),
        },
        descriptor,
        routes: vec![CatalogRoute {
            provider: "fixture".into(),
            status: if available {
                "supported"
            } else {
                "unavailable"
            }
            .into(),
            available,
        }],
        policy_preview: None,
    }
}

fn write_requirements(root: &std::path::Path, value: serde_json::Value) {
    fs::create_dir_all(root.join(".semwright")).unwrap();
    fs::write(
        root.join(".semwright/requirements.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
}

#[test]
fn validates_standard_skill_without_semwright_metadata() {
    let (_temp, root) = temp_skill("plain-skill");
    let report = validate(&root).unwrap();
    assert!(report.standard_valid);
    assert!(!report.semwright_requirements);
    assert_eq!(report.script_execution, "disabled");
}

#[test]
fn rejects_missing_or_duplicate_frontmatter_fields() {
    let temp = TempDir::new().unwrap();
    let missing = temp.path().join("missing");
    fs::create_dir(&missing).unwrap();
    fs::write(missing.join("SKILL.md"), "---\nname: missing\n---\nbody").unwrap();
    assert!(load(&missing).is_err());

    let duplicate = temp.path().join("duplicate");
    fs::create_dir(&duplicate).unwrap();
    fs::write(
        duplicate.join("SKILL.md"),
        "---\nname: duplicate\nname: other\ndescription: x\n---\n",
    )
    .unwrap();
    assert!(load(&duplicate).is_err());
}
#[test]
fn rejects_yaml_aliases_invalid_utf8_and_oversized_frontmatter() {
    let temp = TempDir::new().unwrap();
    let alias = temp.path().join("alias");
    fs::create_dir(&alias).unwrap();
    fs::write(
        alias.join("SKILL.md"),
        "---\nname: alias\ndescription: &d dangerous\n---\n",
    )
    .unwrap();
    assert!(load(&alias).is_err());

    let bad = temp.path().join("bad-utf8");
    fs::create_dir(&bad).unwrap();
    fs::write(bad.join("SKILL.md"), [0xff, 0xfe, 0xfd]).unwrap();
    assert!(load(&bad).is_err());

    let huge = temp.path().join("huge-frontmatter");
    fs::create_dir(&huge).unwrap();
    let text = format!(
        "---\nname: huge-frontmatter\ndescription: {}\n---\n",
        "x".repeat(MAX_FRONTMATTER_BYTES)
    );
    fs::write(huge.join("SKILL.md"), text).unwrap();
    assert!(load(&huge).is_err());
}

#[test]
fn archive_paths_reject_traversal_and_absolute_paths() {
    for value in [
        "../secret",
        "refs/../../secret",
        "/etc/passwd",
        r"..\secret",
        r"refs\..\secret",
        r"references\a.md",
        r"C:\secret",
        "C:/secret",
        r"\\server\share",
    ] {
        assert!(validate_archive_path(value).is_err(), "{value}");
    }
    assert_eq!(
        validate_archive_path("./references/a.md").unwrap(),
        std::path::PathBuf::from("references/a.md")
    );
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape_and_recursive_symlink() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new().unwrap();

    let escape = temp.path().join("escape");
    write_skill(&escape, "escape", "");
    fs::create_dir(escape.join("references")).unwrap();
    symlink("/etc/passwd", escape.join("references/secret")).unwrap();
    assert!(load(&escape).is_err());

    let recursive = temp.path().join("recursive");
    write_skill(&recursive, "recursive", "");
    fs::create_dir(recursive.join("references")).unwrap();
    symlink("../references", recursive.join("references/loop")).unwrap();
    assert!(load(&recursive).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_hardlinked_resources_and_lock_write_does_not_truncate_external_inode() {
    let temp = TempDir::new().unwrap();

    let root = temp.path().join("hardlink-skill");
    write_skill(&root, "hardlink-skill", "");
    fs::create_dir(root.join("references")).unwrap();
    let external = temp.path().join("external-secret");
    fs::write(&external, "do-not-touch").unwrap();
    fs::hard_link(&external, root.join("references/leak.txt")).unwrap();
    assert!(load(&root).is_err());

    fs::remove_file(root.join("references/leak.txt")).unwrap();
    fs::create_dir_all(root.join(".semwright")).unwrap();
    let lock_path = root.join(".semwright/lock.json");
    fs::hard_link(&external, &lock_path).unwrap();
    let lock = SkillLock {
        version: 1,
        semwright_version: "0.9.0-dev.1".into(),
        requirements_sha256: "0".repeat(64),
        entries: vec![],
    };
    write_lock(&root, &lock).unwrap();
    assert_eq!(fs::read_to_string(&external).unwrap(), "do-not-touch");
    assert_ne!(fs::read_to_string(lock_path).unwrap(), "do-not-touch");
}

#[test]
fn enforces_resource_count_and_size_budgets_without_allocating_large_files() {
    let temp = TempDir::new().unwrap();
    let count = temp.path().join("too-many");
    write_skill(&count, "too-many", "");
    let refs = count.join("references");
    fs::create_dir(&refs).unwrap();
    for index in 0..=MAX_RESOURCE_COUNT {
        fs::write(refs.join(format!("{index}.txt")), b"x").unwrap();
    }
    assert!(load(&count).is_err());

    let directories = temp.path().join("too-many-directories");
    write_skill(&directories, "too-many-directories", "");
    for index in 0..=MAX_RESOURCE_COUNT {
        fs::create_dir(directories.join(format!("dir-{index}"))).unwrap();
    }
    assert!(load(&directories).is_err());

    let large = temp.path().join("large-file");
    write_skill(&large, "large-file", "");
    let file = fs::File::create(large.join("large.bin")).unwrap();
    file.set_len(MAX_RESOURCE_BYTES + 1).unwrap();
    assert!(load(&large).is_err());
}

#[test]
fn requirements_reject_authority_fields_and_resolve_exact_query_optional_routes() {
    assert!(
        parse_requirements_bytes(br#"{"version":1,"version":1,"semwright":{"capabilities":[]}}"#)
            .is_err()
    );
    assert!(
        parse_requirements_bytes(
            br#"{"version":1,"semwright":{"capabilities":[],"capabilities":[]}}"#
        )
        .is_err()
    );

    let (_temp, root) = temp_skill("requires");
    write_requirements(
        &root,
        json!({
            "version":1,
            "semwright":{
                "capabilities":[
                    {"id":"driver.fixture.read","required":true},
                    {"query":{"tags":["artifact-out:model/3d"]},"minimum_matches":1,"required":true},
                    {"id":"driver.fixture.optional","required":false}
                ]
            }
        }),
    );
    let package = load(&root).unwrap();
    let report = doctor(
        &package,
        &[
            capability("driver.fixture.read", &[], true),
            capability("driver.fixture.export", &["artifact-out:model/3d"], true),
        ],
        "0.9.0-dev.1",
    )
    .unwrap();
    assert!(report.semwright_compatible);
    assert_eq!(report.result, "degraded");
    assert_eq!(
        report.authority_summary.required_scopes,
        ["filesystem.read"]
    );

    fs::write(
        root.join(".semwright/requirements.json"),
        br#"{"version":1,"semwright":{"permissions":["filesystem.write"]}}"#,
    )
    .unwrap();
    assert!(load(&root).is_err());
}

#[test]
fn doctor_distinguishes_policy_preview_denial() {
    let (_temp, root) = temp_skill("policy-preview");
    write_requirements(
        &root,
        json!({
            "version":1,
            "semwright":{"capabilities":[{"id":"driver.fixture.read"}]}
        }),
    );
    let package = load(&root).unwrap();
    let mut denied = capability("driver.fixture.read", &[], true);
    denied.policy_preview = Some(PolicyPreview {
        state: "deny".into(),
        preview_only: true,
        execution_rechecks: true,
        reason: "fixture policy denies this capability".into(),
    });
    let report = doctor(&package, &[denied], "0.9.0-dev.1").unwrap();
    assert!(!report.semwright_compatible);
    assert_eq!(report.policy_denied, ["capability:driver.fixture.read"]);
    assert_eq!(report.requirements[0].state, "policy_denied_preview");
    assert_eq!(report.authority_summary.policy_state, "preview_denied");
}

#[test]
fn doctor_distinguishes_missing_from_unavailable() {
    let (_temp, root) = temp_skill("routes");
    write_requirements(
        &root,
        json!({
            "version":1,
            "semwright":{"capabilities":[
                {"id":"driver.fixture.offline"},
                {"id":"driver.fixture.missing"}
            ]}
        }),
    );
    let package = load(&root).unwrap();
    let report = doctor(
        &package,
        &[capability("driver.fixture.offline", &[], false)],
        "0.9.0-dev.1",
    )
    .unwrap();
    assert!(!report.semwright_compatible);
    assert_eq!(report.unavailable, ["capability:driver.fixture.offline"]);
    assert_eq!(report.missing, ["capability:driver.fixture.missing"]);
    assert_eq!(report.authority_summary.policy_state, "not_evaluated");
}

#[test]
fn lock_detects_descriptor_and_schema_drift() {
    let (_temp, root) = temp_skill("locked");
    write_requirements(
        &root,
        json!({
            "version":1,
            "semwright":{"capabilities":[{"id":"driver.fixture.read"}]}
        }),
    );
    let package = load(&root).unwrap();
    let current = capability("driver.fixture.read", &[], true);
    let generated = lock(&package, std::slice::from_ref(&current), "0.9.0-dev.1").unwrap();
    assert_eq!(generated.semwright_version, "0.9.0-dev.1");
    write_lock(&root, &generated).unwrap();

    let locked = load(&root).unwrap();
    let clean = doctor(&locked, std::slice::from_ref(&current), "0.9.0-dev.1").unwrap();
    assert!(clean.drift.is_empty());

    let version_drift = doctor(&locked, std::slice::from_ref(&current), "0.9.0-dev.2").unwrap();
    assert!(
        version_drift
            .drift
            .iter()
            .any(|drift| { drift.capability_id == "<semwright>" && drift.field == "version" })
    );
    assert!(!version_drift.semwright_compatible);

    let mut changed = current.clone();
    changed.descriptor.input_schema = json!({"type":"object","properties":{"x":{"type":"string"}}});
    changed.provenance.descriptor_sha256 = descriptor_digest(&changed.descriptor).unwrap();
    let changed_report = doctor(&locked, &[changed], "0.9.0-dev.1").unwrap();
    assert!(!changed_report.drift.is_empty());
    assert!(!changed_report.semwright_compatible);
}

#[test]
fn versioned_report_schema_accepts_validate_doctor_and_test_outputs() {
    let (_temp, root) = temp_skill("reports");
    let package = load(&root).unwrap();
    let schema: serde_json::Value = serde_json::from_str(REPORT_SCHEMA).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let reports = [
        serde_json::to_value(validate(&root).unwrap()).unwrap(),
        serde_json::to_value(doctor(&package, &[], "0.9.0-dev.1").unwrap()).unwrap(),
        serde_json::to_value(conformance_test(&package, &[], "0.9.0-dev.1").unwrap()).unwrap(),
    ];
    for report in reports {
        assert!(validator.is_valid(&report), "{report:#}");
    }
}

#[test]
fn example_tests_validate_schemas_without_execution() {
    let (_temp, root) = temp_skill("examples");
    fs::create_dir_all(root.join(".semwright/examples")).unwrap();
    fs::write(
        root.join(".semwright/examples/good.json"),
        br#"{"capability":"driver.fixture.read","input":{},"output":{}}"#,
    )
    .unwrap();
    let package = load(&root).unwrap();
    let report = test_examples(&package, &[capability("driver.fixture.read", &[], true)]).unwrap();
    assert_eq!(report.checked, 1);
    assert_eq!(report.passed, 1);
    assert_eq!(report.executed_operations, 0);
}

#[test]
fn scaffold_and_export_create_portable_standard_skills() {
    let temp = TempDir::new().unwrap();
    let scaffolded = temp.path().join("new-skill");
    scaffold("new-skill", &scaffolded).unwrap();
    assert!(load(&scaffolded).is_ok());

    let exported = temp.path().join("fixture-export");
    export_capability(&capability("driver.fixture.read", &[], true), &exported).unwrap();
    let package = load(&exported).unwrap();
    assert_eq!(
        package.requirements.unwrap().semwright.capabilities[0]
            .id
            .as_deref(),
        Some("driver.fixture.read")
    );
}

#[test]
fn bundle_is_deterministic_single_root_and_excludes_obvious_secrets() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("portable");
    write_skill(&root, "portable", "");
    fs::create_dir(root.join("references")).unwrap();
    fs::write(root.join("references/info.md"), "hello").unwrap();
    fs::write(root.join(".env"), "SECRET=value").unwrap();
    let package = load(&root).unwrap();
    let first = temp.path().join("one.zip");
    let second = temp.path().join("two.zip");
    let a = bundle(&package, &first).unwrap();
    let b = bundle(&package, &second).unwrap();
    assert_eq!(a.sha256, b.sha256);
    assert_eq!(a.excluded_sensitive, 1);
    let bytes = fs::read(first).unwrap();
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert!(
        bytes
            .windows(b"portable/SKILL.md".len())
            .any(|w| w == b"portable/SKILL.md")
    );
    assert!(
        !bytes
            .windows(b"SECRET=value".len())
            .any(|w| w == b"SECRET=value")
    );
}

#[test]
fn unicode_skill_names_follow_reference_normalization_rules() {
    assert!(valid_skill_name("café"));
    assert!(valid_skill_name("数据"));
    assert!(!valid_skill_name("Café"));
    assert!(!valid_skill_name("café--tool"));

    let temp = TempDir::new().unwrap();
    let root = temp.path().join("café");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("SKILL.md"),
        "---\nname: cafe\u{301}\ndescription: Skill name is canonically equivalent to its directory.\n---\n",
    )
    .unwrap();
    assert!(load(&root).is_ok());

    let spaced = temp.path().join(" spaced");
    fs::create_dir(&spaced).unwrap();
    fs::write(
        spaced.join("SKILL.md"),
        "---\nname: ' spaced'\ndescription: Whitespace is trimmed from the field but not from directory identity.\n---\n",
    )
    .unwrap();
    assert!(load(&spaced).is_err());
}

proptest! {
    #[test]
    fn parent_components_are_never_valid_archive_paths(prefix in "[a-z]{1,12}") {
        let candidate = format!("{prefix}/../secret");
        prop_assert!(validate_archive_path(&candidate).is_err());
    }

    #[test]
    fn canonical_skill_names_round_trip(name in "[a-z][a-z0-9-]{0,30}[a-z0-9]") {
        if !name.contains("--") {
            prop_assert!(valid_skill_name(&name));
        }
    }
}

#[test]
fn official_skills_validate_and_keep_progressive_disclosure_small() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in [
        "semwright-core",
        "semwright-cross-app-artifacts",
        "semwright-workflow-distillation",
        "semwright-driver-authoring",
        "semwright-figma-production",
    ] {
        let report = validate(&repo.join("skills").join(name)).unwrap();
        assert!(report.standard_valid, "{name}");
        assert!(
            report.skill_md_bytes < 16 * 1024,
            "{name} SKILL.md is too large"
        );
        assert!(
            report.total_bytes < 64 * 1024,
            "{name} package text footprint is too large"
        );
    }
}
