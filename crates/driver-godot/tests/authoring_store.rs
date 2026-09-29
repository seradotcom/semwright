#![cfg(target_os = "linux")]
use semwright_godot_driver::{
    authoring::{store::Store, *},
    config::AuthoringConfig,
};
use semwright_types::{Error, ErrorCode};
use std::{
    cell::Cell,
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
};
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
#[test]
fn creates_from_empty_without_a_paired_editor_and_reopens_provider_state() {
    let (_root, config) = environment();
    let store = Store::new(config.clone()).unwrap();
    let spec = fixture();
    let prepared = store.prepare(&spec, false, false).unwrap();
    assert_eq!(prepared.before.status, "EMPTY");
    assert!(
        !config.output_root.join(&spec.project).exists(),
        "planning must not create native files"
    );
    let written = store.apply(&prepared, || Ok(())).unwrap();
    assert_eq!(written.source_state, "IN_SYNC");
    let reopened = Store::new(config).unwrap();
    let observed = reopened.snapshot(&spec.project).unwrap();
    assert_eq!(observed.record().unwrap().project, written.project);
    assert_eq!(observed.status, "IN_SYNC");
}
#[test]
fn incremental_behavior_ui_and_entity_count_preserve_unaffected_resources_and_identity() {
    let (_root, config) = environment();
    let store = Store::new(config.clone()).unwrap();
    let mut spec = fixture();
    store
        .apply(&store.prepare(&spec, false, false).unwrap(), || Ok(()))
        .unwrap();
    let first = store.snapshot(&spec.project).unwrap();
    let first_bindings = first.record().unwrap().bindings.clone();
    let project = config.output_root.join(&spec.project);
    let animation = project.join("resources/arena_pulse.tres");
    let audio = project.join("assets/start_cue.wav");
    let animation_inode = fs::metadata(&animation).unwrap().ino();
    let audio_inode = fs::metadata(&audio).unwrap().ino();
    let audio_bytes = fs::read(&audio).unwrap();

    spec.scenes[0].behavior.variables[0].initial = Literal::Int(4);
    let hud = spec.scenes[0]
        .entities
        .iter_mut()
        .find(|entity| entity.id == "hud")
        .unwrap();
    match &mut hud.node {
        NativeNode::Label { text, .. } => *text = "Ready for the next round".into(),
        _ => panic!("fixture HUD must remain a Label"),
    }
    spec.scenes[0].entities.push(Entity {
        id: "bonus_marker".into(),
        parent: None,
        position: [12.0, 48.0, 0.0],
        rotation: [0.0; 3],
        scale: [1.0; 3],
        groups: vec!["incremental".into()],
        node: NativeNode::Visual2d {
            size: [18.0, 18.0],
            color: [0.2, 0.7, 0.9, 1.0],
        },
    });

    let plan = store.prepare(&spec, false, false).unwrap();
    assert!(plan.writes.contains(&"scripts/arena.gd".into()));
    assert!(plan.writes.contains(&"scenes/arena.tscn".into()));
    assert!(!plan.writes.contains(&"resources/arena_pulse.tres".into()));
    assert!(!plan.writes.contains(&"assets/start_cue.wav".into()));
    store.apply(&plan, || Ok(())).unwrap();

    let second = store.snapshot(&spec.project).unwrap();
    let second_bindings = &second.record().unwrap().bindings;
    for (key, identity) in first_bindings {
        assert_eq!(
            second_bindings.get(&key),
            Some(&identity),
            "pre-existing logical identity changed for {key}"
        );
    }
    assert!(second_bindings.contains_key("entity:arena/bonus_marker"));
    assert_eq!(animation_inode, fs::metadata(animation).unwrap().ino());
    assert_eq!(audio_inode, fs::metadata(&audio).unwrap().ino());
    assert_eq!(audio_bytes, fs::read(audio).unwrap());
}
#[test]
fn material_and_animation_graph_bindings_are_persistent_project_identities() {
    let (_root, config) = environment();
    let store = Store::new(config.clone()).unwrap();

    let mut materials =
        validate::decode(include_bytes!("fixtures/authoring/resource_sharing.json")).unwrap();
    store
        .apply(&store.prepare(&materials, false, false).unwrap(), || Ok(()))
        .unwrap();
    let first_material = store
        .snapshot(&materials.project)
        .unwrap()
        .record()
        .unwrap()
        .bindings["material:arena/bronze"]
        .clone();
    let NativeNode::Mesh3dMaterial { material, .. } = &mut materials.scenes[0].entities[2].node
    else {
        panic!("resource sharing fixture changed");
    };
    material.roughness_override = Some(0.4);
    store
        .apply(&store.prepare(&materials, false, false).unwrap(), || Ok(()))
        .unwrap();
    let material_snapshot = store.snapshot(&materials.project).unwrap();
    assert_eq!(
        material_snapshot.record().unwrap().bindings["material:arena/bronze"],
        first_material
    );
    let material_file = &material_snapshot.record().unwrap().files
        ["resources/arena_material_bronze.tres"];
    assert_eq!(material_file.logical_key, "material:arena/bronze");
    assert_eq!(material_file.kind, "material_shared");
    assert!(material_file.active);

    let mut graphs =
        validate::decode(include_bytes!("fixtures/authoring/animation_graphs.json")).unwrap();
    store
        .apply(&store.prepare(&graphs, false, false).unwrap(), || Ok(()))
        .unwrap();
    let first_graph_bindings = store
        .snapshot(&graphs.project)
        .unwrap()
        .record()
        .unwrap()
        .bindings
        .iter()
        .filter(|(key, _)| key.starts_with("animation_graph:"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(first_graph_bindings.len(), 2);

    let AnimationGraphRoot::StateMachine { transitions, .. } =
        &mut graphs.scenes[0].animation_graphs[0].root
    else {
        panic!("animation graph fixture changed");
    };
    transitions[0].xfade_time = 0.35;
    store
        .apply(&store.prepare(&graphs, false, false).unwrap(), || Ok(()))
        .unwrap();
    let graph_snapshot = store.snapshot(&graphs.project).unwrap();
    for (key, identity) in &first_graph_bindings {
        assert_eq!(
            graph_snapshot.record().unwrap().bindings.get(key),
            Some(identity)
        );
    }
    let scene = fs::read_to_string(
        config
            .output_root
            .join(&graphs.project)
            .join("scenes/arena.tscn"),
    )
    .unwrap();
    for (key, identity) in first_graph_bindings {
        let logical_key = key
            .strip_prefix("animation_graph:")
            .expect("graph binding prefix");
        assert!(scene.contains(&format!(
            "metadata/semwright_logical_key={}\nmetadata/semwright_logical_id={}",
            serde_json::to_string(&format!("animation_graph/{logical_key}")).unwrap(),
            serde_json::to_string(identity.as_str()).unwrap()
        )));
    }
}

#[test]
fn identical_intent_is_a_noop() {
    let (_root, config) = environment();
    let store = Store::new(config).unwrap();
    let spec = fixture();
    store
        .apply(&store.prepare(&spec, false, false).unwrap(), || Ok(()))
        .unwrap();
    let plan = store.prepare(&spec, false, false).unwrap();
    assert!(plan.writes.is_empty());
    let before = store.snapshot(&spec.project).unwrap().fingerprint;
    store.apply(&plan, || Ok(())).unwrap();
    assert_eq!(before, store.snapshot(&spec.project).unwrap().fingerprint);
}
#[test]
fn human_edit_is_diverged_and_never_overwritten_by_a_normal_plan() {
    let (_root, config) = environment();
    let store = Store::new(config.clone()).unwrap();
    let spec = fixture();
    store
        .apply(&store.prepare(&spec, false, false).unwrap(), || Ok(()))
        .unwrap();
    let script = config
        .output_root
        .join(&spec.project)
        .join("scripts/arena.gd");
    let human = "# Explicit external-edit fault injection; not fixture creation.\nextends Node\n";
    fs::write(&script, human).unwrap();
    assert_eq!(store.snapshot(&spec.project).unwrap().status, "DIVERGED");
    assert_eq!(
        store.prepare(&spec, false, false).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        store.prepare(&spec, true, false).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(fs::read_to_string(script).unwrap(), human);
}
#[test]
fn stale_plan_cannot_overwrite_a_changed_source() {
    let (_root, config) = environment();
    let store = Store::new(config.clone()).unwrap();
    let mut spec = fixture();
    store
        .apply(&store.prepare(&spec, false, false).unwrap(), || Ok(()))
        .unwrap();
    spec.title = "New title".into();
    let plan = store.prepare(&spec, false, false).unwrap();
    let project = config.output_root.join(&spec.project).join("project.godot");
    fs::write(&project, "human edit").unwrap();
    let error = store.apply(&plan, || Ok(())).unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleReference);
    assert!(error.outcome_known);
    assert_eq!(fs::read_to_string(project).unwrap(), "human edit");
}
#[test]
fn interruption_stays_partial_until_explicit_reconciliation() {
    let (_root, config) = environment();
    let store = Store::new(config).unwrap();
    let spec = fixture();
    let plan = store.prepare(&spec, false, false).unwrap();
    let calls = Cell::new(0);
    let error = store
        .apply(&plan, || {
            calls.set(calls.get() + 1);
            if calls.get() >= 4 {
                Err(Error::new(ErrorCode::Cancelled, "synthetic cancellation"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert!(!error.outcome_known);
    assert_eq!(store.snapshot(&spec.project).unwrap().status, "PARTIAL");
    assert!(store.prepare(&spec, false, false).is_err());
    let recovery = store.prepare(&spec, false, true).unwrap();
    store.apply(&recovery, || Ok(())).unwrap();
    assert_eq!(store.snapshot(&spec.project).unwrap().status, "IN_SYNC");
}
#[test]
fn missing_owned_source_can_be_repaired_without_changing_intent() {
    let (_root, config) = environment();
    let store = Store::new(config.clone()).unwrap();
    let spec = fixture();
    store
        .apply(&store.prepare(&spec, false, false).unwrap(), || Ok(()))
        .unwrap();
    let path = config
        .output_root
        .join(&spec.project)
        .join("scripts/arena.gd");
    let bytes = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    assert!(store.prepare(&spec, false, false).is_err());
    let repair = store.prepare(&spec, true, false).unwrap();
    assert!(repair.writes.contains(&"scripts/arena.gd".into()));
    store.apply(&repair, || Ok(())).unwrap();
    assert_eq!(fs::read(path).unwrap(), bytes);
}
#[test]
fn existing_directory_and_symlink_roots_are_never_adopted_implicitly() {
    let (root, config) = environment();
    let store = Store::new(config.clone()).unwrap();
    let spec = fixture();
    let target = config.output_root.join(&spec.project);
    fs::create_dir(&target).unwrap();
    fs::write(target.join("human.txt"), b"sentinel").unwrap();
    assert!(store.prepare(&spec, false, false).is_err());
    assert_eq!(fs::read(target.join("human.txt")).unwrap(), b"sentinel");
    let mut alternate = spec.clone();
    alternate.project = "other_project".into();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel.txt"), b"private canary").unwrap();
    symlink(&outside, config.output_root.join(&alternate.project)).unwrap();
    assert!(store.prepare(&alternate, false, false).is_err());
    assert_eq!(
        fs::read(outside.join("sentinel.txt")).unwrap(),
        b"private canary"
    );
    assert!(!outside.join("project.godot").exists());
}
#[test]
fn symlink_and_hardlink_managed_files_fail_closed() {
    let (root, config) = environment();
    let store = Store::new(config.clone()).unwrap();
    let spec = fixture();
    store
        .apply(&store.prepare(&spec, false, false).unwrap(), || Ok(()))
        .unwrap();
    let file = config
        .output_root
        .join(&spec.project)
        .join("scripts/arena.gd");
    let canary = root.path().join("canary.txt");
    fs::write(&canary, b"canary").unwrap();
    fs::remove_file(&file).unwrap();
    symlink(&canary, &file).unwrap();
    assert!(store.snapshot(&spec.project).is_err());
    fs::remove_file(&file).unwrap();
    fs::hard_link(&canary, &file).unwrap();
    assert!(store.snapshot(&spec.project).is_err());
    assert_eq!(fs::read(canary).unwrap(), b"canary");
}
#[test]
fn reintroduced_name_gets_a_new_logical_identity() {
    let (_root, config) = environment();
    let store = Store::new(config).unwrap();
    let mut spec = fixture();
    let entity = Entity {
        id: "temporary_object".into(),
        parent: None,
        position: [0.0; 3],
        rotation: [0.0; 3],
        scale: [1.0; 3],
        groups: vec![],
        node: NativeNode::Node2d,
    };
    spec.scenes[0].entities.push(entity.clone());
    store
        .apply(&store.prepare(&spec, false, false).unwrap(), || Ok(()))
        .unwrap();
    let first = store
        .snapshot(&spec.project)
        .unwrap()
        .record()
        .unwrap()
        .bindings["entity:arena/temporary_object"]
        .clone();
    spec.scenes[0].entities.pop();
    store
        .apply(&store.prepare(&spec, false, false).unwrap(), || Ok(()))
        .unwrap();
    spec.scenes[0].entities.push(entity);
    store
        .apply(&store.prepare(&spec, false, false).unwrap(), || Ok(()))
        .unwrap();
    let second = store
        .snapshot(&spec.project)
        .unwrap()
        .record()
        .unwrap()
        .bindings["entity:arena/temporary_object"]
        .clone();
    assert_ne!(
        first, second,
        "a reused name must not prove identity continuity"
    );
}
#[test]
fn private_state_grant_must_be_separate_and_owner_only() {
    let (_root, mut config) = environment();
    config.state_root = config.output_root.clone();
    assert_eq!(
        config.validate().unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    let (_root, config) = environment();
    fs::set_permissions(&config.state_root, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        config.validate().unwrap_err().code,
        ErrorCode::PermissionDenied
    );
}
