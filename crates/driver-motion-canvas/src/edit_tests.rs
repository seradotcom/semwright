//! Candidate Rust regression tests. These have NOT been compiled or executed in
//! the read-only continuation session; see verification/SESSION_REPORT.json.
use crate::{
    Result,
    edit::{MAX_TRANSACTION_OPERATIONS, Operation, apply, prepare},
    model::*,
    refs::{Kind, Reference},
    security, validate,
};
use proptest::prelude::*;
use serde_json::json;
use std::collections::BTreeSet;

fn fixture() -> Project {
    let mut project = Project::empty("motion".into());
    project.generation = "0".repeat(32);
    project.revision = 7;
    project.scenes.push(Scene {
        id: "intro".into(),
        name: "Introduction".into(),
        duration_ms: 2000,
        nodes: vec![text_node("title", "Alpha")],
        animations: vec![],
        cues: vec![],
        transition: None,
    });
    project
}
fn text_node(id: &str, text: &str) -> Node {
    Node {
        id: id.into(),
        name: "title".into(),
        kind: NodeKind::Text,
        parent: None,
        properties: Properties {
            text: Some(text.into()),
            ..Properties::default()
        },
    }
}
fn fingerprint(project: &Project) -> String {
    security::sha256(&serde_json::to_vec(project).unwrap())
}
fn reference(project: &Project, kind: Kind, id: &str) -> String {
    Reference::new(project, &fingerprint(project), kind, id).encode()
}
fn set_text(project: &Project, text: &str) -> Operation {
    Operation::NodePatch {
        node_ref: reference(project, Kind::Node, "title"),
        patch: json!({"text":text}),
        name: None,
    }
}
fn opacity(id: &str, target: &str, duration_ms: u64) -> Animation {
    Animation {
        id: id.into(),
        target: target.into(),
        property: AnimatedProperty::Opacity,
        from: Some(AnimatedValue::Number(0.0)),
        to: AnimatedValue::Number(1.0),
        at: TimeAnchor::default(),
        duration_ms,
        duration_cue: None,
        easing: Easing::Linear,
    }
}
fn run(project: &Project, operations: &[Operation]) -> Result<Project> {
    apply(project, &fingerprint(project), operations)
}

#[test]
fn empty_and_semantic_noop_do_not_advance_revision() {
    let project = fixture();
    assert_eq!(run(&project, &[]).unwrap(), project);
    assert_eq!(
        run(&project, &[set_text(&project, "Alpha")]).unwrap(),
        project
    );
    assert_eq!(
        run(
            &project,
            &[Operation::SettingsPatch {
                patch: json!({"fps":30})
            }]
        )
        .unwrap(),
        project
    );
}

#[test]
fn successful_batch_changes_one_revision_and_leaves_input_untouched() {
    let project = fixture();
    let before = project.clone();
    let edited = run(
        &project,
        &[set_text(&project, "Beta"), set_text(&project, "Gamma")],
    )
    .unwrap();
    assert_eq!(project, before);
    assert_eq!(edited.revision, project.revision + 1);
    assert_eq!(edited.generation, project.generation);
    assert_eq!(
        edited.scenes[0].nodes[0].properties.text.as_deref(),
        Some("Gamma")
    );
    let old = Reference::decode(&reference(&project, Kind::Node, "title")).unwrap();
    assert!(
        old.check(&edited, &fingerprint(&edited), Kind::Node)
            .is_err()
    );
}

#[test]
fn later_failure_rolls_back_the_whole_batch_including_prior_successes() {
    let project = fixture();
    let before = serde_json::to_vec(&project).unwrap();
    let bad = Operation::NodePatch {
        node_ref: reference(&project, Kind::Node, "title"),
        patch: json!({"execute_js":null}),
        name: None,
    };
    assert!(run(&project, &[set_text(&project, "Changed"), bad]).is_err());
    assert_eq!(serde_json::to_vec(&project).unwrap(), before);
}

#[test]
fn unsupported_null_keys_are_rejected_not_silently_deleted() {
    let project = fixture();
    for patch in [
        json!({"shell":null}),
        json!({"package":null}),
        json!({"source":null}),
    ] {
        assert!(
            run(
                &project,
                &[Operation::NodePatch {
                    node_ref: reference(&project, Kind::Node, "title"),
                    patch,
                    name: None,
                }]
            )
            .is_err()
        );
    }
}

#[test]
fn optional_null_resets_a_known_property() {
    let mut project = fixture();
    project.scenes[0].nodes[0].properties.opacity = Some(0.5);
    let edited = run(
        &project,
        &[Operation::NodePatch {
            node_ref: reference(&project, Kind::Node, "title"),
            patch: json!({"opacity":null}),
            name: None,
        }],
    )
    .unwrap();
    assert!(edited.scenes[0].nodes[0].properties.opacity.is_none());
}

#[test]
fn required_setting_null_and_unknown_nested_layout_key_are_rejected() {
    let project = fixture();
    assert!(
        run(
            &project,
            &[Operation::SettingsPatch {
                patch: json!({"fps":null})
            }]
        )
        .is_err()
    );
    let mut project = fixture();
    project.scenes[0].nodes[0].kind = NodeKind::Layout;
    project.scenes[0].nodes[0].properties = Properties::default();
    assert!(
        run(
            &project,
            &[Operation::NodePatch {
                node_ref: reference(&project, Kind::Node, "title"),
                patch: json!({"layout":{"direction":"row","gap":10,"eval":null}}),
                name: None,
            }]
        )
        .is_err()
    );
}

#[test]
fn operation_count_and_serialized_budget_are_bounded() {
    let project = fixture();
    let operations =
        vec![Operation::SettingsPatch { patch: json!({}) }; MAX_TRANSACTION_OPERATIONS + 1];
    assert!(run(&project, &operations).is_err());
    assert!(
        run(
            &project,
            &[set_text(&project, &"a".repeat(MAX_PROJECT_BYTES + 1))]
        )
        .is_err()
    );
    assert!(apply(&project, "not-a-digest", &[]).is_err());
}

#[test]
fn revision_exhaustion_is_not_wrapped_or_hidden_by_a_noop() {
    let mut project = fixture();
    project.revision = u64::MAX;
    assert!(run(&project, &[set_text(&project, "Changed")]).is_err());
    assert_eq!(
        run(&project, &[set_text(&project, "Alpha")])
            .unwrap()
            .revision,
        u64::MAX
    );
}

#[test]
fn altered_source_fingerprint_is_a_stale_reference() {
    let project = fixture();
    let result = apply(&project, &"a".repeat(64), &[set_text(&project, "Changed")]);
    assert!(result.is_err());
}

#[test]
fn remove_then_recreate_cannot_resurrect_an_original_node_id() {
    let project = fixture();
    let operations = [
        Operation::NodeRemove {
            node_ref: reference(&project, Kind::Node, "title"),
            cascade: false,
        },
        Operation::NodeCreate {
            scene_ref: reference(&project, Kind::Scene, "intro"),
            node: text_node("title", "Replacement"),
        },
    ];
    assert!(run(&project, &operations).is_err());
    assert_eq!(
        project.scenes[0].nodes[0].properties.text.as_deref(),
        Some("Alpha")
    );
}

#[test]
fn same_display_name_does_not_alias_node_identity() {
    let project = fixture();
    let edited = run(
        &project,
        &[
            Operation::NodeCreate {
                scene_ref: reference(&project, Kind::Scene, "intro"),
                node: text_node("different", "Other"),
            },
            set_text(&project, "Original only"),
        ],
    )
    .unwrap();
    assert_eq!(
        edited.scenes[0].nodes[0].name,
        edited.scenes[0].nodes[1].name
    );
    assert_eq!(
        edited.scenes[0].nodes[1].properties.text.as_deref(),
        Some("Other")
    );
}

#[test]
fn original_ref_does_not_target_a_removed_scene() {
    let project = fixture();
    assert!(
        run(
            &project,
            &[
                Operation::SceneRemove {
                    scene_ref: reference(&project, Kind::Scene, "intro")
                },
                set_text(&project, "Gone"),
            ]
        )
        .is_err()
    );
}

#[test]
fn cyclic_reparent_rolls_back() {
    let mut project = fixture();
    project.scenes[0].nodes = vec![
        Node {
            id: "root".into(),
            name: "Root".into(),
            kind: NodeKind::Group,
            parent: None,
            properties: Properties::default(),
        },
        Node {
            id: "child".into(),
            name: "Child".into(),
            kind: NodeKind::Group,
            parent: Some("root".into()),
            properties: Properties::default(),
        },
    ];
    let before = project.clone();
    assert!(
        run(
            &project,
            &[Operation::NodeReparent {
                node_ref: reference(&project, Kind::Node, "root"),
                parent_ref: Some(reference(&project, Kind::Node, "child")),
            }]
        )
        .is_err()
    );
    assert_eq!(project, before);
}

#[test]
fn sibling_reorder_is_exact_and_leaves_other_slots_unchanged() {
    let mut project = fixture();
    let root = Node {
        id: "root".into(),
        name: "Root".into(),
        kind: NodeKind::Group,
        parent: None,
        properties: Properties::default(),
    };
    project.scenes[0].nodes[0].parent = Some("root".into());
    project.scenes[0].nodes.insert(0, root);
    let mut other = text_node("other", "Other");
    other.parent = Some("root".into());
    project.scenes[0].nodes.push(other);
    project.scenes[0].nodes.push(text_node("free", "Free"));
    let edited = run(
        &project,
        &[Operation::NodeReorder {
            scene_ref: reference(&project, Kind::Scene, "intro"),
            parent_ref: Some(reference(&project, Kind::Node, "root")),
            order: vec!["other".into(), "title".into()],
        }],
    )
    .unwrap();
    assert_eq!(
        edited.scenes[0]
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>(),
        vec!["root", "other", "title", "free"]
    );
    assert!(
        run(
            &project,
            &[Operation::NodeReorder {
                scene_ref: reference(&project, Kind::Scene, "intro"),
                parent_ref: Some(reference(&project, Kind::Node, "root")),
                order: vec!["title".into(), "title".into()],
            }]
        )
        .is_err()
    );
}

#[test]
fn scene_duplicate_remaps_nodes_edges_cues_animations_and_camera_focus() {
    let mut project = fixture();
    project.scenes[0].nodes.push(text_node("other", "Other"));
    project.scenes[0].nodes.push(Node {
        id: "camera".into(),
        name: "Camera".into(),
        kind: NodeKind::Camera,
        parent: None,
        properties: Properties::default(),
    });
    project.scenes[0].nodes.push(Node {
        id: "edge".into(),
        name: "Edge".into(),
        kind: NodeKind::Line,
        parent: None,
        properties: Properties {
            edge: Some(Edge {
                from: "title".into(),
                to: "other".into(),
            }),
            ..Properties::default()
        },
    });
    project.scenes[0].cues.push(Cue {
        id: "beat".into(),
        name: "Beat".into(),
        time_ms: 100,
        duration_ms: 300,
    });
    project.scenes[0].animations.push(Animation {
        id: "focus".into(),
        target: "camera".into(),
        property: AnimatedProperty::CameraFocus,
        from: None,
        to: AnimatedValue::Text("title".into()),
        at: TimeAnchor {
            cue: Some("beat".into()),
            offset_ms: 0,
        },
        duration_ms: 300,
        duration_cue: Some("beat".into()),
        easing: Easing::Linear,
    });
    let operation = Operation::SceneDuplicate {
        scene_ref: reference(&project, Kind::Scene, "intro"),
        new_id: "duplicate".into(),
        name: "Duplicate".into(),
    };
    let a = run(&project, std::slice::from_ref(&operation)).unwrap();
    let b = run(&project, &[operation]).unwrap();
    assert_eq!(a, b);
    let copied = &a.scenes[1];
    let old_ids = project.scenes[0]
        .nodes
        .iter()
        .map(|n| n.id.clone())
        .collect::<BTreeSet<_>>();
    assert!(copied.nodes.iter().all(|n| !old_ids.contains(&n.id)));
    assert_ne!(copied.cues[0].id, "beat");
    assert_eq!(
        copied.animations[0].at.cue.as_deref(),
        Some(copied.cues[0].id.as_str())
    );
    assert_eq!(
        copied.animations[0].duration_cue,
        copied.animations[0].at.cue
    );
    validate::project_valid(&a).unwrap();
}

#[test]
fn cascade_removes_incident_edge_chains_and_dependent_animations() {
    let mut project = fixture();
    project.scenes[0].nodes.push(text_node("other", "Other"));
    for (id, from, to) in [("e1", "title", "other"), ("e2", "e1", "other")] {
        project.scenes[0].nodes.push(Node {
            id: id.into(),
            name: id.into(),
            kind: NodeKind::Line,
            parent: None,
            properties: Properties {
                edge: Some(Edge {
                    from: from.into(),
                    to: to.into(),
                }),
                ..Properties::default()
            },
        });
    }
    project.scenes[0]
        .animations
        .push(opacity("fade", "title", 500));
    assert!(
        run(
            &project,
            &[Operation::NodeRemove {
                node_ref: reference(&project, Kind::Node, "title"),
                cascade: false
            }]
        )
        .is_err()
    );
    let edited = run(
        &project,
        &[Operation::NodeRemove {
            node_ref: reference(&project, Kind::Node, "title"),
            cascade: true,
        }],
    )
    .unwrap();
    assert_eq!(
        edited.scenes[0]
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>(),
        vec!["other"]
    );
    assert!(edited.scenes[0].animations.is_empty());
}

#[test]
fn sequential_groups_resolve_cues_offsets_and_stagger_in_integer_time() {
    let mut project = fixture();
    project.scenes[0].cues.push(Cue {
        id: "beat".into(),
        name: "Beat".into(),
        time_ms: 500,
        duration_ms: 100,
    });
    let first = opacity("first", "title", 100);
    let mut second = opacity("second", "title", 100);
    second.at.offset_ms = 50;
    let edited = run(
        &project,
        &[Operation::AnimationGroup {
            scene_ref: reference(&project, Kind::Scene, "intro"),
            animations: vec![first, second],
            mode: GroupMode::Sequence,
            at: TimeAnchor {
                cue: Some("beat".into()),
                offset_ms: 0,
            },
            stagger_ms: 100,
        }],
    )
    .unwrap();
    let scene = &edited.scenes[0];
    assert_eq!(
        validate::animation_times(scene, &scene.animations[0]).unwrap(),
        (500, 600)
    );
    assert_eq!(
        validate::animation_times(scene, &scene.animations[1]).unwrap(),
        (750, 850)
    );
}

#[test]
fn grouped_same_property_overlap_and_child_cues_are_rejected() {
    let project = fixture();
    let group = |animations| Operation::AnimationGroup {
        scene_ref: reference(&project, Kind::Scene, "intro"),
        animations,
        mode: GroupMode::Parallel,
        at: TimeAnchor::default(),
        stagger_ms: 0,
    };
    assert!(
        run(
            &project,
            &[group(vec![
                opacity("a", "title", 200),
                opacity("b", "title", 200)
            ])]
        )
        .is_err()
    );
    let mut animation = opacity("a", "title", 200);
    animation.at.cue = Some("unexpected".into());
    assert!(run(&project, &[group(vec![animation])]).is_err());
}

#[test]
fn moving_or_removing_a_required_cue_cannot_leave_a_dangling_animation() {
    let mut project = fixture();
    project.scenes[0].cues.push(Cue {
        id: "beat".into(),
        name: "Beat".into(),
        time_ms: 100,
        duration_ms: 100,
    });
    let mut animation = opacity("fade", "title", 500);
    animation.at.cue = Some("beat".into());
    project.scenes[0].animations.push(animation);
    assert!(
        run(
            &project,
            &[Operation::CueRemove {
                cue_ref: reference(&project, Kind::Cue, "beat")
            }]
        )
        .is_err()
    );
    assert!(
        run(
            &project,
            &[Operation::CueUpsert {
                scene_ref: reference(&project, Kind::Scene, "intro"),
                cue: Cue {
                    id: "beat".into(),
                    name: "Beat".into(),
                    time_ms: 1900,
                    duration_ms: 100
                },
            }]
        )
        .is_err()
    );
    let edited = run(
        &project,
        &[
            Operation::AnimationRemove {
                animation_ref: reference(&project, Kind::Animation, "fade"),
            },
            Operation::CueRemove {
                cue_ref: reference(&project, Kind::Cue, "beat"),
            },
        ],
    )
    .unwrap();
    assert!(edited.scenes[0].cues.is_empty());
}

#[test]
fn animation_replacement_preserves_identity() {
    let mut project = fixture();
    project.scenes[0]
        .animations
        .push(opacity("fade", "title", 500));
    assert!(
        run(
            &project,
            &[Operation::AnimationPatch {
                animation_ref: reference(&project, Kind::Animation, "fade"),
                animation: opacity("different", "title", 300),
            }]
        )
        .is_err()
    );
    let edited = run(
        &project,
        &[Operation::AnimationPatch {
            animation_ref: reference(&project, Kind::Animation, "fade"),
            animation: opacity("fade", "title", 300),
        }],
    )
    .unwrap();
    assert_eq!(edited.scenes[0].animations[0].duration_ms, 300);
}

#[test]
fn camera_focus_and_diagram_edges_use_existing_revision_bound_refs() {
    let mut project = fixture();
    project.scenes[0].nodes.push(text_node("other", "Other"));
    project.scenes[0].nodes.push(Node {
        id: "camera".into(),
        name: "Camera".into(),
        kind: NodeKind::Camera,
        parent: None,
        properties: Properties::default(),
    });
    let edited = run(
        &project,
        &[
            Operation::CameraFocus {
                camera_ref: reference(&project, Kind::Node, "camera"),
                target_ref: reference(&project, Kind::Node, "title"),
                id: "focus".into(),
                at: TimeAnchor::default(),
                duration_ms: 500,
            },
            Operation::DiagramEdgeCreate {
                scene_ref: reference(&project, Kind::Scene, "intro"),
                id: "edge".into(),
                from_ref: reference(&project, Kind::Node, "title"),
                to_ref: reference(&project, Kind::Node, "other"),
                properties: Properties::default(),
            },
        ],
    )
    .unwrap();
    assert_eq!(
        edited.scenes[0].animations[0].property,
        AnimatedProperty::CameraFocus
    );
    let edge = edited.scenes[0]
        .nodes
        .iter()
        .find(|n| n.id == "edge")
        .unwrap();
    assert_eq!(edge.properties.end_arrow, Some(true));
    assert_eq!(edge.properties.edge.as_ref().unwrap().from, "title");
}

#[test]
fn code_highlight_rejects_text_nodes_and_accepts_bounded_code_selection() {
    let mut project = fixture();
    let operation = |p: &Project| Operation::CodeHighlight {
        node_ref: reference(p, Kind::Node, "title"),
        selection: Some(CodeSelection::Lines { start: 0, end: 0 }),
    };
    assert!(run(&project, &[operation(&project)]).is_err());
    project.scenes[0].nodes[0].kind = NodeKind::Code;
    project.scenes[0].nodes[0].properties = Properties {
        code: Some("let value = 1;".into()),
        ..Properties::default()
    };
    assert!(run(&project, &[operation(&project)]).is_ok());
}

#[test]
fn scalar_and_panel_components_compile_to_ordinary_model_nodes() {
    for component in [
        Component::Title,
        Component::Subtitle,
        Component::Badge,
        Component::Panel,
        Component::TerminalWindow,
        Component::CodePanel,
        Component::ArchitectureNode,
        Component::CapabilityChip,
        Component::MetricCounter,
        Component::AppCard,
        Component::Callout,
        Component::LogoLockup,
    ] {
        let project = fixture();
        let edited = run(
            &project,
            &[Operation::ComponentCreate {
                scene_ref: reference(&project, Kind::Scene, "intro"),
                id: "component".into(),
                component,
                text: "Displayed data only".into(),
                parent_ref: None,
                asset: None,
                properties: Properties::default(),
            }],
        )
        .unwrap();
        assert!(edited.scenes[0].nodes.iter().any(|n| n.id == "component"));
        crate::compiler::compile(&edited).unwrap();
    }
}

#[test]
fn browser_component_requires_a_real_asset_reference_not_fake_chrome_text() {
    let mut project = fixture();
    let make = |p: &Project, asset: Option<String>, text: &str| Operation::ComponentCreate {
        scene_ref: reference(p, Kind::Scene, "intro"),
        id: "browser".into(),
        component: Component::BrowserFrame,
        text: text.into(),
        parent_ref: None,
        asset,
        properties: Properties::default(),
    };
    assert!(run(&project, &[make(&project, None, "")]).is_err());
    project.assets.push(Asset {
        id: "capture".into(),
        kind: AssetKind::Image,
        path: "assets/capture.png".into(),
        sha256: "a".repeat(64),
        bytes: 100,
        dimensions: Some([640, 360]),
        duration_ms: None,
        provenance: None,
        license: None,
    });
    // This is a pure model test, NOT validation that capture.png actually exists.
    assert!(
        run(
            &project,
            &[make(&project, Some("capture".into()), "fake output")]
        )
        .is_err()
    );
    assert!(run(&project, &[make(&project, Some("capture".into()), "")]).is_ok());
}

#[test]
fn architecture_edge_component_requires_the_ref_aware_operation() {
    let project = fixture();
    assert!(
        run(
            &project,
            &[Operation::ComponentCreate {
                scene_ref: reference(&project, Kind::Scene, "intro"),
                id: "edge".into(),
                component: Component::ArchitectureEdge,
                text: String::new(),
                parent_ref: None,
                asset: None,
                properties: Properties::default(),
            }]
        )
        .is_err()
    );
}

#[test]
fn eight_curated_presets_produce_valid_ordinary_animations() {
    for preset in [
        AnimationPreset::Fade,
        AnimationPreset::Slide,
        AnimationPreset::Reveal,
        AnimationPreset::ScalePunch,
        AnimationPreset::TrackingExpansion,
        AnimationPreset::WordReveal,
        AnimationPreset::LineReveal,
        AnimationPreset::Counter,
    ] {
        let mut project = fixture();
        project.scenes[0].nodes[0].properties.text = Some("Alpha beta\ngamma".into());
        let edited = run(
            &project,
            &[Operation::AnimationPreset {
                node_ref: reference(&project, Kind::Node, "title"),
                id: "enter".into(),
                preset,
                at: TimeAnchor {
                    cue: None,
                    offset_ms: 100,
                },
                duration_ms: 900,
                from_number: 0.0,
                to_number: 3.0,
            }],
        )
        .unwrap();
        assert!(!edited.scenes[0].animations.is_empty());
        crate::compiler::compile(&edited).unwrap();
    }
}

#[test]
fn word_reveal_preserves_unicode_and_whitespace_exactly() {
    let mut project = fixture();
    let full = "  Oído 🦀\nSemwright  ";
    project.scenes[0].nodes[0].properties.text = Some(full.into());
    let edited = run(
        &project,
        &[Operation::AnimationPreset {
            node_ref: reference(&project, Kind::Node, "title"),
            id: "words".into(),
            preset: AnimationPreset::WordReveal,
            at: TimeAnchor::default(),
            duration_ms: 900,
            from_number: 0.0,
            to_number: 0.0,
        }],
    )
    .unwrap();
    assert_eq!(
        edited.scenes[0].animations.last().unwrap().to,
        AnimatedValue::Text(full.into())
    );
    let times = edited.scenes[0]
        .animations
        .iter()
        .map(|a| validate::animation_times(&edited.scenes[0], a).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(times.first().unwrap().0, 0);
    assert_eq!(times.last().unwrap().1, 900);
    assert!(times.windows(2).all(|w| w[0].1 == w[1].0));
}

#[test]
fn reveal_budget_and_nonfinite_counter_are_rejected() {
    let mut project = fixture();
    project.scenes[0].nodes[0].properties.text = Some(vec!["a"; 129].join(" "));
    let operation = |preset, from_number| Operation::AnimationPreset {
        node_ref: reference(&project, Kind::Node, "title"),
        id: "preset".into(),
        preset,
        at: TimeAnchor::default(),
        duration_ms: 900,
        from_number,
        to_number: 5.0,
    };
    assert!(run(&project, &[operation(AnimationPreset::WordReveal, 0.0)]).is_err());
    assert!(
        run(
            &project,
            &[operation(AnimationPreset::Counter, f64::INFINITY)]
        )
        .is_err()
    );
}

#[test]
fn prepare_reports_source_impact_without_mutating_the_original() {
    let project = fixture();
    let before = project.clone();
    let prepared = prepare(
        &project,
        &fingerprint(&project),
        &[set_text(&project, "<script>${not_executed}</script>")],
    )
    .unwrap();
    assert!(prepared.render_invalidated);
    assert!(!prepared.diff.is_empty());
    assert!(prepared.generated.files.contains_key("src/project.ts"));
    assert_eq!(project, before);
}

#[test]
fn partial_patch_schema_accepts_one_field_and_rejects_unknown_fields() {
    let schema = serde_json::to_value(schemars::schema_for!(Operation)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(&json!({"op":"settings_patch","patch":{"fps":60}})));
    assert!(validator.is_valid(&json!({"op":"theme_patch","patch":{"font_size":40}})));
    assert!(!validator.is_valid(&json!({"op":"settings_patch","patch":{"eval":null}})));
}

proptest! {
    #[test]
    fn text_roundtrip_preserves_data_and_atomic_revision(text in "[^\\x00]{0,120}") {
        let project=fixture();
        let edited=run(&project,&[set_text(&project,&text)]).unwrap();
        prop_assert_eq!(edited.scenes[0].nodes[0].properties.text.as_deref(),Some(text.as_str()));
        prop_assert_eq!(edited.revision,project.revision+u64::from(text!="Alpha"));
        prop_assert_eq!(project.scenes[0].nodes[0].properties.text.as_deref(),Some("Alpha"));
        prop_assert_eq!(serde_json::from_slice::<Project>(&serde_json::to_vec(&edited).unwrap()).unwrap(),edited);
    }
    #[test]
    fn bounded_creations_keep_unique_ids_and_one_revision(count in 1usize..24) {
        let project=fixture();
        let operations=(0..count).map(|i|Operation::NodeCreate {
            scene_ref:reference(&project,Kind::Scene,"intro"),node:text_node(&format!("node-{i}"),"Data"),
        }).collect::<Vec<_>>();
        let edited=run(&project,&operations).unwrap();
        prop_assert_eq!(edited.scenes[0].nodes.len(),count+1);
        prop_assert_eq!(edited.revision,project.revision+1);
        let unique=edited.scenes[0].nodes.iter().map(|n|&n.id).collect::<BTreeSet<_>>();
        prop_assert_eq!(unique.len(),count+1);
    }
    #[test]
    fn integer_fps_patch_preserves_the_remaining_settings(fps in 1u32..=120) {
        let project=fixture();
        let edited=run(&project,&[Operation::SettingsPatch {patch:json!({"fps":fps})}]).unwrap();
        prop_assert_eq!(edited.settings.fps,fps);
        prop_assert_eq!(edited.settings.width,project.settings.width);
        prop_assert_eq!(edited.settings.height,project.settings.height);
        prop_assert_eq!(edited.scenes,project.scenes);
    }
}
