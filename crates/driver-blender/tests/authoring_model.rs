use proptest::prelude::*;
use semwright_driver_blender::authoring::*;
use semwright_media_time::Rate;
use semwright_project_graph as graph;
use semwright_semantic_composition::*;
use std::collections::BTreeMap;

fn spec() -> BlenderAuthoringSpec {
    serde_json::from_str(include_str!(
        "../../../fixtures/blender-authoring/hard_surface.json"
    ))
    .unwrap()
}
#[test]
fn valid_hard_surface() {
    spec().validate().unwrap();
}
#[test]
fn valid_articulated() {
    let s: BlenderAuthoringSpec = serde_json::from_str(include_str!(
        "../../../fixtures/blender-authoring/articulated.json"
    ))
    .unwrap();
    s.validate().unwrap();
}
#[test]
fn valid_product_scene() {
    let s: BlenderAuthoringSpec = serde_json::from_str(include_str!(
        "../../../fixtures/blender-authoring/product_scene.json"
    ))
    .unwrap();
    s.validate().unwrap();
}
#[test]
fn public_plan_transport_schema_fits_registry_budget_and_preserves_strict_decode() {
    let schema = plan_transport_input_schema();
    semwright_registry::bounds::schema_budget(&schema, true).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();

    let create = serde_json::json!({
        "intent":{
            "kind":"create",
            "spec":serde_json::to_value(spec()).unwrap()
        }
    });
    assert!(validator.is_valid(&create));
    let decoded: AuthoringIntent = serde_json::from_value(create["intent"].clone()).unwrap();
    assert!(matches!(decoded, AuthoringIntent::Create { .. }));

    let digest = Digest::of_bytes(b"transport-transform");
    let transform = serde_json::json!({
        "intent":{
            "kind":"transform",
            "island":"managed_island",
            "entity":"part",
            "transform":{
                "translation":[1.0,2.0,3.0],
                "rotation":[0.0,0.0,0.0],
                "scale":[1.0,1.0,1.0]
            },
            "meters_per_unit":1.0,
            "expected_fingerprint":digest.as_str()
        }
    });
    assert!(validator.is_valid(&transform));
    let decoded: AuthoringIntent = serde_json::from_value(transform["intent"].clone()).unwrap();
    assert!(matches!(decoded, AuthoringIntent::Transform { .. }));

    let mut nested_unknown = create;
    nested_unknown["intent"]["spec"]["entities"][0]["python"] = "print(1)".into();
    assert!(
        serde_json::from_value::<AuthoringIntent>(nested_unknown["intent"].clone()).is_err(),
        "compact registry envelope never replaces strict typed decoding"
    );
}
#[test]
fn valid_collision_false_positive_fixture() {
    let s: BlenderAuthoringSpec = serde_json::from_str(include_str!(
        "../../../fixtures/blender-authoring/collision_false_positive.json"
    ))
    .unwrap();
    s.validate().unwrap();
}
#[test]
fn no_executable_escape_or_unknown_fields() {
    let mut value = serde_json::to_value(spec()).unwrap();
    value["python"] = "print(1)".into();
    assert!(serde_json::from_value::<BlenderAuthoringSpec>(value).is_err());
}
#[test]
fn duplicate_json_keys_are_not_a_spec() {
    assert!(strict_decode::<BlenderAuthoringSpec>(br#"{"version":1,"version":2}"#).is_err());
}
#[test]
fn duplicate_identity_rejected() {
    let mut s = spec();
    s.entities.push(s.entities[0].clone());
    assert!(s.validate().is_err());
}
#[test]
fn missing_material_rejected() {
    let mut s = spec();
    s.entities[0].materials = vec!["absent".into()];
    assert!(s.validate().is_err());
}
#[test]
fn missing_relation_rejected() {
    let mut s = spec();
    s.relations.push(Relation::Parent {
        child: s.entities[0].id.clone(),
        parent: "absent".into(),
    });
    assert!(s.validate().is_err());
}
#[test]
fn self_relation_rejected() {
    let mut s = spec();
    let id = s.entities[0].id.clone();
    s.relations.push(Relation::Parent {
        child: id.clone(),
        parent: id,
    });
    assert!(s.validate().is_err());
}
#[test]
fn multiple_parents_rejected() {
    let mut s = spec();
    let rel = Relation::Parent {
        child: s.entities[0].id.clone(),
        parent: s.entities[1].id.clone(),
    };
    s.relations.extend([rel.clone(), rel]);
    assert!(s.validate().is_err());
}
#[test]
fn graph_cycles_rejected() {
    assert!(
        dag_order(&BTreeMap::from([
            ("a".into(), vec!["b".into()]),
            ("b".into(), vec!["a".into()])
        ]))
        .is_err()
    );
}
#[test]
fn graph_missing_rejected() {
    assert!(dag_order(&BTreeMap::from([("a".into(), vec!["b".into()])])).is_err());
}
#[test]
fn graph_is_deterministic() {
    let g = BTreeMap::from([("b".into(), vec!["a".into()]), ("a".into(), vec![])]);
    assert_eq!(dag_order(&g).unwrap(), ["a", "b"]);
}
#[test]
fn nonfinite_rejected() {
    let mut s = spec();
    s.entities[0].transform.scale[0] = f64::NAN;
    assert!(s.validate().is_err());
}
#[test]
fn zero_scale_rejected() {
    let mut s = spec();
    s.entities[0].transform.scale[0] = 0.0;
    assert!(s.validate().is_err());
}
#[test]
fn negative_scale_is_explicit_not_lost() {
    let mut s = spec();
    s.entities[0].transform.scale[0] = -1.0;
    s.validate().unwrap();
}
#[test]
fn invalid_topology_index_rejected() {
    let mut s = spec();
    s.entities[0].shape = Shape::Mesh {
        vertices: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        faces: vec![vec![0, 1, 3]],
        uv: None,
    };
    assert!(s.validate().is_err());
}
#[test]
fn collapsed_face_rejected() {
    let mut s = spec();
    s.entities[0].shape = Shape::Mesh {
        vertices: vec![[0., 0., 0.], [1., 0., 0.], [2., 0., 0.]],
        faces: vec![vec![0, 1, 2]],
        uv: None,
    };
    assert!(s.validate().is_err());
}
#[test]
fn bad_uv_length_rejected() {
    let mut s = spec();
    s.entities[0].shape = Shape::Mesh {
        vertices: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        faces: vec![vec![0, 1, 2]],
        uv: Some(vec![[0., 0.]]),
    };
    assert!(s.validate().is_err());
}
#[test]
fn modifier_explosion_rejected() {
    let mut s = spec();
    s.entities[0].modifiers = vec![
        Modifier::Array {
            count: 16,
            offset: [1., 0., 0.]
        };
        8
    ];
    assert!(s.validate().is_err());
}
#[test]
fn unit_bounds_rejected() {
    let mut s = spec();
    s.meters_per_unit = 0.0;
    assert!(s.validate().is_err());
}
#[test]
fn bone_cycle_rejected() {
    let mut s = spec();
    s.entities[0].materials.clear();
    s.entities[0].modifiers.clear();
    s.entities[0].shape = Shape::Armature {
        bones: vec![
            Bone {
                id: "a".into(),
                head: [0.; 3],
                tail: [0., 0., 1.],
                parent: Some("b".into()),
            },
            Bone {
                id: "b".into(),
                head: [0., 0., 1.],
                tail: [0., 0., 2.],
                parent: Some("a".into()),
            },
        ],
    };
    assert!(s.validate().is_err());
}
#[test]
fn duplicate_keys_rejected() {
    let mut s = spec();
    s.animation = Some(Animation {
        id: "move".into(),
        rate: Rate::new(24, 1).unwrap(),
        channels: vec![Channel {
            entity: s.entities[0].id.clone(),
            bone: None,
            property: AnimatedProperty::Translation,
            keys: vec![
                Key {
                    frame: 1,
                    value: [0.; 3],
                },
                Key {
                    frame: 1,
                    value: [1.; 3],
                },
            ],
        }],
        nla_tracks: vec![],
    });
    assert!(s.validate().is_err());
}
#[test]
fn instance_cannot_mutate_shared_material_slots() {
    let mut s = spec();
    s.entities[1].shape = Shape::MeshInstance {
        source: s.entities[0].id.clone(),
    };
    s.entities[1].materials = vec![s.materials[0].id.clone()];
    assert!(s.validate().is_err());
}

#[test]
fn curve_requires_bounded_generated_geometry() {
    let mut s = spec();
    let cable = s
        .entities
        .iter_mut()
        .find(|entity| entity.id == "cable")
        .unwrap();
    cable.shape = Shape::Curve {
        points: vec![[0., 0., 0.], [1., 0., 0.]],
        cyclic: false,
        extrude: 0.0,
        bevel_depth: 0.0,
        bevel_resolution: 0,
    };
    assert!(s.validate().is_err());
}
#[test]
fn boolean_missing_target_is_rejected() {
    let mut s = spec();
    let housing = s
        .entities
        .iter_mut()
        .find(|entity| entity.id == "housing")
        .unwrap();
    housing.modifiers.push(Modifier::Boolean {
        operation: BooleanOperation::Difference,
        target: "missing".into(),
    });
    assert!(s.validate().is_err());
}
#[test]
fn boolean_dependencies_are_acyclic_and_topological() {
    let s = spec();
    let housing = s
        .entities
        .iter()
        .find(|entity| entity.id == "housing")
        .unwrap();
    assert!(housing.dependency_ids().contains(&"cutter".to_string()));
    let mut cyclic = s.clone();
    let cutter = cyclic
        .entities
        .iter_mut()
        .find(|entity| entity.id == "cutter")
        .unwrap();
    cutter.modifiers.push(Modifier::Boolean {
        operation: BooleanOperation::Union,
        target: "housing".into(),
    });
    assert!(cyclic.validate().is_err());
}
#[test]
fn pbr_emission_and_opacity_are_bounded() {
    let s = spec();
    let insert = s
        .materials
        .iter()
        .find(|material| material.id == "insert")
        .unwrap();
    assert_eq!(insert.opacity, 0.82);
    assert_eq!(insert.emission_strength, 0.35);
    let mut invalid = s.clone();
    invalid
        .materials
        .iter_mut()
        .find(|material| material.id == "insert")
        .unwrap()
        .emission_strength = 10.1;
    assert!(invalid.validate().is_err());
}

#[test]
fn texture_resources_are_hash_pinned_and_channel_typed() {
    let s = spec();
    assert_eq!(s.textures.len(), 2);
    s.validate().unwrap();
}
#[test]
fn texture_missing_reference_is_rejected() {
    let mut s = spec();
    s.materials
        .iter_mut()
        .find(|material| material.id == "insert")
        .unwrap()
        .normal_texture
        .as_mut()
        .unwrap()
        .texture = "missing".into();
    assert!(s.validate().is_err());
}
#[test]
fn scalar_texture_requires_explicit_component() {
    let mut s = spec();
    s.materials
        .iter_mut()
        .find(|material| material.id == "insert")
        .unwrap()
        .roughness_texture
        .as_mut()
        .unwrap()
        .channel = TextureChannel::Color;
    assert!(s.validate().is_err());
}
#[test]
fn normal_texture_requires_non_color_space() {
    let mut s = spec();
    s.textures
        .iter_mut()
        .find(|texture| texture.id == "surface_data")
        .unwrap()
        .color_space = TextureColorSpace::Srgb;
    assert!(s.validate().is_err());
}

fn owner() -> Owner {
    Owner {
        session: "host-session".into(),
        principal: PrincipalBinding::HostSession,
    }
}
fn snap() -> NativeSnapshot {
    NativeSnapshot {
        native_session: "native-boot".into(),
        island: None,
        fingerprint: Digest::of_bytes(b"before"),
        drift: false,
        total: 0,
        items: vec![],
        source_only: true,
        exhaustive: true,
    }
}
fn bindings() -> Vec<CapabilityBinding> {
    vec![CapabilityBinding {
        phase: Phase::Apply,
        command: "driver.blender.composition.apply".into(),
        descriptor: Digest::of_bytes(b"descriptor"),
        effects: [
            EffectClass::CreateOwnedObject,
            EffectClass::UpdateOwnedObject,
        ]
        .into(),
    }]
}
#[test]
fn prepares_common_changeset_without_second_kernel() {
    let s = spec();
    let count = s.operation_count();
    let prepared = prepare(
        owner(),
        AuthoringIntent::Create { spec: s },
        &snap(),
        "native_island".into(),
        bindings(),
    )
    .unwrap();
    prepared.plan.verify(&prepared.profile).unwrap();
    assert_eq!(prepared.plan.body.changes.operations.len(), count);
    assert_eq!(
        prepared.plan.body.changes.atomicity,
        Atomicity::NonAtomicSequence
    );
    assert_eq!(
        prepared.plan.body.dependencies["effects.contract"],
        prepared.contract.digest().unwrap()
    );
    assert_eq!(
        prepared.plan.body.required_rules,
        prepared.contract.required_rules()
    );
}
#[test]
fn partial_snapshot_never_prepares() {
    let mut s = snap();
    s.exhaustive = false;
    assert!(
        prepare(
            owner(),
            AuthoringIntent::Create { spec: spec() },
            &s,
            "native_island".into(),
            bindings()
        )
        .is_err()
    );
}
#[test]
fn common_vault_refuses_client_tampering_and_replay() {
    let prepared = prepare(
        owner(),
        AuthoringIntent::Create { spec: spec() },
        &snap(),
        "native_island".into(),
        bindings(),
    )
    .unwrap();
    let p = prepared.plan;
    let mut vault = PlanVault::bounded(16, 8, 64);
    let id = p.digest.as_str();
    vault
        .issue(
            &owner(),
            id,
            &p,
            p.body.budget.clone(),
            p.body.changes.operations.len() as u32,
            None,
            false,
        )
        .unwrap();
    let mut tampered = p.clone();
    tampered.body.changes.operations.clear();
    assert!(vault.matches(&owner(), id, &tampered).is_err());
    let permit = vault.begin(&owner(), id, &p, "request-1").unwrap();
    vault
        .finish(permit, ExecutionStatus::Completed, vec![])
        .unwrap();
    assert!(vault.begin(&owner(), id, &p, "request-2").is_err());
}
fn transform_snapshot() -> NativeSnapshot {
    let item = serde_json::json!({"entity":"part","type":"EMPTY","translation":[1.0,2.0,3.0],"rotation":[0.0,0.0,0.0],"scale":[1.0,1.0,1.0]});
    NativeSnapshot {
        native_session: "native-boot".into(),
        island: Some("island".into()),
        fingerprint: Digest::of_bytes(b"transform"),
        drift: false,
        total: 1,
        items: vec![item],
        source_only: true,
        exhaustive: true,
    }
}
fn transform_prepared(before: &NativeSnapshot) -> PreparedAuthoring {
    prepare(
        owner(),
        AuthoringIntent::Transform {
            island: "island".into(),
            entity: "part".into(),
            transform: Transform {
                translation: [1., 2., 3.],
                rotation: [0.; 3],
                scale: [1.; 3],
            },
            meters_per_unit: 1.0,
            expected_fingerprint: before.fingerprint.clone(),
        },
        before,
        "unused".into(),
        bindings(),
    )
    .unwrap()
}
#[test]
fn trusted_f_adapter_can_pass_native_readback() {
    let before = transform_snapshot();
    let prepared = transform_prepared(&before);
    let evaluation = evaluate_native_effects(
        &prepared,
        "request-1",
        &before,
        &before,
        None,
        ExecutionStatus::Completed,
    )
    .unwrap();
    assert_eq!(evaluation.verdict().unwrap(), Verdict::Pass);
}
#[test]
fn drift_remains_a_required_f_failure() {
    let before = transform_snapshot();
    let prepared = transform_prepared(&before);
    let mut after = before.clone();
    after.drift = true;
    let evaluation = evaluate_native_effects(
        &prepared,
        "request-1",
        &before,
        &after,
        None,
        ExecutionStatus::Completed,
    )
    .unwrap();
    assert_eq!(evaluation.verdict().unwrap(), Verdict::Fail);
}
#[test]
fn root_transform_plan_rejects_observed_drift() {
    let mut before = transform_snapshot();
    before.drift = true;
    let intent = AuthoringIntent::Transform {
        island: "island".into(),
        entity: "part".into(),
        transform: Transform {
            translation: [1., 2., 3.],
            rotation: [0.; 3],
            scale: [1.; 3],
        },
        meters_per_unit: 1.0,
        expected_fingerprint: before.fingerprint.clone(),
    };
    assert!(prepare(owner(), intent, &before, "unused".into(), bindings()).is_err());
}
#[test]
fn repair_plan_is_transform_only_and_inherits_root_budget() {
    let before = transform_snapshot();
    let root = transform_prepared(&before);
    let mut vault = PlanVault::bounded(16, 8, 64);
    vault
        .issue(
            &owner(),
            "root",
            &root.plan,
            root.plan.body.budget.clone(),
            root.plan.body.changes.operations.len() as u32,
            None,
            false,
        )
        .unwrap();
    let permit = vault
        .begin(&owner(), "root", &root.plan, "root-request")
        .unwrap();
    vault
        .finish(permit, ExecutionStatus::Completed, vec![])
        .unwrap();
    let mut drifted = before.clone();
    drifted.drift = true;
    drifted.fingerprint = Digest::of_bytes(b"manual-drift");
    let repair_intent = AuthoringIntent::Transform {
        island: "island".into(),
        entity: "part".into(),
        transform: Transform {
            translation: [1., 2., 3.],
            rotation: [0.; 3],
            scale: [1.; 3],
        },
        meters_per_unit: 1.0,
        expected_fingerprint: drifted.fingerprint.clone(),
    };
    let child = prepare_repair(owner(), repair_intent, &drifted, bindings()).unwrap();
    assert_eq!(child.plan.body.budget, root.plan.body.budget);
    vault
        .issue(
            &owner(),
            "repair",
            &child.plan,
            child.plan.body.budget.clone(),
            child.plan.body.changes.operations.len() as u32,
            Some("root"),
            true,
        )
        .unwrap();
    assert!(
        prepare_repair(
            owner(),
            AuthoringIntent::Create { spec: spec() },
            &snap(),
            bindings()
        )
        .is_err()
    );
}
#[test]
fn c_receipt_requires_host_owned_ids_and_admission() {
    let before = transform_snapshot();
    let prepared = transform_prepared(&before);
    let evaluation = evaluate_native_effects(
        &prepared,
        "request-1",
        &before,
        &before,
        None,
        ExecutionStatus::Completed,
    )
    .unwrap();
    let descriptor = Digest::of_bytes(b"registered-apply-descriptor");
    let runtime = Digest::of_bytes(b"pinned-blender-runtime");
    let context = GraphReceiptContext {
        id: graph::ReceiptId::new(),
        derivation: graph::DerivationId::new(),
        project: graph::ProjectId::new(),
        inputs: vec![],
        outputs: vec![graph::RevisionPin {
            asset: graph::LogicalAssetId::new(),
            revision: graph::AssetRevision::new(),
            fingerprint: graph::Fingerprint {
                bytes: None,
                projection: Some(graph::ProjectionDigest {
                    digest: before.fingerprint.clone(),
                    method: "blender-source-projection-v2".into(),
                    method_version: 1,
                }),
            },
            equivalence: graph::Equivalence::Projection,
        }],
        additional_determinants: vec![],
        descriptor: descriptor.clone(),
        runtime: runtime.clone(),
        completed_unix_ms: 1,
        coverage: graph::Coverage::unknown(),
    };
    let receipt =
        graph_receipt_candidate(context, "request-1", &prepared.plan, evaluation.report).unwrap();
    let adapter = graph::ReceiptAdapter::registered(
        "driver.blender.composition.apply".into(),
        descriptor,
        runtime,
    )
    .unwrap();
    let admitted = adapter
        .admit(&receipt.owner.clone(), "request-1", receipt)
        .unwrap();
    assert_eq!(
        admitted.record().verification.verdict().unwrap(),
        Verdict::Pass
    );
    assert!(!admitted.record().coverage.cache_safe());
}
#[test]
fn nla_strip_range_must_stay_inside_managed_action() {
    let mut value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/blender-authoring/articulated.json"
    ))
    .unwrap();
    value["animation"]["nla_tracks"][0]["strips"][0]["action_frame_end"] = 25.into();
    let spec: BlenderAuthoringSpec = serde_json::from_value(value).unwrap();
    assert!(spec.validate().is_err());
}
#[test]
fn nla_track_requires_entity_with_managed_channels() {
    let mut value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/blender-authoring/articulated.json"
    ))
    .unwrap();
    value["animation"]["nla_tracks"][0]["entity"] = "body".into();
    let spec: BlenderAuthoringSpec = serde_json::from_value(value).unwrap();
    assert!(spec.validate().is_err());
}
#[test]
fn nla_repeat_below_blender_minimum_is_rejected() {
    let mut value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/blender-authoring/articulated.json"
    ))
    .unwrap();
    value["animation"]["nla_tracks"][0]["strips"][0]["repeat"] = 0.05.into();
    let spec: BlenderAuthoringSpec = serde_json::from_value(value).unwrap();
    assert!(spec.validate().is_err());
}
proptest! {
    #[test] fn finite_dimension_wire_normalizes_stably(x in 0.01f64..100.0) { let mut s=spec();s.entities[0].shape=Shape::Box{size:[x,x/2.,x*2.]};s.validate().unwrap();let first=canonical_bytes(&s).unwrap();let decoded:BlenderAuthoringSpec=strict_decode(&first).unwrap();decoded.validate().unwrap();let normalized=canonical_bytes(&decoded).unwrap();let decoded_again:BlenderAuthoringSpec=strict_decode(&normalized).unwrap();prop_assert_eq!(canonical_bytes(&decoded_again).unwrap(),normalized);if let Shape::Box{size}=decoded.entities[0].shape { prop_assert!((size[0]-x).abs() <= 1e-12*x.abs().max(1.0)); } else { prop_assert!(false); }}
    #[test] fn all_out_of_bounds_indices_fail(i in 3u32..u32::MAX) { let mut s=spec();s.entities[0].shape=Shape::Mesh{vertices:vec![[0.,0.,0.],[1.,0.,0.],[0.,1.,0.]],faces:vec![vec![0,1,i]],uv:None};prop_assert!(s.validate().is_err()); }
}
