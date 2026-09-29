use semwright_registry::Registry;
use semwright_skills as skills;
use std::path::Path;
#[test]
fn continuity_skill_uses_real_catalog_and_bundler_without_claiming_runtime_execution() {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/semwright-project-continuity");
    let validation = skills::validate(&root).unwrap();
    assert!(validation.standard_valid);
    let registry = Registry::builtin().unwrap();
    let catalog: Vec<_> = registry
        .all()
        .map(|descriptor| skills::CatalogCapability {
            descriptor: descriptor.clone(),
            provenance: registry.metadata(&descriptor.name).unwrap().clone(),
            routes: Vec::new(),
            policy_preview: None,
        })
        .collect();
    let package = skills::load(&root).unwrap();
    let report = skills::conformance_test(&package, &catalog, env!("CARGO_PKG_VERSION")).unwrap();
    assert!(report.pass);
    assert_eq!(report.script_execution, "disabled");
    assert_eq!(report.examples.checked, 6);
    assert_eq!(report.examples.passed, 6);
    assert_eq!(report.executed_operations, 0);
    let temp = tempfile::tempdir().unwrap();
    let copy = temp.path().join("semwright-project-continuity");
    for relative in [
        "SKILL.md",
        "references/states.md",
        ".semwright/requirements.json",
        ".semwright/examples/discover.json",
        ".semwright/examples/describe.json",
        ".semwright/examples/project-create.json",
        ".semwright/examples/impact.json",
        ".semwright/examples/provenance.json",
        ".semwright/examples/gc-preview.json",
    ] {
        let target = copy.join(relative);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(root.join(relative), target).unwrap();
    }
    let lock = skills::lock(&package, &catalog, env!("CARGO_PKG_VERSION")).unwrap();
    skills::write_lock(&copy, &lock).unwrap();
    let locked = skills::load(&copy).unwrap();
    assert!(
        skills::conformance_test(&locked, &catalog, env!("CARGO_PKG_VERSION"))
            .unwrap()
            .pass
    );
    let output = if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../verification/project-graph")
    } else {
        temp.path().join("artifacts")
    };
    std::fs::create_dir_all(&output).unwrap();
    let archive = output.join("semwright-project-continuity.swskill");
    let bundle = skills::bundle(&locked, &archive).unwrap();
    let bytes = std::fs::read(&archive).unwrap();
    assert!(bytes.starts_with(b"PK"));
    assert_eq!(
        bundle.sha256,
        semwright_project_graph::composition::Digest::of_bytes(&bytes).as_str()
    );
    let evidence = serde_json::json!({"schema_version":1,"validation":validation,"conformance":report,"bundle":bundle,"runtime_gate":"Broker route execution evidence is produced by the separate core integration lane; native cross-app and rebuild execution remain open","native":false,"executed_project_operations":0});
    std::fs::write(
        output.join("skill-package.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    std::fs::write(
        output.join("skill-lock.json"),
        serde_json::to_vec_pretty(&lock).unwrap(),
    )
    .unwrap();
}
