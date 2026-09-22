mod common;
use semwright_mlt_video::{
    adapters,
    edit::{self, Edit},
    model::Format,
};
fn roundtrip(path: &str) {
    let bytes = common::fixture(path);
    let p = adapters::load(&bytes).unwrap();
    let serialized = adapters::save(&p).unwrap();
    let back = adapters::load(serialized.as_bytes()).unwrap();
    assert_eq!(p.semantic_json(), back.semantic_json());
    assert_eq!(serialized, adapters::save(&back).unwrap());
    if !p.generated {
        assert_eq!(
            semwright_mlt_video::xml::serialize(p.original.as_ref().unwrap()).unwrap(),
            serialized
        );
    }
}
#[test]
fn rt_kdenlive_file_uri_kdenlive() {
    roundtrip("kdenlive/file-uri.kdenlive");
}
#[test]
fn rt_kdenlive_future_kdenlive() {
    roundtrip("kdenlive/future.kdenlive");
}
#[test]
fn rt_kdenlive_multiple_kdenlive() {
    roundtrip("kdenlive/multiple.kdenlive");
}
#[test]
fn rt_kdenlive_network_kdenlive() {
    roundtrip("kdenlive/network.kdenlive");
}
#[test]
fn rt_kdenlive_path_traversal_kdenlive() {
    roundtrip("kdenlive/path-traversal.kdenlive");
}
#[test]
fn rt_kdenlive_proxies_kdenlive() {
    roundtrip("kdenlive/proxies.kdenlive");
}
#[test]
fn rt_kdenlive_simple_kdenlive() {
    roundtrip("kdenlive/simple.kdenlive");
}
#[test]
fn rt_kdenlive_unknown_effect_kdenlive() {
    roundtrip("kdenlive/unknown-effect.kdenlive");
}
#[test]
fn rt_mlt_cdata_mlt() {
    roundtrip("mlt/cdata.mlt");
}
#[test]
fn rt_mlt_comments_mlt() {
    roundtrip("mlt/comments.mlt");
}
#[test]
fn rt_mlt_effects_mlt() {
    roundtrip("mlt/effects.mlt");
}
#[test]
fn rt_mlt_gap_mlt() {
    roundtrip("mlt/gap.mlt");
}
#[test]
fn rt_mlt_injection_metadata_mlt() {
    roundtrip("mlt/injection-metadata.mlt");
}
#[test]
fn rt_mlt_multitrack_mlt() {
    roundtrip("mlt/multitrack.mlt");
}
#[test]
fn rt_mlt_namespace_mlt() {
    roundtrip("mlt/namespace.mlt");
}
#[test]
fn rt_mlt_one_frame_mlt() {
    roundtrip("mlt/one-frame.mlt");
}
#[test]
fn rt_mlt_rate_24_1_mlt() {
    roundtrip("mlt/rate-24-1.mlt");
}
#[test]
fn rt_mlt_rate_24000_1001_mlt() {
    roundtrip("mlt/rate-24000-1001.mlt");
}
#[test]
fn rt_mlt_rate_25_1_mlt() {
    roundtrip("mlt/rate-25-1.mlt");
}
#[test]
fn rt_mlt_rate_30_1_mlt() {
    roundtrip("mlt/rate-30-1.mlt");
}
#[test]
fn rt_mlt_rate_30000_1001_mlt() {
    roundtrip("mlt/rate-30000-1001.mlt");
}
#[test]
fn rt_mlt_rate_50_1_mlt() {
    roundtrip("mlt/rate-50-1.mlt");
}
#[test]
fn rt_mlt_rate_60_1_mlt() {
    roundtrip("mlt/rate-60-1.mlt");
}
#[test]
fn rt_mlt_rate_60000_1001_mlt() {
    roundtrip("mlt/rate-60000-1001.mlt");
}
#[test]
fn rt_mlt_simple_mlt() {
    roundtrip("mlt/simple.mlt");
}
#[test]
fn rt_mlt_transition_mlt() {
    roundtrip("mlt/transition.mlt");
}
#[test]
fn rt_mlt_unknown_element_mlt() {
    roundtrip("mlt/unknown-element.mlt");
}
#[test]
fn rt_mlt_unknown_property_mlt() {
    roundtrip("mlt/unknown-property.mlt");
}
#[test]
fn rt_shotcut_annotations_mlt() {
    roundtrip("shotcut/annotations.mlt");
}
#[test]
fn rt_shotcut_export_job_xml() {
    roundtrip("shotcut/export-job.xml");
}
#[test]
fn rt_shotcut_future_mlt() {
    roundtrip("shotcut/future.mlt");
}
#[test]
fn rt_shotcut_simple_mlt() {
    roundtrip("shotcut/simple.mlt");
}
#[test]
fn rt_shotcut_virtual_mlt() {
    roundtrip("shotcut/virtual.mlt");
}
#[test]
fn kdenlive_multiple_sequences_not_flattened() {
    let p = adapters::load(&common::fixture("kdenlive/multiple.kdenlive")).unwrap();
    assert!(p.sequences.len() > 1);
}
#[test]
fn kdenlive_track_rename_requires_risk_acknowledgement() {
    let p = adapters::load(&common::fixture("kdenlive/simple.kdenlive")).unwrap();
    let s = &p.sequences[0];
    let t = &s.tracks[0];
    let edit = Edit::TrackRename {
        sequence: s.id.clone(),
        track: t.id.clone(),
        name: "Renamed".into(),
    };
    assert!(
        edit::plan(
            &p,
            &edit::revision(&p).unwrap(),
            edit.clone(),
            "rename",
            false
        )
        .is_err()
    );
    let plan = edit::plan(&p, &edit::revision(&p).unwrap(), edit, "rename", true).unwrap();
    assert_eq!(plan.result.sequences[0].tracks[0].name, "Renamed");
    assert_eq!(plan.support, adapters::Support::MetadataRisk);
}
#[test]
fn shotcut_track_rename_keeps_annotations() {
    let p = adapters::load(&common::fixture("shotcut/annotations.mlt")).unwrap();
    let s = &p.sequences[0];
    let t = &s.tracks[0];
    let edit = Edit::TrackRename {
        sequence: s.id.clone(),
        track: t.id.clone(),
        name: "New name".into(),
    };
    let r = edit::plan(&p, &edit::revision(&p).unwrap(), edit, "rename", true).unwrap();
    assert_eq!(r.result.sequences[0].tracks[0].name, "New name");
    assert_eq!(
        p.sequences[0].tracks[0].lanes[0].clips[0].effects,
        r.result.sequences[0].tracks[0].lanes[0].clips[0].effects
    );
}
#[test]
fn future_kdenlive_version_is_read_only() {
    let p = adapters::load(&common::fixture("kdenlive/future.kdenlive")).unwrap();
    assert_eq!(
        adapters::adapter(p.format).supported_mutation(&p, "track.rename"),
        adapters::Support::Unsupported
    );
}
#[test]
fn future_shotcut_version_is_read_only() {
    let p = adapters::load(&common::fixture("shotcut/future.mlt")).unwrap();
    assert_eq!(
        adapters::adapter(p.format).supported_mutation(&p, "track.rename"),
        adapters::Support::Unsupported
    );
}
#[test]
fn native_frame_mutation_not_advertised() {
    for file in ["kdenlive/simple.kdenlive", "shotcut/simple.mlt"] {
        let p = adapters::load(&common::fixture(file)).unwrap();
        assert_eq!(
            adapters::adapter(p.format).supported_mutation(&p, "clip.trim"),
            adapters::Support::Unsupported
        );
    }
}
#[test]
fn export_job_is_not_editable_shotcut_project() {
    let p = adapters::load(&common::fixture("shotcut/export-job.xml")).unwrap();
    assert_eq!(p.format, Format::ShotcutExport);
}
#[test]
fn virtual_composition_has_separate_format() {
    let p = adapters::load(&common::fixture("shotcut/virtual.mlt")).unwrap();
    assert_eq!(p.format, Format::ShotcutVirtual);
}
#[test]
fn no_extension_only_detection() {
    let bytes = common::fixture("kdenlive/simple.kdenlive");
    assert_eq!(adapters::load(&bytes).unwrap().format, Format::Kdenlive);
}
#[test]
fn unknown_plugin_metadata_preserved_without_execution() {
    let p = adapters::load(&common::fixture("kdenlive/unknown-effect.kdenlive")).unwrap();
    assert!(!p.generated);
    roundtrip("kdenlive/unknown-effect.kdenlive");
}
#[test]
fn proxy_original_relationship_remains_in_xml() {
    let p = adapters::load(&common::fixture("kdenlive/proxies.kdenlive")).unwrap();
    assert!(p.assets.values().any(|a| a.proxy.is_some()));
    roundtrip("kdenlive/proxies.kdenlive");
}

#[test]
fn rt_shotcut_structured_markers_mlt() {
    roundtrip("shotcut/structured-markers.mlt");
}
