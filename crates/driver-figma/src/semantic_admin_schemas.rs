use serde_json::{Map, Value, json};

pub const OPERATIONS: &[&str] = &[
    "style.order.after",
    "style.folder.order.after",
    "slides.grid.inspect",
    "slides.grid.set",
    "annotation.category.inspect",
    "font.load",
    "dev.focused_node",
    "codegen.status",
    "codegen.refresh",
    "user.current",
    "figjam.active_users",
    "node.top_level_frame",
    "node.plugin_data.keys",
    "node.shared_plugin_data.get",
    "node.shared_plugin_data.keys",
    "node.shared_plugin_data.set",
    "library.publish_status.inspect",
    "figjam.stamp.author.inspect",
    "node.resize_unconstrained",
    "node.aspect_ratio.lock",
    "node.aspect_ratio.unlock",
    "instance.overrides.remove_all",
    "layout.grid.rows.reorder",
    "layout.grid.columns.reorder",
    "layout.grid.child.position",
    "slides.transition.inspect",
    "slides.transition.set",
    "textreview.status",
    "textreview.enable",
    "textreview.disable",
    "widget.find_by_widget_id",
    "dev.measurement.list",
    "dev.measurement.for_node",
    "dev.measurement.add",
    "dev.measurement.edit",
    "dev.measurement.remove",
    "variable.collection.extend",
    "variable.values_for_collection",
    "variable.override.remove_mode",
    "variable.collection.overrides.remove_variable",
    "variable.resolve_for_consumer",
    "style.consumers.list",
    "object.plugin_data.get",
    "object.plugin_data.set",
    "object.plugin_data.keys",
    "object.shared_plugin_data.get",
    "object.shared_plugin_data.set",
    "object.shared_plugin_data.keys",
    "style.variable.bind",
];

fn s(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn u(max: u64) -> Value {
    json!({"type":"integer","minimum":0,"maximum":max})
}
fn n(min: f64, max: f64) -> Value {
    json!({"type":"number","minimum":min,"maximum":max})
}
fn nullable_string(max: usize) -> Value {
    json!({"type":["string","null"],"maxLength":max})
}
fn arr(max: usize, items: Value) -> Value {
    json!({"type":"array","maxItems":max,"items":items})
}
fn en(values: &[&str]) -> Value {
    json!({"type":"string","enum":values})
}
fn input(fields: Vec<(&str, Value)>, required: &[&str]) -> Value {
    let mut properties = Map::new();
    properties.insert("session_id".into(), s(128));
    properties.insert("expected_revision".into(), u(u64::MAX));
    for (name, schema) in fields {
        properties.insert(name.into(), schema);
    }
    json!({
        "type":"object",
        "properties":properties,
        "required":required,
        "additionalProperties":false,
        "maxProperties":properties.len()
    })
}
fn no_args() -> Value {
    input(vec![], &[])
}
fn node_summary() -> Value {
    json!({
        "type":"object",
        "properties":{
            "id":s(256),"type":s(64),"name":{"type":"string","maxLength":512},
            "visible":{"type":"boolean"},"locked":{"type":"boolean"},
            "x":{"type":"number"},"y":{"type":"number"},
            "width":{"type":"number","minimum":0},"height":{"type":"number","minimum":0},
            "rotation":{"type":"number"},"opacity":{"type":"number","minimum":0,"maximum":1},
            "layoutMode":{"type":"string","maxLength":64}
        },
        "required":["id","type","name"],
        "additionalProperties":false
    })
}
fn category_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "id":s(256),
            "label":{"type":"string","maxLength":256},
            "color":en(&["yellow","orange","red","pink","violet","blue","teal","green"]),
            "isPreset":{"type":"boolean"}
        },
        "required":["id","label","color","isPreset"],
        "additionalProperties":false
    })
}
fn user_schema(active: bool) -> Value {
    let mut properties = Map::new();
    properties.insert("id".into(), nullable_string(256));
    properties.insert("name".into(), json!({"type":"string","maxLength":512}));
    properties.insert("photoUrl".into(), nullable_string(4096));
    properties.insert("color".into(), s(128));
    properties.insert("sessionId".into(), u(u64::MAX));
    let mut required = vec!["id", "name", "photoUrl", "color", "sessionId"];
    if active {
        properties.insert(
            "position".into(),
            json!({
                "oneOf":[
                    {"type":"null"},
                    {"type":"object","properties":{"x":n(-1e9,1e9),"y":n(-1e9,1e9)},
                     "required":["x","y"],"additionalProperties":false}
                ]
            }),
        );
        properties.insert(
            "viewport".into(),
            json!({
                "type":"object",
                "properties":{"x":n(-1e9,1e9),"y":n(-1e9,1e9),
                    "width":n(0.0,1e9),"height":n(0.0,1e9)},
                "required":["x","y","width","height"],"additionalProperties":false
            }),
        );
        properties.insert("selection".into(), arr(512, s(256)));
        required.extend(["position", "viewport", "selection"]);
    }
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn slide_transition_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "style":{"type":"string","enum":["NONE","DISSOLVE","SLIDE_FROM_LEFT","SLIDE_FROM_RIGHT","SLIDE_FROM_BOTTOM","SLIDE_FROM_TOP","PUSH_FROM_LEFT","PUSH_FROM_RIGHT","PUSH_FROM_BOTTOM","PUSH_FROM_TOP","MOVE_FROM_LEFT","MOVE_FROM_RIGHT","MOVE_FROM_TOP","MOVE_FROM_BOTTOM","SLIDE_OUT_TO_LEFT","SLIDE_OUT_TO_RIGHT","SLIDE_OUT_TO_TOP","SLIDE_OUT_TO_BOTTOM","MOVE_OUT_TO_LEFT","MOVE_OUT_TO_RIGHT","MOVE_OUT_TO_TOP","MOVE_OUT_TO_BOTTOM","SMART_ANIMATE"]},
            "duration":n(0.0,3600.0),
            "curve":{"type":"string","enum":["EASE_IN","EASE_OUT","EASE_IN_AND_OUT","LINEAR","GENTLE","QUICK","BOUNCY","SLOW"]},
            "timing":{"type":"object","properties":{"type":{"type":"string","enum":["ON_CLICK","AFTER_DELAY"]},"delay":n(0.0,3600.0)},"required":["type"],"additionalProperties":false}
        },
        "required":["style","duration","curve","timing"],
        "additionalProperties":false
    })
}
fn nullable_node_summary() -> Value {
    json!({"oneOf":[{"type":"null"},node_summary()]})
}
fn aspect_ratio_schema() -> Value {
    json!({"oneOf":[{"type":"null"},{"type":"object","properties":{"x":n(-1e9,1e9),"y":n(-1e9,1e9)},"required":["x","y"],"additionalProperties":false}]})
}
fn measurement_offset_schema() -> Value {
    json!({"oneOf":[
        {"type":"object","properties":{"type":{"const":"INNER"},"relative":n(-1000.0,1000.0)},"required":["type","relative"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"OUTER"},"fixed":n(-1e9,1e9)},"required":["type","fixed"],"additionalProperties":false}
    ]})
}
fn measurement_schema() -> Value {
    let endpoint = json!({"type":"object","properties":{"node":node_summary(),"side":en(&["TOP","RIGHT","BOTTOM","LEFT"])},"required":["node","side"],"additionalProperties":false});
    json!({"type":"object","properties":{"id":s(256),"start":endpoint.clone(),"end":endpoint,"offset":measurement_offset_schema(),"freeText":{"type":"string","maxLength":4096}},"required":["id","start","end","offset","freeText"],"additionalProperties":false})
}
fn variable_value_schema() -> Value {
    json!({"oneOf":[
        {"type":"boolean"},{"type":"number"},{"type":"string","maxLength":65536},
        {"type":"object","properties":{"r":n(0.0,1.0),"g":n(0.0,1.0),"b":n(0.0,1.0),"a":n(0.0,1.0)},"required":["r","g","b"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"VARIABLE_ALIAS"},"id":s(256)},"required":["type","id"],"additionalProperties":false}
    ]})
}
pub fn input_schema(name: &str) -> Option<Value> {
    if !OPERATIONS.contains(&name) {
        return None;
    }
    let schema = match name {
        "style.order.after" => input(
            vec![
                ("styleId", s(256)),
                ("referenceStyleId", nullable_string(256)),
            ],
            &["styleId"],
        ),
        "style.folder.order.after" => input(
            vec![
                ("styleType", en(&["PAINT", "TEXT", "EFFECT", "GRID"])),
                ("targetFolder", s(512)),
                ("referenceFolder", nullable_string(512)),
            ],
            &["styleType", "targetFolder"],
        ),
        "slides.grid.inspect"
        | "dev.focused_node"
        | "codegen.status"
        | "codegen.refresh"
        | "user.current"
        | "figjam.active_users"
        | "textreview.status"
        | "textreview.enable"
        | "textreview.disable" => no_args(),
        "slides.grid.set" => input(vec![("rows", arr(100, arr(100, s(256))))], &["rows"]),
        "annotation.category.inspect" => input(vec![("id", s(256))], &["id"]),
        "font.load" => input(
            vec![
                ("family", s(256)),
                ("style", s(256)),
                (
                    "variationSettings",
                    json!({
                        "type":"object","maxProperties":32,
                        "additionalProperties":{"type":"number","minimum":-100000,"maximum":100000}
                    }),
                ),
            ],
            &["family"],
        ),
        "node.top_level_frame"
        | "node.plugin_data.keys"
        | "figjam.stamp.author.inspect"
        | "node.aspect_ratio.lock"
        | "node.aspect_ratio.unlock"
        | "instance.overrides.remove_all"
        | "slides.transition.inspect" => input(vec![("nodeId", s(256))], &["nodeId"]),
        "node.shared_plugin_data.get" => input(
            vec![
                ("nodeId", s(256)),
                (
                    "namespace",
                    json!({"type":"string","pattern":"^[A-Za-z0-9]{3,128}$"}),
                ),
                ("key", s(256)),
            ],
            &["nodeId", "namespace", "key"],
        ),
        "node.shared_plugin_data.keys" => input(
            vec![
                ("nodeId", s(256)),
                (
                    "namespace",
                    json!({"type":"string","pattern":"^[A-Za-z0-9]{3,128}$"}),
                ),
            ],
            &["nodeId", "namespace"],
        ),
        "node.shared_plugin_data.set" => input(
            vec![
                ("nodeId", s(256)),
                (
                    "namespace",
                    json!({"type":"string","pattern":"^[A-Za-z0-9]{3,128}$"}),
                ),
                ("key", s(256)),
                ("value", json!({"type":"string","maxLength":98304})),
            ],
            &["nodeId", "namespace", "key", "value"],
        ),
        "library.publish_status.inspect" => input(
            vec![
                ("targetId", s(256)),
                (
                    "targetKind",
                    en(&["AUTO", "NODE", "STYLE", "VARIABLE", "COLLECTION"]),
                ),
            ],
            &["targetId"],
        ),
        "node.resize_unconstrained" => input(
            vec![
                ("nodeId", s(256)),
                ("width", n(0.01, 100000.0)),
                ("height", n(0.01, 100000.0)),
            ],
            &["nodeId", "width", "height"],
        ),
        "layout.grid.rows.reorder" | "layout.grid.columns.reorder" => input(
            vec![
                ("nodeId", s(256)),
                ("fromIndices", arr(1000, u(100000))),
                ("insertionIndex", u(100000)),
            ],
            &["nodeId", "fromIndices", "insertionIndex"],
        ),
        "layout.grid.child.position" => input(
            vec![
                ("nodeId", s(256)),
                ("rowIndex", u(100000)),
                ("columnIndex", u(100000)),
            ],
            &["nodeId", "rowIndex", "columnIndex"],
        ),
        "slides.transition.set" => input(
            vec![
                ("nodeId", s(256)),
                ("transition", slide_transition_schema()),
            ],
            &["nodeId", "transition"],
        ),
        "widget.find_by_widget_id" => input(
            vec![
                ("widgetId", s(256)),
                ("rootNodeId", s(256)),
                ("limit", u(500)),
            ],
            &["widgetId"],
        ),
        "dev.measurement.list" => no_args(),
        "dev.measurement.for_node" => input(vec![("nodeId", s(256))], &["nodeId"]),
        "dev.measurement.add" => input(
            vec![
                ("startNodeId", s(256)),
                ("startSide", en(&["TOP", "RIGHT", "BOTTOM", "LEFT"])),
                ("endNodeId", s(256)),
                ("endSide", en(&["TOP", "RIGHT", "BOTTOM", "LEFT"])),
                ("offset", measurement_offset_schema()),
                ("freeText", json!({"type":"string","maxLength":4096})),
            ],
            &["startNodeId", "startSide", "endNodeId", "endSide"],
        ),
        "dev.measurement.edit" => input(
            vec![
                ("measurementId", s(256)),
                ("offset", measurement_offset_schema()),
                ("freeText", json!({"type":"string","maxLength":4096})),
            ],
            &["measurementId"],
        ),
        "dev.measurement.remove" => input(vec![("measurementId", s(256))], &["measurementId"]),
        "variable.collection.extend" => input(
            vec![("collectionId", s(256)), ("name", s(256))],
            &["collectionId", "name"],
        ),
        "variable.values_for_collection" => input(
            vec![("variableId", s(256)), ("collectionId", s(256))],
            &["variableId", "collectionId"],
        ),
        "variable.override.remove_mode" => input(
            vec![("variableId", s(256)), ("modeId", s(256))],
            &["variableId", "modeId"],
        ),
        "variable.collection.overrides.remove_variable" => input(
            vec![("collectionId", s(256)), ("variableId", s(256))],
            &["collectionId", "variableId"],
        ),
        "variable.resolve_for_consumer" => input(
            vec![("variableId", s(256)), ("nodeId", s(256))],
            &["variableId", "nodeId"],
        ),
        "style.consumers.list" => {
            input(vec![("styleId", s(256)), ("limit", u(1000))], &["styleId"])
        }
        "object.plugin_data.get" => input(
            vec![
                (
                    "targetKind",
                    en(&["NODE", "STYLE", "VARIABLE", "COLLECTION"]),
                ),
                ("targetId", s(256)),
                ("key", s(256)),
            ],
            &["targetKind", "targetId", "key"],
        ),
        "object.plugin_data.keys" => input(
            vec![
                (
                    "targetKind",
                    en(&["NODE", "STYLE", "VARIABLE", "COLLECTION"]),
                ),
                ("targetId", s(256)),
            ],
            &["targetKind", "targetId"],
        ),
        "object.plugin_data.set" => input(
            vec![
                (
                    "targetKind",
                    en(&["NODE", "STYLE", "VARIABLE", "COLLECTION"]),
                ),
                ("targetId", s(256)),
                ("key", s(256)),
                ("value", json!({"type":"string","maxLength":98304})),
            ],
            &["targetKind", "targetId", "key", "value"],
        ),
        "object.shared_plugin_data.get" => input(
            vec![
                (
                    "targetKind",
                    en(&["NODE", "STYLE", "VARIABLE", "COLLECTION"]),
                ),
                ("targetId", s(256)),
                (
                    "namespace",
                    json!({"type":"string","pattern":"^[A-Za-z0-9]{3,128}$"}),
                ),
                ("key", s(256)),
            ],
            &["targetKind", "targetId", "namespace", "key"],
        ),
        "object.shared_plugin_data.keys" => input(
            vec![
                (
                    "targetKind",
                    en(&["NODE", "STYLE", "VARIABLE", "COLLECTION"]),
                ),
                ("targetId", s(256)),
                (
                    "namespace",
                    json!({"type":"string","pattern":"^[A-Za-z0-9]{3,128}$"}),
                ),
            ],
            &["targetKind", "targetId", "namespace"],
        ),
        "object.shared_plugin_data.set" => input(
            vec![
                (
                    "targetKind",
                    en(&["NODE", "STYLE", "VARIABLE", "COLLECTION"]),
                ),
                ("targetId", s(256)),
                (
                    "namespace",
                    json!({"type":"string","pattern":"^[A-Za-z0-9]{3,128}$"}),
                ),
                ("key", s(256)),
                ("value", json!({"type":"string","maxLength":98304})),
            ],
            &["targetKind", "targetId", "namespace", "key", "value"],
        ),
        "style.variable.bind" => input(
            vec![
                ("styleId", s(256)),
                (
                    "field",
                    en(&[
                        "fontFamily",
                        "fontSize",
                        "fontStyle",
                        "fontWeight",
                        "letterSpacing",
                        "lineHeight",
                        "paragraphSpacing",
                        "paragraphIndent",
                    ]),
                ),
                ("variableId", nullable_string(256)),
            ],
            &["styleId", "field"],
        ),
        other => unreachable!("semantic-admin operation {other} lacks input schema"),
    };
    Some(schema)
}
pub fn output_schema(name: &str) -> Option<Value> {
    if !OPERATIONS.contains(&name) {
        return None;
    }
    let schema = match name {
        "style.order.after" => json!({
            "type":"object",
            "properties":{
                "moved":{"type":"boolean"},
                "targetId":s(256),
                "referenceId":nullable_string(256)
            },
            "required":["moved","targetId","referenceId"],
            "additionalProperties":false
        }),
        "style.folder.order.after" => json!({
            "type":"object",
            "properties":{
                "moved":{"type":"boolean"},
                "styleType":en(&["PAINT","TEXT","EFFECT","GRID"]),
                "targetFolder":s(512),
                "referenceFolder":nullable_string(512)
            },
            "required":["moved","styleType","targetFolder","referenceFolder"],
            "additionalProperties":false
        }),
        "slides.grid.inspect" => arr(100, arr(100, node_summary())),
        "slides.grid.set" => json!({
            "type":"object",
            "properties":{"rows":u(100),"slides":u(10000)},
            "required":["rows","slides"],
            "additionalProperties":false
        }),
        "annotation.category.inspect" => json!({
            "oneOf":[{"type":"null"},category_schema()]
        }),
        "font.load" => json!({
            "type":"object",
            "properties":{
                "loaded":{"type":"boolean"},
                "family":s(256),
                "style":nullable_string(256)
            },
            "required":["loaded","family","style"],
            "additionalProperties":false
        }),
        "dev.focused_node" => json!({"oneOf":[{"type":"null"},node_summary()]}),
        "codegen.status" => json!({
            "type":"object",
            "properties":{
                "editorType":{"type":"string","const":"dev"},
                "mode":en(&["default","textreview","inspect","codegen","linkpreview","auth"]),
                "preferences":{
                    "type":"object",
                    "properties":{
                        "unit":en(&["PIXEL","SCALED"]),
                        "scaleFactor":{"type":["number","null"]},
                        "customSettings":{"type":"object","maxProperties":64,"additionalProperties":{"type":"string","maxLength":1024}}
                    },
                    "required":["unit","scaleFactor","customSettings"],
                    "additionalProperties":false
                }
            },
            "required":["editorType","mode","preferences"],
            "additionalProperties":false
        }),
        "codegen.refresh" => json!({
            "type":"object","properties":{"refreshed":{"type":"boolean"}},
            "required":["refreshed"],"additionalProperties":false
        }),
        "user.current" => json!({"oneOf":[{"type":"null"},user_schema(false)]}),
        "figjam.active_users" => arr(128, user_schema(true)),
        "textreview.status" | "textreview.enable" | "textreview.disable" => json!({
            "type":"object",
            "properties":{"enabled":{"type":"boolean"}},
            "required":["enabled"],
            "additionalProperties":false
        }),
        "node.top_level_frame" => nullable_node_summary(),
        "node.plugin_data.keys" | "node.shared_plugin_data.keys" => arr(1024, s(1024)),
        "node.shared_plugin_data.get" => json!({
            "type":"object","properties":{"namespace":s(128),"key":s(256),"value":{"type":"string","maxLength":98304}},
            "required":["namespace","key","value"],"additionalProperties":false
        }),
        "node.shared_plugin_data.set" => json!({
            "type":"object","properties":{"stored":{"type":"boolean"},"removed":{"type":"boolean"},"bytes":u(98304)},
            "required":["stored","removed","bytes"],"additionalProperties":false
        }),
        "library.publish_status.inspect" => json!({
            "type":"object","properties":{"targetKind":en(&["NODE","STYLE","VARIABLE","COLLECTION"]),"targetId":s(256),"status":en(&["UNPUBLISHED","CURRENT","CHANGED"])},
            "required":["targetKind","targetId","status"],"additionalProperties":false
        }),
        "figjam.stamp.author.inspect" => {
            json!({"oneOf":[{"type":"null"},{"type":"object","properties":{
            "id":nullable_string(256),"name":{"type":"string","maxLength":512},"photoUrl":nullable_string(4096)
        },"required":["id","name","photoUrl"],"additionalProperties":false}]})
        }
        "node.resize_unconstrained" => node_summary(),
        "node.aspect_ratio.lock" | "node.aspect_ratio.unlock" => json!({
            "type":"object","properties":{"nodeId":s(256),"targetAspectRatio":aspect_ratio_schema()},
            "required":["nodeId","targetAspectRatio"],"additionalProperties":false
        }),
        "instance.overrides.remove_all" => json!({
            "type":"object","properties":{"nodeId":s(256),"removed":u(100000)},
            "required":["nodeId","removed"],"additionalProperties":false
        }),
        "layout.grid.rows.reorder" | "layout.grid.columns.reorder" => arr(
            10000,
            json!({
                "type":"object","properties":{"from":u(100000),"to":u(100000)},
                "required":["from","to"],"additionalProperties":false
            }),
        ),
        "layout.grid.child.position" => json!({
            "type":"object","properties":{"nodeId":s(256),"rowIndex":u(100000),"columnIndex":u(100000)},
            "required":["nodeId","rowIndex","columnIndex"],"additionalProperties":false
        }),
        "slides.transition.inspect" => slide_transition_schema(),
        "slides.transition.set" => json!({
            "type":"object","properties":{"nodeId":s(256),"transition":slide_transition_schema()},
            "required":["nodeId","transition"],"additionalProperties":false
        }),
        "widget.find_by_widget_id" => arr(500, node_summary()),
        "dev.measurement.list" | "dev.measurement.for_node" => arr(1000, measurement_schema()),
        "dev.measurement.add" | "dev.measurement.edit" => measurement_schema(),
        "dev.measurement.remove" => {
            json!({"type":"object","properties":{"removed":{"type":"boolean"},"measurementId":s(256)},"required":["removed","measurementId"],"additionalProperties":false})
        }
        "variable.collection.extend" => json!({"type":"object","properties":{
            "id":s(256),"key":s(256),"name":{"type":"string","maxLength":256},"isExtension":{"type":"boolean"},
            "modeCount":u(128),"variableCount":u(100000)
        },"required":["id","key","name","isExtension","modeCount","variableCount"],"additionalProperties":false}),
        "variable.values_for_collection" => json!({"type":"object","properties":{
            "variableId":s(256),"collectionId":s(256),
            "values":{"type":"object","maxProperties":128,"additionalProperties":variable_value_schema()}
        },"required":["variableId","collectionId","values"],"additionalProperties":false}),
        "variable.override.remove_mode" | "variable.collection.overrides.remove_variable" => {
            json!({
                "type":"object","properties":{"removed":{"type":"boolean"}},"required":["removed"],"additionalProperties":false
            })
        }
        "variable.resolve_for_consumer" => json!({"type":"object","properties":{
            "value":variable_value_schema(),"resolvedType":en(&["BOOLEAN","FLOAT","STRING","COLOR"])
        },"required":["value","resolvedType"],"additionalProperties":false}),
        "style.consumers.list" => arr(
            1000,
            json!({"type":"object","properties":{
            "node":node_summary(),"fields":arr(16,s(128))
        },"required":["node","fields"],"additionalProperties":false}),
        ),
        "object.plugin_data.get" => json!({"type":"object","properties":{
            "targetKind":en(&["NODE","STYLE","VARIABLE","COLLECTION"]),"targetId":s(256),"key":s(256),"value":{"type":"string","maxLength":98304}
        },"required":["targetKind","targetId","key","value"],"additionalProperties":false}),
        "object.plugin_data.keys" | "object.shared_plugin_data.keys" => arr(1024, s(1024)),
        "object.plugin_data.set" | "object.shared_plugin_data.set" => {
            json!({"type":"object","properties":{
            "stored":{"type":"boolean"},"removed":{"type":"boolean"},"bytes":u(100000)
        },"required":["stored","removed","bytes"],"additionalProperties":false})
        }
        "object.shared_plugin_data.get" => json!({"type":"object","properties":{
            "targetKind":en(&["NODE","STYLE","VARIABLE","COLLECTION"]),"targetId":s(256),"namespace":s(128),"key":s(256),"value":{"type":"string","maxLength":98304}
        },"required":["targetKind","targetId","namespace","key","value"],"additionalProperties":false}),
        "style.variable.bind" => json!({"type":"object","properties":{
            "styleId":s(256),"field":en(&["fontFamily","fontSize","fontStyle","fontWeight","letterSpacing","lineHeight","paragraphSpacing","paragraphIndent"]),
            "bound":{"type":"boolean"},"variableId":nullable_string(256)
        },"required":["styleId","field","bound","variableId"],"additionalProperties":false}),
        other => unreachable!("semantic-admin operation {other} lacks output schema"),
    };
    Some(schema)
}
