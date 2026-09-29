#![cfg(target_os = "linux")]
use semwright_godot_driver::{
    authoring::{
        profile::{RepairMode, RepairPlanRequest},
        runtime::AuthoringRuntime,
        *,
    },
    catalog::{Catalog, Route},
    config::AuthoringConfig,
};
use semwright_semantic_composition::{Owner, PrincipalBinding, State, Verdict};
use semwright_types::ErrorCode;
use std::{fs, os::unix::fs::PermissionsExt};

fn fixture() -> GodotAuthoringSpec {
    validate::decode(include_bytes!("fixtures/authoring/two_d.json")).unwrap()
}

fn environment() -> (tempfile::TempDir, AuthoringConfig) {
    let root = tempfile::tempdir().unwrap();
    for name in ["output", "state", "input"] {
        fs::create_dir(root.path().join(name)).unwrap();
    }
    fs::write(
        root.path().join("input/start_cue.wav"),
        include_bytes!("fixtures/authoring/start_cue.wav"),
    )
    .unwrap();
    fs::set_permissions(root.path().join("state"), fs::Permissions::from_mode(0o700)).unwrap();
    let config = AuthoringConfig {
        output_root: root.path().join("output"),
        state_root: root.path().join("state"),
        input_root: Some(root.path().join("input")),
    };
    (root, config)
}

fn owner(session: &str) -> Owner {
    Owner {
        session: session.into(),
        principal: PrincipalBinding::HostSession,
    }
}

fn runtime(config: AuthoringConfig) -> AuthoringRuntime {
    let catalog = Catalog::load().unwrap();
    AuthoringRuntime::new(config, catalog.authoring_profile().unwrap()).unwrap()
}

#[test]
fn catalog_exposes_exact_composition_surface_and_strict_schemas() {
    let catalog = Catalog::load().unwrap();
    let names = catalog.names_for(Route::Authoring);
    assert_eq!(names.len(), 8);
    for name in [
        "driver.godot.composition.inspect",
        "driver.godot.composition.plan",
        "driver.godot.composition.apply",
        "driver.godot.composition.measure",
        "driver.godot.composition.validate",
        "driver.godot.composition.repair.plan",
        "driver.godot.composition.repair.apply",
        "driver.godot.composition.verify",
    ] {
        assert!(names.contains(&name.to_owned()), "{name}");
    }
    let profile = catalog.authoring_profile().unwrap();
    assert_eq!(profile.capabilities.len(), 8);
    assert_eq!(profile.required_rules.len(), 2);
    assert!(
        catalog
            .get("driver.godot.composition.inspect")
            .unwrap()
            .validate_input(&serde_json::json!({"project":"arena","unexpected":true}))
            .is_err()
    );
}

#[test]
fn empty_root_plan_apply_validate_verify_uses_a_c_f_contracts() {
    let (_root, config) = environment();
    let mut runtime = runtime(config.clone());
    let owner = owner("host-session-a");
    let spec = fixture();

    let plan = runtime.plan(&owner, spec.clone()).unwrap();
    assert!(!config.output_root.join(&spec.project).exists());
    assert!(!plan.plan_id.is_empty());
    assert_eq!(plan.base.0[0].provider_session, owner.session);

    let applied = runtime
        .apply(&owner, &plan.plan_id, false, "req-apply-1", || Ok(()))
        .unwrap();
    assert_eq!(applied.controller_state, State::Observing);
    assert_eq!(applied.source_state, "IN_SYNC");
    assert!(
        config
            .output_root
            .join(&spec.project)
            .join("project.godot")
            .is_file()
    );

    let measured = runtime.measure(&owner, &plan.plan_id).unwrap();
    assert_eq!(measured.snapshot.status, "IN_SYNC");
    assert!(measured.observation.exhaustive);
    let managed_script = measured
        .snapshot
        .files
        .iter()
        .find(|file| file.path == "scripts/arena.gd")
        .expect("managed behavior file");
    assert!(
        managed_script
            .asset
            .as_deref()
            .is_some_and(|asset| asset.starts_with("asset_"))
    );
    assert!(
        managed_script
            .revision
            .as_deref()
            .is_some_and(|revision| revision.starts_with("rev_"))
    );
    assert_eq!(managed_script.kind.as_deref(), Some("behavior"));
    assert_eq!(managed_script.logical_key.as_deref(), Some("arena"));
    assert_eq!(managed_script.active, Some(true));

    let validated = runtime.validate(&owner, &plan.plan_id).unwrap();
    assert_eq!(validated.report.verdict().unwrap(), Verdict::Pass);
    assert_eq!(validated.controller_state, State::Verified);
    assert_eq!(validated.report.checks.len(), 5);
    let required = validated
        .report
        .checks
        .iter()
        .filter(|check| validated.report.required_rules.contains(&check.rule))
        .collect::<Vec<_>>();
    assert_eq!(required.len(), 2);
    assert!(required.iter().all(|check| {
        check.verdict == Verdict::Pass
            && check.evidence.len() == 1
            && matches!(
                check.evidence[0].method.as_str(),
                "godot_managed_source_hash" | "godot_derivation_manifest"
            )
    }));
    let native_preferences = validated
        .report
        .checks
        .iter()
        .filter(|check| !validated.report.required_rules.contains(&check.rule))
        .collect::<Vec<_>>();
    assert_eq!(native_preferences.len(), 3);
    assert!(
        native_preferences
            .iter()
            .all(|check| { check.verdict == Verdict::Unknown && check.evidence.is_empty() })
    );

    let verified = runtime.verify(&owner, &plan.plan_id).unwrap();
    assert_eq!(verified.report.verdict().unwrap(), Verdict::Pass);
    assert_eq!(verified.report.effects_observed.len(), 2);
    assert_eq!(verified.report.effects_unobservable.len(), 3);
    assert_eq!(verified.receipt.owner, owner);
    assert_eq!(
        verified.receipt.operation.capability,
        "driver.godot.composition.apply"
    );
    assert!(!verified.receipt.coverage.complete);
    verified.receipt.validate().unwrap();
}

#[test]
fn plan_authority_is_owner_bound_and_single_attempt() {
    let (_root, config) = environment();
    let mut runtime = runtime(config);
    let alice = owner("host-session-a");
    let bob = owner("host-session-b");
    let plan = runtime.plan(&alice, fixture()).unwrap();

    let error = runtime
        .apply(&bob, &plan.plan_id, false, "req-bob", || Ok(()))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);

    runtime
        .apply(&alice, &plan.plan_id, false, "req-alice", || Ok(()))
        .unwrap();
    let replay = runtime
        .apply(&alice, &plan.plan_id, false, "req-replay", || Ok(()))
        .unwrap_err();
    assert_eq!(replay.code, ErrorCode::Conflict);
}

#[test]
fn missing_managed_source_has_bounded_repair_but_diverged_source_does_not() {
    let (_root, config) = environment();
    let mut runtime = runtime(config.clone());
    let owner = owner("host-session-a");
    let spec = fixture();
    let plan = runtime.plan(&owner, spec.clone()).unwrap();
    runtime
        .apply(&owner, &plan.plan_id, false, "req-initial", || Ok(()))
        .unwrap();

    let generated = config
        .output_root
        .join(&spec.project)
        .join("scripts/arena.gd");
    fs::remove_file(&generated).unwrap();

    let failed = runtime.validate(&owner, &plan.plan_id).unwrap();
    assert_eq!(failed.report.verdict().unwrap(), Verdict::Fail);
    assert_eq!(failed.controller_state, State::RepairPlanned);

    let repair = runtime
        .repair_plan(
            &owner,
            RepairPlanRequest {
                parent_plan_id: plan.plan_id.clone(),
                mode: RepairMode::MissingManagedSource,
            },
        )
        .unwrap();
    assert!(repair.repair);
    assert!(repair.writes.contains(&"scripts/arena.gd".into()));
    runtime
        .apply(&owner, &repair.plan_id, true, "req-repair", || Ok(()))
        .unwrap();
    let repaired = runtime.validate(&owner, &repair.plan_id).unwrap();
    assert_eq!(repaired.report.verdict().unwrap(), Verdict::Pass);
    assert_eq!(repaired.controller_state, State::Verified);

    fs::write(&generated, b"# human edit\n").unwrap();
    let conflict = runtime.plan(&owner, spec).unwrap_err();
    assert_eq!(conflict.code, ErrorCode::Conflict);
}
