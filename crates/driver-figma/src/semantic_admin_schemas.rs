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
    let mut required = vec!["id","name","photoUrl","color","sessionId"];
    if active {
        properties.insert("position".into(), json!({
            "oneOf":[
                {"type":"null"},
                {"type":"object","properties":{"x":n(-1e9,1e9),"y":n(-1e9,1e9)},
                 "required":["x","y"],"additionalProperties":false}
            ]
        }));
        properties.insert("viewport".into(), json!({
            "type":"object",
            "properties":{"x":n(-1e9,1e9),"y":n(-1e9,1e9),
                "width":n(0.0,1e9),"height":n(0.0,1e9)},
            "required":["x","y","width","height"],"additionalProperties":false
        }));
        properties.insert("selection".into(), arr(512, s(256)));
        required.extend(["position","viewport","selection"]);
    }
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
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
                ("styleType", en(&["PAINT","TEXT","EFFECT","GRID"])),
                ("targetFolder", s(512)),
                ("referenceFolder", nullable_string(512)),
            ],
            &["styleType","targetFolder"],
        ),
        "slides.grid.inspect"
        | "dev.focused_node"
        | "codegen.status"
        | "codegen.refresh"
        | "user.current"
        | "figjam.active_users" => no_args(),
        "slides.grid.set" => input(
            vec![("rows", arr(100, arr(100, s(256))))],
            &["rows"],
        ),
        "annotation.category.inspect" => input(vec![("id", s(256))], &["id"]),
        "font.load" => input(
            vec![
                ("family", s(256)),
                ("style", s(256)),
                ("variationSettings", json!({
                    "type":"object","maxProperties":32,
                    "additionalProperties":{"type":"number","minimum":-100000,"maximum":100000}
                })),
            ],
            &["family"],
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
        other => unreachable!("semantic-admin operation {other} lacks output schema"),
    };
    Some(schema)
}
