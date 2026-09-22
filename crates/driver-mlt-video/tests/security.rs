mod common;
use semwright_mlt_video::{
    adapters,
    fs::{self, Root},
    json, xml,
};
#[test]
fn reject_ambiguous_format_mlt() {
    assert!(adapters::load(&common::fixture("malformed/ambiguous-format.mlt")).is_err());
}
#[test]
fn reject_attribute_no_space_xml() {
    assert!(adapters::load(&common::fixture("malformed/attribute-no-space.xml")).is_err());
}
#[test]
fn reject_bad_cdata_xml() {
    assert!(adapters::load(&common::fixture("malformed/bad-cdata.xml")).is_err());
}
#[test]
fn reject_bad_comment_xml() {
    assert!(adapters::load(&common::fixture("malformed/bad-comment.xml")).is_err());
}
#[test]
fn reject_bad_numeric_entity_xml() {
    assert!(adapters::load(&common::fixture("malformed/bad-numeric-entity.xml")).is_err());
}
#[test]
fn reject_bad_out_mlt() {
    assert!(adapters::load(&common::fixture("malformed/bad-out.mlt")).is_err());
}
#[test]
fn reject_control_xml() {
    assert!(adapters::load(&common::fixture("malformed/control.xml")).is_err());
}
#[test]
fn reject_cycle_xml() {
    assert!(adapters::load(&common::fixture("malformed/cycle.xml")).is_err());
}
#[test]
fn reject_depth_xml() {
    assert!(adapters::load(&common::fixture("malformed/depth.xml")).is_err());
}
#[test]
fn reject_doctype_xml() {
    assert!(adapters::load(&common::fixture("malformed/doctype.xml")).is_err());
}
#[test]
fn reject_duplicate_attribute_xml() {
    assert!(adapters::load(&common::fixture("malformed/duplicate-attribute.xml")).is_err());
}
#[test]
fn reject_duplicate_id_xml() {
    assert!(adapters::load(&common::fixture("malformed/duplicate-id.xml")).is_err());
}
#[test]
fn reject_encoding_xml() {
    assert!(adapters::load(&common::fixture("malformed/encoding.xml")).is_err());
}
#[test]
fn reject_entity_expansion_xml() {
    assert!(adapters::load(&common::fixture("malformed/entity-expansion.xml")).is_err());
}
#[test]
fn reject_external_entity_xml() {
    assert!(adapters::load(&common::fixture("malformed/external-entity.xml")).is_err());
}
#[test]
fn reject_extra_root_xml() {
    assert!(adapters::load(&common::fixture("malformed/extra-root.xml")).is_err());
}
#[test]
fn reject_huge_attribute_xml() {
    assert!(adapters::load(&common::fixture("malformed/huge-attribute.xml")).is_err());
}
#[test]
fn reject_huge_text_xml() {
    assert!(adapters::load(&common::fixture("malformed/huge-text.xml")).is_err());
}
#[test]
fn reject_invalid_idref_xml() {
    assert!(adapters::load(&common::fixture("malformed/invalid-idref.xml")).is_err());
}
#[test]
fn reject_invalid_utf8_xml() {
    assert!(adapters::load(&common::fixture("malformed/invalid-utf8.xml")).is_err());
}
#[test]
fn reject_mismatch_xml() {
    assert!(adapters::load(&common::fixture("malformed/mismatch.xml")).is_err());
}
#[test]
fn reject_negative_frame_mlt() {
    assert!(adapters::load(&common::fixture("malformed/negative-frame.mlt")).is_err());
}
#[test]
fn reject_parameter_entity_xml() {
    assert!(adapters::load(&common::fixture("malformed/parameter-entity.xml")).is_err());
}
#[test]
fn reject_pi_xml() {
    assert!(adapters::load(&common::fixture("malformed/pi.xml")).is_err());
}
#[test]
fn reject_surrogate_entity_xml() {
    assert!(adapters::load(&common::fixture("malformed/surrogate-entity.xml")).is_err());
}
#[test]
fn reject_unclosed_xml() {
    assert!(adapters::load(&common::fixture("malformed/unclosed.xml")).is_err());
}
#[test]
fn reject_undeclared_prefix_xml() {
    assert!(adapters::load(&common::fixture("malformed/undeclared-prefix.xml")).is_err());
}
#[test]
fn reject_unknown_entity_xml() {
    assert!(adapters::load(&common::fixture("malformed/unknown-entity.xml")).is_err());
}
#[test]
fn reject_zero_fps_mlt() {
    assert!(adapters::load(&common::fixture("malformed/zero-fps.mlt")).is_err());
}
#[test]
fn duplicate_property_names_rejected() {
    assert!(xml::parse(br#"<mlt><producer id="a"><property name="resource">a</property><property name="resource">b</property></producer></mlt>"#).is_err());
}
#[test]
fn graph_shared_subtree_cannot_hide_long_path() {
    let mut body = String::from(r#"<mlt><producer id="a00"/>"#);
    for i in 1..36 {
        body.push_str(&format!(
            r#"<tractor id="a{i:02}"><track producer="a{:02}"/></tractor>"#,
            i - 1
        ));
    }
    body.push_str("</mlt>");
    assert!(xml::parse(body.as_bytes()).is_err());
}
#[test]
fn xml_maximum_size_rejected_before_parse() {
    assert!(xml::parse(&vec![b'a'; xml::MAX_XML + 1]).is_err());
}
#[test]
fn lexical_path_traversal_denied() {
    for p in [
        "../x",
        "/etc/passwd",
        "a//b",
        "a/./b",
        "a/../b",
        "file:///a",
        "https://example.invalid/a",
        "a\\b",
        "\nname",
    ] {
        assert!(fs::validate_relative(p).is_err(), "{p}");
    }
}
#[test]
fn shell_metacharacters_are_filenames_not_programs() {
    for p in [
        "--help.mp4",
        "$(touch pwned).mp4",
        "a;b.mp4",
        "quote\".mp4",
        "a b.mp4",
        "é.mp4",
    ] {
        assert!(fs::validate_relative(p).is_ok(), "{p}");
    }
}
#[test]
fn confined_read_rejects_symlink() {
    let d = common::temp();
    std::os::unix::fs::symlink("/etc/passwd", d.path().join("escape")).unwrap();
    let r = Root::open(d.path(), true, true).unwrap();
    assert!(r.read("escape", 10000).is_err());
}
#[test]
fn confined_read_rejects_hardlink() {
    let d = common::temp();
    let source = d.path().join("a");
    std::fs::write(&source, b"owned").unwrap();
    std::fs::hard_link(&source, d.path().join("b")).unwrap();
    let r = Root::open(d.path(), true, true).unwrap();
    assert!(r.read("b", 10000).is_err());
}
#[test]
fn confined_read_rejects_directory() {
    let d = common::temp();
    std::fs::create_dir(d.path().join("child")).unwrap();
    assert!(
        Root::open(d.path(), true, false)
            .unwrap()
            .read("child", 10000)
            .is_err()
    );
}
#[test]
fn write_denied_by_read_only_grant() {
    let d = common::temp();
    assert!(
        Root::open(d.path(), true, false)
            .unwrap()
            .write_new("output", "file", b"x")
            .is_err()
    );
}
#[test]
fn atomic_save_never_overwrites() {
    let d = common::temp();
    let r = Root::open(d.path(), true, true).unwrap();
    r.write_new("output", "file", b"original").unwrap();
    assert!(r.write_new("output", "file", b"new").is_err());
    assert_eq!(r.read("file", 100).unwrap(), b"original");
}
#[test]
fn atomic_save_rejects_output_symlink() {
    let d = common::temp();
    std::os::unix::fs::symlink("/tmp/semwright-must-not-write", d.path().join("file")).unwrap();
    let r = Root::open(d.path(), true, true).unwrap();
    assert!(r.write_new("output", "file", b"new").is_err());
}
#[test]
fn publication_budget_preserves_originals() {
    let d = common::temp();
    let r = Root::open(d.path(), true, true).unwrap();
    assert!(r.publish("output", "file", &b"abcd"[..], 3).is_err());
    assert!(!d.path().join("file").exists());
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0);
}
#[test]
fn pinned_root_survives_ancestor_rename() {
    let d = common::temp();
    std::fs::create_dir(d.path().join("old")).unwrap();
    let r = Root::open(&d.path().join("old"), true, true).unwrap();
    std::fs::rename(d.path().join("old"), d.path().join("moved")).unwrap();
    std::fs::create_dir(d.path().join("old")).unwrap();
    r.write_new("output", "file", b"pinned").unwrap();
    assert!(!d.path().join("old/file").exists());
    assert!(d.path().join("moved/file").exists());
}
#[test]
fn observed_external_resources_are_not_opened() {
    for fixture in ["path-traversal", "file-uri", "network"] {
        let p = adapters::load(&common::fixture(&format!("kdenlive/{fixture}.kdenlive"))).unwrap();
        assert!(p.assets.values().any(|a| matches!(
            a.resource,
            semwright_mlt_video::model::Resource::External(_)
        )));
    }
}
#[test]
fn metadata_display_not_raw_terminal_escape() {
    let s = json::display("\u{202e} title\u{1b}[31m");
    assert!(!s.contains('\u{202e}'));
    assert!(!s.contains('\u{1b}'));
}

#[test]
fn nested_routing_property_is_not_silently_flattened() {
    assert!(
        semwright_mlt_video::xml::parse(
            b"<mlt><property name='resource'><properties name='x'/></property></mlt>"
        )
        .is_err()
    );
}
