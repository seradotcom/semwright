use semwright_effect_conformance::Predicate;
use semwright_godot_driver::authoring::native_observation::*;
use semwright_semantic_composition::Digest;
use std::collections::{BTreeMap, BTreeSet};

fn digest(label: &str) -> Digest {
    Digest::of_bytes(label.as_bytes())
}

fn resource(path: &str, instance_id: &str) -> NativeResourceRef {
    NativeResourceRef {
        class: "StandardMaterial3D".into(),
        path: path.into(),
        uid: Some("uid://abc123".into()),
        instance_id: instance_id.into(),
        local_to_scene: false,
    }
}

fn projection(scene_path: &str, node_id: &str, resource_id: &str) -> NativeProjection {
    NativeProjection {
        nodes: vec![NativeNode {
            path: ".".into(),
            class: "Node2D".into(),
            instance_id: node_id.into(),
            parent: None,
            owner: None,
            scene_file: scene_path.into(),
            logical_id: None,
            logical_key: Some("scene:arena".into()),
            groups: vec!["gameplay".into()],
            properties: BTreeMap::from([(
                "material".into(),
                NativeValue::Resource(resource("res://assets/material.tres", resource_id)),
            )]),
        }],
        resources: vec![
            NativeResource {
                binding: "root:material".into(),
                resource: resource("res://assets/material.tres", resource_id),
                properties: BTreeMap::from([("roughness".into(), NativeValue::Float(0.5))]),
            },
            NativeResource {
                binding: "root:material_alias".into(),
                resource: resource("res://assets/material.tres", resource_id),
                properties: BTreeMap::from([("roughness".into(), NativeValue::Float(0.5))]),
            },
        ],
        animations: vec![],
        connections: vec![],
        unknown: vec![],
    }
}

fn observation(
    mode: ProbeMode,
    nonce: &str,
    process_id: &str,
    authored: NativeProjection,
    loaded_scene_sha256: Digest,
    candidate_sha256: Option<Digest>,
) -> NativeObservation {
    NativeObservation {
        version: NATIVE_VERSION,
        nonce: nonce.into(),
        source_fingerprint: digest("source"),
        mode,
        engine_version: "4.7.2.stable.official.test".into(),
        process_id: process_id.into(),
        loaded_scene: if mode == ProbeMode::ReopenCandidate {
            "res://__sw_saved/arena.tscn".into()
        } else {
            "res://scenes/arena.tscn".into()
        },
        loaded_scene_sha256,
        candidate_sha256,
        authored,
        live: None,
        frames: vec![],
        dependencies: vec![NativeDependency {
            source: if matches!(mode, ProbeMode::SaveCandidate | ProbeMode::ReopenCandidate) {
                "res://__sw_saved/arena.tscn".into()
            } else {
                "res://scenes/arena.tscn".into()
            },
            path: "res://assets/material.tres".into(),
            uid: Some("uid://abc123".into()),
            sha256: Some(digest("material-bytes")),
            exists: true,
        }],
        dependency_complete: true,
        inputs_delivered: 0,
        elapsed_physics_frames: 0,
        failures: vec![],
    }
}

#[test]
fn native_request_is_typed_bounded_and_action_scoped() {
    let declared: BTreeSet<String> = ["jump".to_owned()].into();
    let request = NativeRequest {
        version: NATIVE_VERSION,
        nonce: "native_request_0001".into(),
        source_fingerprint: digest("source"),
        mode: ProbeMode::Play,
        scene: "res://scenes/arena.tscn".into(),
        ticks: 2,
        inputs: vec![
            InputStep {
                tick: 1,
                action: "jump".into(),
                pressed: true,
            },
            InputStep {
                tick: 2,
                action: "jump".into(),
                pressed: false,
            },
        ],
        checkpoints: vec![1, 2],
        variables: vec!["score".into()],
        capture: false,
    };
    request.validate(&declared).unwrap();

    let mut undeclared = request.clone();
    undeclared.inputs[0].action = "shell".into();
    undeclared.inputs[1].action = "shell".into();
    assert!(undeclared.validate(&declared).is_err());

    let mut non_play = request;
    non_play.mode = ProbeMode::Inspect;
    assert!(non_play.validate(&declared).is_err());
}

#[test]
fn projection_digest_removes_process_identity_but_preserves_resource_aliases() {
    let first = projection("res://scenes/arena.tscn", "11", "21");
    let second = projection("res://__sw_saved/arena.tscn", "99", "42");
    assert_eq!(
        first.stable_digest().unwrap(),
        second.stable_digest().unwrap()
    );

    let mut broken_alias = second;
    broken_alias.resources[1].resource.instance_id = "43".into();
    assert_ne!(
        first.stable_digest().unwrap(),
        broken_alias.stable_digest().unwrap()
    );
}

#[test]
fn observation_decode_rejects_unknown_wire_fields() {
    let request = NativeRequest {
        version: NATIVE_VERSION,
        nonce: "native_request_0002".into(),
        source_fingerprint: digest("source"),
        mode: ProbeMode::Inspect,
        scene: "res://scenes/arena.tscn".into(),
        ticks: 0,
        inputs: vec![],
        checkpoints: vec![],
        variables: vec![],
        capture: false,
    };
    request.validate(&BTreeSet::new()).unwrap();
    let observed = observation(
        ProbeMode::Inspect,
        &request.nonce,
        "101",
        projection("res://scenes/arena.tscn", "11", "21"),
        digest("scene"),
        None,
    );
    let bytes = serde_json::to_vec(&observed).unwrap();
    decode_observation(&bytes, &request).unwrap();

    let mut value = serde_json::to_value(observed).unwrap();
    value["caller_evidence"] = serde_json::json!(true);
    assert!(decode_observation(&serde_json::to_vec(&value).unwrap(), &request).is_err());
}

fn tracks(count: u32) -> NativeAnimation {
    NativeAnimation {
        player: ".".into(),
        library: "".into(),
        name: "walk".into(),
        root: ".".into(),
        length: 2.0,
        loop_mode: 1,
        resource: NativeResourceRef {
            class: "Animation".into(),
            path: "".into(),
            uid: None,
            instance_id: "300".into(),
            local_to_scene: false,
        },
        track_count: count,
        tracks: (0..count)
            .map(|index| NativeTrack {
                index,
                track_type: 0,
                path: format!("Entity:property_{index}"),
                enabled: true,
                interpolation: 1,
                imported: false,
                key_count: 0,
                keys: vec![],
            })
            .collect(),
    }
}

#[test]
fn signed_nonzero_object_ids_are_admitted_but_zero_and_text_are_rejected() {
    let request = NativeRequest {
        version: NATIVE_VERSION,
        nonce: "native_signed_ids_0001".into(),
        source_fingerprint: digest("source"),
        mode: ProbeMode::Inspect,
        scene: "res://scenes/arena.tscn".into(),
        ticks: 0,
        inputs: vec![],
        checkpoints: vec![],
        variables: vec![],
        capture: false,
    };
    request.validate(&BTreeSet::new()).unwrap();

    let mut observed = observation(
        ProbeMode::Inspect,
        &request.nonce,
        "111",
        projection("res://scenes/arena.tscn", "-9223372036854775807", "-42"),
        digest("scene"),
        None,
    );
    for resource in &mut observed.authored.resources {
        resource.resource.instance_id = "-42".into();
    }
    if let NativeValue::Resource(resource) = observed.authored.nodes[0]
        .properties
        .get_mut("material")
        .unwrap()
    {
        resource.instance_id = "-42".into();
    }
    decode_observation(&serde_json::to_vec(&observed).unwrap(), &request).unwrap();

    let mut zero_node = observed.clone();
    zero_node.authored.nodes[0].instance_id = "0".into();
    assert!(decode_observation(&serde_json::to_vec(&zero_node).unwrap(), &request).is_err());

    let mut zero_resource = observed.clone();
    zero_resource.authored.resources[0].resource.instance_id = "0".into();
    assert!(decode_observation(&serde_json::to_vec(&zero_resource).unwrap(), &request).is_err());

    let mut text_resource = observed;
    text_resource.authored.resources[0].resource.instance_id = "not-an-id".into();
    assert!(decode_observation(&serde_json::to_vec(&text_resource).unwrap(), &request).is_err());
}

#[test]
fn stable_projection_ignores_subresource_container_hash_but_not_external_hash() {
    let mut before = projection("res://scenes/arena.tscn", "11", "21");
    for resource in &mut before.resources {
        resource.resource.path = "res://scenes/arena.tscn::SubResource_mat".into();
        resource
            .properties
            .insert("source_sha256".into(), NativeValue::Text("a".repeat(64)));
    }
    if let NativeValue::Resource(resource) = before.nodes[0].properties.get_mut("material").unwrap()
    {
        resource.path = "res://scenes/arena.tscn::SubResource_mat".into();
    }

    let mut reopened = before.clone();
    reopened.nodes[0].scene_file = "res://__sw_saved/arena.tscn".into();
    for resource in &mut reopened.resources {
        resource.resource.path = "res://__sw_saved/arena.tscn::SubResource_mat".into();
        resource
            .properties
            .insert("source_sha256".into(), NativeValue::Text("b".repeat(64)));
    }
    if let NativeValue::Resource(resource) =
        reopened.nodes[0].properties.get_mut("material").unwrap()
    {
        resource.path = "res://__sw_saved/arena.tscn::SubResource_mat".into();
    }
    assert_eq!(
        before.stable_digest().unwrap(),
        reopened.stable_digest().unwrap(),
        "container bytes/path are checked by reopen/dependency evidence, not semantic projection",
    );

    let mut external_before = projection("res://scenes/arena.tscn", "31", "41");
    external_before.resources[0]
        .properties
        .insert("source_sha256".into(), NativeValue::Text("c".repeat(64)));
    let mut external_after = external_before.clone();
    external_after.resources[0]
        .properties
        .insert("source_sha256".into(), NativeValue::Text("d".repeat(64)));
    assert_ne!(
        external_before.stable_digest().unwrap(),
        external_after.stable_digest().unwrap(),
        "external resource bytes remain part of stable semantic evidence",
    );
}

#[test]
fn native_track_cursor_is_snapshot_and_source_bound_without_truncation() {
    let mut authored = projection("res://scenes/arena.tscn", "11", "21");
    authored.animations.push(tracks(70));
    let observed = observation(
        ProbeMode::Inspect,
        "native_request_0003",
        "102",
        authored,
        digest("scene"),
        None,
    );
    let source = digest("source");
    let first = track_page(&observed, &source, None, 64).unwrap();
    assert_eq!(first.total, 70);
    assert_eq!(first.tracks.len(), 64);
    assert_eq!(first.tracks.last().unwrap().index, 63);
    let mut reobserved = observed.clone();
    reobserved.nonce = "native_request_0004".into();
    reobserved.process_id = "103".into();
    assert_ne!(
        observed.snapshot_digest().unwrap(),
        reobserved.snapshot_digest().unwrap()
    );
    assert_eq!(
        observed.paging_snapshot_digest().unwrap(),
        reobserved.paging_snapshot_digest().unwrap()
    );
    let second = track_page(&reobserved, &source, first.next_cursor.as_deref(), 64).unwrap();
    assert_eq!(second.tracks.len(), 6);
    assert_eq!(second.tracks.last().unwrap().index, 69);
    assert!(second.next_cursor.is_none());
    assert!(track_page(&observed, &digest("changed-source"), None, 64).is_err());

    let mut changed = reobserved.clone();
    changed.authored.animations[0].tracks[69].path = "Entity:changed".into();
    assert!(track_page(&changed, &source, first.next_cursor.as_deref(), 64).is_err());

    let stale = format!("gtr1.{}.64", digest("other-snapshot").as_str());
    assert!(track_page(&observed, &source, Some(&stale), 64).is_err());
}

#[test]
fn native_key_cursor_is_track_bound_and_survives_fresh_process() {
    let mut authored = projection("res://scenes/arena.tscn", "11", "21");
    let mut animation = tracks(1);
    animation.tracks[0].keys = (0..70)
        .map(|index| NativeKey {
            time: f64::from(index) * 2.0 / 69.0,
            transition: 1.0,
            value: NativeValue::Float(f64::from(index)),
        })
        .collect();
    animation.tracks[0].key_count = 70;
    authored.animations.push(animation);
    let observed = observation(
        ProbeMode::Inspect,
        "native_key_request_0001",
        "301",
        authored,
        digest("scene"),
        None,
    );
    let source = digest("source");
    let first = key_page(&observed, &source, ".", "", "walk", 0, None, 64).unwrap();
    assert_eq!(first.total, 70);
    assert_eq!(first.keys.len(), 64);
    assert_eq!(first.keys.last().unwrap().index, 63);
    let cursor = first.next_cursor.clone().unwrap();

    let mut reobserved = observed.clone();
    reobserved.nonce = "native_key_request_0002".into();
    reobserved.process_id = "302".into();
    let second = key_page(&reobserved, &source, ".", "", "walk", 0, Some(&cursor), 64).unwrap();
    assert_eq!(second.snapshot, first.snapshot);
    assert_eq!(second.query_digest, first.query_digest);
    assert_eq!(second.keys.len(), 6);
    assert_eq!(second.keys.last().unwrap().index, 69);
    assert!(second.next_cursor.is_none());

    let mut changed = reobserved.clone();
    changed.authored.animations[0].tracks[0].keys[69].value = NativeValue::Float(999.0);
    assert!(key_page(&changed, &source, ".", "", "walk", 0, Some(&cursor), 64,).is_err());
    assert!(key_page(&observed, &source, ".", "", "missing", 0, None, 64,).is_err());
}

#[test]
fn native_target_query_is_bounded_filtered_and_rejects_ambiguity() {
    let observed = observation(
        ProbeMode::Inspect,
        "native_query_0001",
        "401",
        projection("res://scenes/arena.tscn", "11", "21"),
        digest("scene"),
        None,
    );

    let node = query_projection(
        &observed,
        &NativeQueryTarget::Node {
            logical_key: "scene:arena".into(),
        },
        &["material".into()],
    )
    .unwrap();
    let NativeQueryValue::Node { value: node } = node else {
        panic!("node query returned resource");
    };
    assert_eq!(node.logical_key.as_deref(), Some("scene:arena"));
    assert_eq!(node.properties.len(), 1);
    assert!(matches!(
        node.properties.get("material"),
        Some(NativeValue::Resource(resource))
            if resource.path == "res://assets/material.tres"
    ));

    let resource = query_projection(
        &observed,
        &NativeQueryTarget::Resource {
            path: "res://assets/material.tres".into(),
        },
        &["roughness".into()],
    )
    .unwrap();
    let NativeQueryValue::Resource { value: resource } = resource else {
        panic!("resource query returned node");
    };
    assert_eq!(resource.resource.path, "res://assets/material.tres");
    assert_eq!(resource.properties.len(), 1);
    assert!(matches!(
        resource.properties.get("roughness"),
        Some(NativeValue::Float(value)) if (*value - 0.5).abs() < f64::EPSILON
    ));

    assert!(
        query_projection(
            &observed,
            &NativeQueryTarget::Node {
                logical_key: "scene:arena".into(),
            },
            &["missing".into()],
        )
        .is_err()
    );
    assert!(
        query_projection(
            &observed,
            &NativeQueryTarget::Node {
                logical_key: "scene:arena".into(),
            },
            &["material".into(), "material".into()],
        )
        .is_err()
    );
    assert!(
        query_projection(
            &observed,
            &NativeQueryTarget::Resource {
                path: "../material.tres".into(),
            },
            &[],
        )
        .is_err()
    );

    let mut ambiguous = observed;
    ambiguous.authored.resources[1]
        .properties
        .insert("roughness".into(), NativeValue::Float(0.7));
    assert!(
        query_projection(
            &ambiguous,
            &NativeQueryTarget::Resource {
                path: "res://assets/material.tres".into(),
            },
            &["roughness".into()],
        )
        .is_err()
    );
}

#[test]
fn persistence_requires_fresh_process_and_unchanged_external_sentinels() {
    let candidate = digest("saved-scene");
    let writer = observation(
        ProbeMode::SaveCandidate,
        "native_save_000001",
        "201",
        projection("res://scenes/arena.tscn", "11", "21"),
        digest("source-scene"),
        Some(candidate.clone()),
    );
    let reader = observation(
        ProbeMode::ReopenCandidate,
        "native_reopen_0001",
        "202",
        projection("res://__sw_saved/arena.tscn", "99", "42"),
        candidate,
        None,
    );
    assert_eq!(writer.dependencies[0].source, "res://__sw_saved/arena.tscn");
    assert_eq!(reader.dependencies[0].source, "res://__sw_saved/arena.tscn");
    let evidence = persistence_value(&writer, &reader).unwrap();
    assert_eq!(Predicate::Reopened.compare(&evidence).unwrap(), Some(true));

    let mut changed = reader.clone();
    changed.dependencies[0].sha256 = Some(digest("human-edit"));
    assert!(persistence_value(&writer, &changed).is_err());

    let mut incomplete = reader.clone();
    incomplete.dependency_complete = false;
    assert!(persistence_value(&writer, &incomplete).is_err());

    let mut same_process = reader;
    same_process.process_id = writer.process_id.clone();
    assert!(persistence_value(&writer, &same_process).is_err());
}

#[test]
fn fixed_native_probe_contains_no_arbitrary_execution_surface() {
    assert!(PROBE_SOURCE.contains("ResourceLoader.CACHE_MODE_IGNORE_DEEP"));
    assert!(PROBE_SOURCE.contains("get_signal_connection_list"));
    assert!(PROBE_SOURCE.contains("track_get_key_count"));
    assert!(PROBE_SOURCE.contains("save_png_to_buffer"));
    assert!(PROBE_SOURCE.contains("ResourceLoader.get_dependencies"));
    assert!(PROBE_SOURCE.contains("var dependency_root: String = scene_path"));
    assert!(PROBE_SOURCE.contains("dependency_root = candidate_path"));
    assert!(PROBE_SOURCE.contains("_dependencies(dependency_root)"));
    assert!(!PROBE_SOURCE.contains("_dependencies(scene_path)"));
    assert!(PROBE_SOURCE.contains("resource.get_mesh_arrays() if resource is PrimitiveMesh else resource.surface_get_arrays(index)"));
    assert!(!PROBE_SOURCE.contains("surface_get_array_len("));
    assert!(!PROBE_SOURCE.contains("surface_get_array_index_len("));
    assert!(PROBE_SOURCE.contains("int(checkpoint) == tick"));
    assert!(!PROBE_SOURCE.contains("_request.checkpoints.has(tick)"));
    let play_loop = PROBE_SOURCE.find("for tick in range(").unwrap();
    let live_projection = PROBE_SOURCE
        .find("report.live = _projection(_root_scene)")
        .unwrap();
    assert!(
        live_projection > play_loop,
        "live projection must describe post-play runtime state"
    );
    assert_eq!(
        PROBE_SOURCE
            .matches("report.live = _projection(_root_scene)")
            .count(),
        1
    );
    for forbidden in [
        "OS.execute",
        "Expression.execute",
        ".callv(",
        "JavaScriptBridge",
        "EngineDebugger",
    ] {
        assert!(!PROBE_SOURCE.contains(forbidden), "{forbidden}");
    }
}
