use semwright_types::{NameMatch, NameOp, RelationMatch, Selector, UiNode};
use serde_json::json;

fn parse(value: serde_json::Value) -> UiNode {
    serde_json::from_value(value).expect("semantic UI fixture must deserialize")
}

#[test]
fn linux_atspi_rich_projection_is_portable() {
    let node = parse(json!({
        "ref":"ui:linux-slider","role":"slider","name":"","description":"Layer opacity",
        "help":"Adjust opacity","accessibility_id":"opacity-control","framework":"gtk4",
        "attributes":{"semantic-name":"Opacity","orientation":"horizontal"},
        "relations":[{"kind":"labelled_by","targets":["ui:linux-label"]}],
        "facets":{"value":{"current":75.0,"minimum":0.0,"maximum":100.0,"increment":1.0}},
        "states":["enabled","focusable"],"actions":["increment","decrement"],
        "app":"org.gimp.GIMP","parent_ref":"ui:linux-dialog",
        "bounds":{"x":80,"y":120,"width":240,"height":28,"coordinate_space":"screen"},
        "children_count":0
    }));
    assert_eq!(node.framework, "gtk4");
    assert!(node.facets.has("value"));
    assert_eq!(
        node.facets.value.as_ref().and_then(|v| v.current),
        Some(75.0)
    );
    assert_eq!(node.relations[0].kind, "labelled_by");
}

#[test]
fn ax_and_uia_fit_the_same_node_contract() {
    let mac = parse(json!({
        "ref":"ui:mac-window","role":"window","name":"Export","description":"",
        "help":"","accessibility_id":"export-window","framework":"appkit",
        "attributes":{"subrole":"AXStandardWindow"},"relations":[],
        "facets":{"window":{"modal":false,"minimized":false,"maximized":null,
          "can_minimize":true,"can_maximize":null},
          "transform":{"can_move":true,"can_resize":true,"can_rotate":false}},
        "states":["enabled","focused"],"actions":[],"app":"com.example.editor",
        "parent_ref":null,"bounds":null,"children_count":12
    }));
    let win = parse(json!({
        "ref":"ui:win-grid","role":"table","name":"Assets","description":"",
        "help":"Asset browser","accessibility_id":"AssetsGrid","framework":"WinUI",
        "attributes":{"class":"ItemsRepeater","item_status":"Ready"},
        "relations":[{"kind":"labelled_by","targets":["ui:win-label"]}],
        "facets":{"table":{"rows":40,"columns":4,"row":null,"column":null,
          "row_span":null,"column_span":null,"selected_rows":2,"selected_columns":0,
          "row_headers":[],"column_headers":["Name","Type","Size","Modified"]},
          "selection":{"selected":null,"selected_count":2,"child_count":160,"multi_select":true},
          "scroll":{"horizontal_percent":0.0,"vertical_percent":25.0,
          "horizontal_view_size":100.0,"vertical_view_size":30.0}},
        "states":["enabled"],"actions":[],"app":"pid:4242",
        "parent_ref":null,"bounds":null,"children_count":160
    }));
    assert!(mac.facets.has("window"));
    assert!(mac.facets.has("transform"));
    assert!(win.facets.has("table"));
    assert!(win.facets.has("selection"));
    assert!(win.facets.has("scroll"));
    assert_eq!(win.facets.table.as_ref().and_then(|v| v.columns), Some(4));
}

#[test]
fn one_selector_matches_equivalent_cross_platform_semantics() {
    let linux = parse(json!({
        "ref":"ui:linux","role":"slider","name":"","help":"Adjust opacity","framework":"gtk4",
        "attributes":{"semantic-name":"Opacity"},"relations":[{"kind":"labelled_by","targets":["ui:label"]}],
        "facets":{"value":{"current":50.0,"minimum":0.0,"maximum":100.0,"increment":1.0}},
        "states":["enabled"],"actions":["increment"],"app":"editor","parent_ref":null,
        "bounds":null,"children_count":0
    }));
    let mut windows = linux.clone();
    windows.reference = "ui:windows".into();
    windows.name = "Opacity".into();
    windows.framework = "WinUI".into();

    let selector = Selector {
        app: Some("editor".into()),
        role: Some("slider".into()),
        name: None,
        help: Some(NameMatch {
            op: NameOp::Regex,
            value: "(?i)opacity".into(),
        }),
        framework: None,
        states: vec!["enabled".into()],
        action: None,
        attributes: [("semantic-name".into(), "Opacity".into())].into(),
        relation: Some(RelationMatch {
            kind: "labelled_by".into(),
            target: Some("ui:label".into()),
        }),
        facet: Some("value".into()),
        ancestor: None,
        nth: None,
        query: None,
    };
    assert_eq!(selector.select(&[linux, windows]).unwrap().len(), 2);
}

#[test]
fn v1_compatibility_and_strict_facets_are_preserved() {
    let legacy = parse(json!({
        "ref":"ui:legacy","role":"button","name":"Save","states":["enabled"],
        "actions":["click"],"app":"legacy","parent_ref":null,"bounds":null,"children_count":0
    }));
    assert!(legacy.help.is_empty());
    assert!(legacy.attributes.is_empty());
    assert!(legacy.relations.is_empty());
    assert!(legacy.facets.is_empty());

    let err = serde_json::from_value::<UiNode>(json!({
        "ref":"ui:bad","role":"slider","name":"Opacity",
        "facets":{"value":{
          "current":1.0,"minimum":0.0,"maximum":1.0,"increment":0.1,
          "invented_native_field":"must-not-leak"
        }},
        "states":[],"actions":[],"app":"fixture",
        "parent_ref":null,"bounds":null,"children_count":0
    }))
    .expect_err("native extension fields must not leak into portable facets");
    assert!(err.to_string().contains("invented_native_field"));
}
