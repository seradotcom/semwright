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
        text: None,
        value: None,
        selection: None,
        table: None,
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

#[test]
fn rich_text_and_table_cell_facets_are_bounded_portable_data() {
    let text = parse(json!({
        "ref":"ui:text","role":"text","name":"Editor","description":"",
        "facets":{"text":{"character_count":120,"caret_offset":42,"selection_count":1,
          "selections":[{"start":10,"end":18}],
          "caret_attributes":{"font-family":"Inter","weight":"700"},
          "caret_attribute_range":{"start":40,"end":48},
          "editable":true,"password":false}},
        "states":["enabled","editable"],"actions":[],"app":"editor",
        "parent_ref":null,"bounds":null,"children_count":0
    }));
    let facet = text.facets.text.as_ref().unwrap();
    assert_eq!(facet.selections[0].start, 10);
    assert_eq!(facet.caret_attributes["weight"], "700");

    let cell = parse(json!({
        "ref":"ui:cell","role":"table_cell","name":"Revenue","description":"",
        "facets":{"table":{"rows":null,"columns":null,"row":5,"column":2,
          "row_span":1,"column_span":2,"selected_rows":null,"selected_columns":null,
          "row_headers":["August"],"column_headers":["Revenue"]}},
        "states":["enabled"],"actions":[],"app":"sheet",
        "parent_ref":"ui:table","bounds":null,"children_count":0
    }));
    let table = cell.facets.table.as_ref().unwrap();
    assert_eq!((table.row, table.column), (Some(5), Some(2)));
    assert_eq!(table.column_span, Some(2));
    assert_eq!(table.row_headers, vec!["August"]);
}

#[test]
fn facet_specific_selectors_match_semantics_not_platform_details() {
    let mut slider = parse(json!({
        "ref":"ui:slider","role":"slider","name":"Opacity","description":"",
        "facets":{"value":{"current":52.0,"minimum":0.0,"maximum":100.0,"increment":1.0}},
        "states":["enabled"],"actions":["set_value"],"app":"editor",
        "parent_ref":null,"bounds":null,"children_count":0
    }));
    slider.facets.text = Some(UiTextFacet {
        editable: false,
        password: false,
        ..UiTextFacet::default()
    });
    let selector = Selector {
        value: Some(ValueFacetMatch {
            minimum: Some(50.0),
            maximum: Some(60.0),
        }),
        text: Some(TextFacetMatch {
            editable: Some(false),
            password: Some(false),
            has_selection: Some(false),
        }),
        ..Default::default()
    };
    assert_eq!(selector.unique(&[slider]).unwrap().reference, "ui:slider");
}

#[test]
fn table_and_selection_selectors_match_rich_facets() {
    let mut cell = parse(json!({
        "ref":"ui:cell","role":"table_cell","name":"Revenue","description":"",
        "facets":{"table":{"row":5,"column":2,"row_span":1,"column_span":1}},
        "states":["enabled"],"actions":[],"app":"sheet",
        "parent_ref":"ui:table","bounds":null,"children_count":0
    }));
    cell.facets.selection = Some(UiSelectionFacet {
        selected: Some(true),
        multi_select: Some(false),
        ..UiSelectionFacet::default()
    });
    let selector = Selector {
        table: Some(TableFacetMatch {
            row: Some(5),
            column: Some(2),
            min_rows: None,
            min_columns: None,
        }),
        selection: Some(SelectionFacetMatch {
            selected: Some(true),
            multi_select: Some(false),
        }),
        ..Default::default()
    };
    assert_eq!(selector.unique(&[cell]).unwrap().reference, "ui:cell");
}

#[test]
fn inverted_or_nonfinite_value_ranges_are_rejected() {
    let node = parse(json!({
        "ref":"ui:value","role":"slider","name":"Value","description":"",
        "facets":{"value":{"current":50.0}},
        "states":[],"actions":[],"app":"fixture","parent_ref":null,"bounds":null,"children_count":0
    }));
    let inverted = Selector {
        value: Some(ValueFacetMatch {
            minimum: Some(80.0),
            maximum: Some(20.0),
        }),
        ..Default::default()
    };
    assert_eq!(
        inverted.select(&[node.clone()]).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    let nonfinite = Selector {
        value: Some(ValueFacetMatch {
            minimum: Some(f64::NAN),
            maximum: None,
        }),
        ..Default::default()
    };
    assert_eq!(
        nonfinite.select(&[node]).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
}
