use super::*;
use model::*;
use proptest::prelude::*;
use serde_json::json;

fn fixture() -> Project {
    let mut p = Project::empty("fixture".into());
    p.generation = "0123456789abcdef0123456789abcdef".into();
    p.scenes.push(Scene {
        id: "main".into(),
        name: "Main".into(),
        duration_ms: 2000,
        nodes: vec![Node {
            id: "title".into(),
            name: "Title".into(),
            kind: NodeKind::Text,
            parent: None,
            properties: Properties {
                text: Some("Semwright".into()),
                ..Default::default()
            },
        }],
        animations: vec![],
        cues: vec![],
        transition: None,
    });
    p
}
#[test]
fn valid_project_roundtrips() {
    let p = fixture();
    let data = serde_json::to_vec(&p).unwrap();
    assert_eq!(validate::parse(&data).unwrap(), p);
}
#[test]
fn deterministic_source_and_inventory() {
    let p = fixture();
    let a = compiler::compile(&p).unwrap();
    let b = compiler::compile(&p).unwrap();
    assert_eq!(a.files, b.files);
    assert_eq!(a.fingerprint().unwrap(), b.fingerprint().unwrap());
    assert!(a.files.contains_key("src/scenes/main.tsx"));
    assert!(a.files.contains_key("package-lock.json"));
}
#[test]
fn strict_semantic_fields() {
    let mut v = serde_json::to_value(fixture()).unwrap();
    v["unknown"] = json!(true);
    assert!(serde_json::from_value::<Project>(v).is_err());
    assert!(serde_json::from_value::<Properties>(json!({"unknown": "data"})).is_err());
    assert!(serde_json::from_value::<NodeKind>(json!("custom")).is_err());
}
#[test]
fn project_version_is_explicit() {
    for version in [0, 2, u32::MAX] {
        let mut p = fixture();
        p.schema_version = version;
        assert!(validate::project_valid(&p).is_err());
    }
}
#[test]
fn unsupported_properties_are_rejected_by_kind() {
    let mut p = fixture();
    p.scenes[0].nodes[0].properties.radius = Some(12.0);
    assert!(validate::project_valid(&p).is_err());
}
#[test]
fn graph_cycle_is_not_a_diagram_cycle() {
    let mut p = fixture();
    p.scenes[0].nodes[0].kind = NodeKind::Group;
    p.scenes[0].nodes[0].properties = Properties::default();
    p.scenes[0].nodes[0].parent = Some("title".into());
    assert!(validate::project_valid(&p).is_err());
}
#[test]
fn duplicate_ids_are_not_display_names() {
    let mut p = fixture();
    let duplicate = p.scenes[0].nodes[0].clone();
    p.scenes[0].nodes.push(duplicate);
    assert!(validate::project_valid(&p).is_err());
}
#[test]
fn unknown_parent_is_rejected() {
    let mut p = fixture();
    p.scenes[0].nodes[0].parent = Some("missing".into());
    assert!(validate::project_valid(&p).is_err());
}
#[test]
fn bounded_dimensions_and_fps() {
    for fps in [0, 121, u32::MAX] {
        let mut p = fixture();
        p.settings.fps = fps;
        assert!(validate::project_valid(&p).is_err());
    }
    let mut p = fixture();
    p.settings.width = u32::MAX;
    assert!(validate::project_valid(&p).is_err());
}
#[test]
fn duration_overflow_is_an_error_not_a_panic() {
    let mut p = fixture();
    p.scenes[0].duration_ms = u64::MAX;
    p.scenes.push(p.scenes[0].clone());
    assert!(validate::project_valid(&p).is_err());
}
#[test]
fn stale_reference_revision_generation_and_hash() {
    let mut p = fixture();
    let hash = security::sha256(&serde_json::to_vec(&p).unwrap());
    let reference = refs::Reference::new(&p, &hash, refs::Kind::Node, "title");
    assert_eq!(
        refs::Reference::decode(&reference.encode()).unwrap(),
        reference
    );
    reference.check(&p, &hash, refs::Kind::Node).unwrap();
    p.revision += 1;
    assert_eq!(
        reference
            .check(&p, &hash, refs::Kind::Node)
            .unwrap_err()
            .code,
        ErrorCode::StaleReference
    );
    p.revision -= 1;
    p.generation = "f".repeat(32);
    assert!(reference.check(&p, &hash, refs::Kind::Node).is_err());
}
#[test]
fn removed_node_ref_is_stale() {
    let mut p = fixture();
    let hash = "a".repeat(64);
    let r = refs::Reference::new(&p, &hash, refs::Kind::Node, "title");
    p.scenes[0].nodes.clear();
    assert_eq!(
        r.check(&p, &hash, refs::Kind::Node).unwrap_err().code,
        ErrorCode::StaleReference
    );
}
#[test]
fn semantic_diff_identifies_properties_without_revision_noise() {
    let p = fixture();
    let mut q = p.clone();
    q.revision += 1;
    assert!(diff::between(&p, &q).unwrap().is_empty());
    q.scenes[0].nodes[0].properties.text = Some("Meaning, not pixels.".into());
    let d = diff::between(&p, &q).unwrap();
    assert_eq!(d.properties_changed.len(), 1);
    assert_eq!(d.properties_changed[0].property, "properties.text");
}
#[test]
fn render_range_is_half_open() {
    let p = fixture();
    let plan = validate::render_plan(
        &p,
        &RenderProfile {
            first_frame: 5,
            end_frame_exclusive: 6,
            scale: RenderScale::Full,
            transparent: true,
            timeout_ms: 10000,
        },
    )
    .unwrap();
    assert_eq!(plan.frame_count, 1);
    assert!(plan.alpha);
    assert_eq!(plan.width, 1920);
}
#[test]
fn rounding_boundaries_are_integer_based() {
    assert_eq!(validate::ms_to_frames(16, 30).unwrap(), 0);
    assert_eq!(validate::ms_to_frames(17, 30).unwrap(), 1);
    assert_eq!(validate::ms_to_frames(500, 1).unwrap(), 1);
    assert_eq!(validate::ms_to_frames(499, 1).unwrap(), 0);
    assert_eq!(validate::seconds(1234), "1.234");
}
#[test]
fn local_svg_subset_is_structural() {
    assert!(
        security::validate_svg(
            r##"<svg viewBox="0 0 10 10"><rect width="10" height="10" fill="#fff"/></svg>"##
        )
        .is_ok()
    );
    assert!(security::validate_svg("<svg><unknown/></svg>").is_err());
    assert!(security::validate_svg("<svg><path unexpected='1'/></svg>").is_err());
}
#[test]
fn math_vocabulary_is_bounded() {
    assert!(security::validate_latex(r"\frac{1}{2} + \alpha").is_ok());
    assert!(security::validate_latex(r"\notInTheVocabulary{1}").is_err());
}
#[test]
fn render_profile_is_bounded() {
    let p = fixture();
    for (first, end) in [(0, 0), (2, 1), (0, 61), (u64::MAX, u64::MAX)] {
        assert!(
            validate::render_plan(
                &p,
                &RenderProfile {
                    first_frame: first,
                    end_frame_exclusive: end,
                    scale: RenderScale::Full,
                    transparent: false,
                    timeout_ms: 10000
                }
            )
            .is_err()
        );
    }
}
#[test]
fn required_managed_fixtures_validate_and_compile() {
    for bytes in [
        include_bytes!("../../../fixtures/motion-canvas/hello-text/semwright-motion.json")
            .as_slice(),
        include_bytes!("../../../fixtures/motion-canvas/architecture-graph/semwright-motion.json")
            .as_slice(),
        include_bytes!("../../../fixtures/motion-canvas/code-morph/semwright-motion.json")
            .as_slice(),
        include_bytes!("../../../fixtures/motion-canvas/camera-pan/semwright-motion.json")
            .as_slice(),
        include_bytes!("../../../fixtures/motion-canvas/audio-cues/semwright-motion.json")
            .as_slice(),
        include_bytes!("../../../fixtures/motion-canvas/transparent-overlay/semwright-motion.json")
            .as_slice(),
        include_bytes!("../../../fixtures/motion-canvas/media-embed/semwright-motion.json")
            .as_slice(),
    ] {
        let project = validate::parse(bytes).unwrap();
        let first = compiler::compile(&project).unwrap();
        let second = compiler::compile(&project).unwrap();
        assert_eq!(first.files, second.files);
    }
}

proptest! {
    #[test]
    fn display_strings_roundtrip_as_json(text in ".{0,2048}") {
        let quoted = security::js_string(&text);
        prop_assert_eq!(serde_json::from_str::<String>(&quoted).unwrap(), text);
    }
    #[test]
    fn text_projects_compile_deterministically(text in ".{0,1024}") {
        let mut p = fixture(); p.scenes[0].nodes[0].properties.text = Some(text);
        let a = compiler::compile(&p).unwrap(); let b = compiler::compile(&p).unwrap();
        prop_assert_eq!(a.files,b.files);
    }
    #[test]
    fn time_conversion_is_monotonic(a in 0u64..600000, b in 0u64..600000, fps in 1u32..121) {
        let lo = a.min(b); let hi = a.max(b);
        prop_assert!(validate::ms_to_frames(lo,fps).unwrap() <= validate::ms_to_frames(hi,fps).unwrap());
    }
    #[test]
    fn generated_refs_roundtrip(revision in 1u64..u64::MAX) {
        let mut p = fixture(); p.revision = revision;
        let r = refs::Reference::new(&p,&"a".repeat(64),refs::Kind::Project,&p.id);
        prop_assert_eq!(refs::Reference::decode(&r.encode()).unwrap(),r);
    }
}
