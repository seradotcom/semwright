use serde_json::{Map, Value, json};

pub const OPERATIONS: &[&str] = &[
    "artifact.status",
    "artifact.upload.append",
    "artifact.upload.begin",
    "brush.load",
    "buzz.media_content.set",
    "component.size_variants.create",
    "component.variant_matrix.create",
    "file.thumbnail.get",
    "file.thumbnail.set",
    "group.create",
    "group.ungroup",
    "history.commit",
    "history.undo",
    "image.create",
    "image.export",
    "image.inspect",
    "media.fill.apply",
    "motion.playhead.get",
    "node.plugin_data.get",
    "node.plugin_data.set",
    "node.relaunch_data.get",
    "node.relaunch_data.set",
    "page.divider.create",
    "slice.create",
    "video.create",
    "figjam.table.inspect",
    "figjam.table.cell.inspect",
    "figjam.table.cell.text.set",
    "figjam.table.row.insert",
    "figjam.table.row.remove",
    "figjam.table.row.move",
    "figjam.table.row.resize",
    "figjam.table.column.insert",
    "figjam.table.column.remove",
    "figjam.table.column.move",
    "figjam.table.column.resize",
    "node.bindings.inspect",
    "node.properties.patch",
    "canvas.info",
    "canvas.next_position",
    "canvas.arrange",
    "slot.list",
    "slot.preferred.set",
    "slot.content.add",
    "analysis.colors",
    "analysis.typography",
    "analysis.spacing",
    "analysis.clusters",
    "validate.lint",
    "font.status",
    "viewport.canvas_view.get",
    "viewport.canvas_view.set",
    "library.collection.extend",
    "variable.bind.paint",
    "variable.bind.effect",
    "variable.bind.layout_grid",
    "design_system.export.css",
    "design_system.export.tailwind",
    "node.export.jsx",
    "node.export.storybook",
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
fn arr(max: usize, items: Value) -> Value {
    json!({"type":"array","maxItems":max,"items":items})
}
fn obj(max: usize) -> Value {
    json!({"type":"object","maxProperties":max})
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
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false,"maxProperties":properties.len()})
}
fn no_args() -> Value {
    input(vec![], &[])
}
fn node_id() -> Value {
    input(vec![("nodeId", s(256))], &["nodeId"])
}
pub fn input_schema(name: &str) -> Option<Value> {
    if !OPERATIONS.contains(&name) {
        return None;
    }
    let schema = match name {
        "file.thumbnail.get" | "motion.playhead.get" | "history.commit" | "history.undo" => {
            no_args()
        }
        "group.create" => input(
            vec![
                ("nodeIds", arr(128, s(256))),
                ("parentId", s(256)),
                ("index", u(100_000)),
            ],
            &["nodeIds"],
        ),
        "group.ungroup" | "figjam.table.inspect" => node_id(),
        "slice.create" => input(
            vec![
                ("name", s(256)),
                ("x", n(-1e6, 1e6)),
                ("y", n(-1e6, 1e6)),
                ("width", n(0.01, 1e5)),
                ("height", n(0.01, 1e5)),
            ],
            &[],
        ),
        "page.divider.create" => input(
            vec![(
                "name",
                json!({"type":"string","maxLength":64,"pattern":"^(?:\\*+|–+|—+| +)$"}),
            )],
            &[],
        ),
        "file.thumbnail.set" => input(
            vec![("nodeId", json!({"type":["string","null"],"maxLength":256}))],
            &[],
        ),
        "brush.load" => input(
            vec![("brushType", en(&["STRETCH", "SCATTER"]))],
            &["brushType"],
        ),
        "artifact.upload.begin" => input(vec![("name", s(256)), ("mediaType", s(128))], &[]),
        "artifact.upload.append" => input(
            vec![
                ("token", s(128)),
                ("offset", u(16_777_216)),
                ("base64", json!({"type":"string","maxLength":300000})),
            ],
            &["token", "offset", "base64"],
        ),
        "artifact.status" => input(vec![("token", s(128))], &["token"]),
        "image.create" => input(vec![("token", s(128))], &["token"]),
        "image.inspect" => input(vec![("hash", s(256))], &["hash"]),
        "image.export" => input(
            vec![("hash", s(256)), ("name", s(256)), ("mediaType", s(128))],
            &["hash"],
        ),
        "video.create" => input(vec![("token", s(128))], &["token"]),
        "media.fill.apply" => input(
            vec![
                ("nodeId", s(256)),
                ("mediaType", en(&["IMAGE", "VIDEO"])),
                ("hash", s(256)),
                ("scaleMode", en(&["FILL", "FIT", "CROP", "TILE"])),
            ],
            &["nodeId", "mediaType", "hash"],
        ),
        "buzz.media_content.set" => input(
            vec![
                ("nodeId", s(256)),
                ("index", u(1000)),
                ("mediaType", en(&["IMAGE", "VIDEO"])),
                ("hash", s(256)),
                ("scaleMode", en(&["FILL", "FIT", "CROP", "TILE"])),
            ],
            &["nodeId", "index", "mediaType", "hash"],
        ),
        "component.variant_matrix.create" => input(
            vec![("componentId", s(256)), ("axes", obj(8)), ("name", s(256))],
            &["componentId", "axes"],
        ),
        "component.size_variants.create" => input(
            vec![
                ("componentId", s(256)),
                ("sizes", arr(32, obj(8))),
                ("name", s(256)),
            ],
            &["componentId", "sizes"],
        ),
        "node.plugin_data.get" => input(
            vec![("nodeId", s(256)), ("key", s(128))],
            &["nodeId", "key"],
        ),
        "node.plugin_data.set" => input(
            vec![
                ("nodeId", s(256)),
                ("key", s(128)),
                ("value", json!({"type":"string","maxLength":32768})),
            ],
            &["nodeId", "key", "value"],
        ),
        "node.relaunch_data.get" => node_id(),
        "node.relaunch_data.set" => input(
            vec![("nodeId", s(256)), ("data", obj(32))],
            &["nodeId", "data"],
        ),
        "figjam.table.cell.inspect" => input(
            vec![("nodeId", s(256)), ("row", u(1000)), ("column", u(1000))],
            &["nodeId", "row", "column"],
        ),
        "figjam.table.cell.text.set" => input(
            vec![
                ("nodeId", s(256)),
                ("row", u(1000)),
                ("column", u(1000)),
                ("characters", json!({"type":"string","maxLength":65536})),
            ],
            &["nodeId", "row", "column", "characters"],
        ),
        "figjam.table.row.insert"
        | "figjam.table.row.remove"
        | "figjam.table.column.insert"
        | "figjam.table.column.remove" => input(
            vec![("nodeId", s(256)), ("index", u(1000))],
            &["nodeId", "index"],
        ),
        "figjam.table.row.move" | "figjam.table.column.move" => input(
            vec![("nodeId", s(256)), ("from", u(1000)), ("to", u(1000))],
            &["nodeId", "from", "to"],
        ),
        "figjam.table.row.resize" | "figjam.table.column.resize" => input(
            vec![
                ("nodeId", s(256)),
                ("index", u(1000)),
                ("size", n(0.01, 100_000.0)),
            ],
            &["nodeId", "index", "size"],
        ),
        "node.bindings.inspect" | "slot.list" => node_id(),
        "node.properties.patch" => input(
            vec![("nodeId", s(256)), ("properties", obj(32))],
            &["nodeId", "properties"],
        ),
        "canvas.info" => no_args(),
        "canvas.next_position" => input(vec![("gap", n(0.0, 10_000.0))], &[]),
        "canvas.arrange" => input(
            vec![
                ("nodeIds", arr(256, s(256))),
                ("columns", u(64)),
                ("gap", n(0.0, 10_000.0)),
                ("x", n(-10_000_000.0, 10_000_000.0)),
                ("y", n(-10_000_000.0, 10_000_000.0)),
            ],
            &[],
        ),
        "slot.preferred.set" => input(
            vec![("nodeId", s(256)), ("componentKeys", arr(64, s(256)))],
            &["nodeId", "componentKeys"],
        ),
        "slot.content.add" => input(
            vec![
                ("nodeId", s(256)),
                ("sourceNodeId", s(256)),
                ("componentId", s(256)),
                ("text", json!({"type":"string","maxLength":65536})),
            ],
            &["nodeId"],
        ),
        "analysis.colors"
        | "analysis.typography"
        | "analysis.spacing"
        | "analysis.clusters"
        | "validate.lint"
        | "font.status"
        | "viewport.canvas_view.get" => no_args(),
        "viewport.canvas_view.set" => input(
            vec![("canvasView", en(&["grid", "single-asset"]))],
            &["canvasView"],
        ),
        "library.collection.extend" => input(
            vec![("collectionKey", s(256)), ("name", s(256))],
            &["collectionKey", "name"],
        ),
        "variable.bind.paint" => input(
            vec![
                ("nodeId", s(256)),
                ("target", en(&["fills", "strokes"])),
                ("index", u(1024)),
                ("field", en(&["color"])),
                (
                    "variableId",
                    json!({"type":["string","null"],"maxLength":256}),
                ),
            ],
            &["nodeId", "target", "index", "field"],
        ),
        "variable.bind.effect" => input(
            vec![
                ("nodeId", s(256)),
                ("index", u(1024)),
                (
                    "field",
                    en(&["color", "radius", "spread", "offsetX", "offsetY"]),
                ),
                (
                    "variableId",
                    json!({"type":["string","null"],"maxLength":256}),
                ),
            ],
            &["nodeId", "index", "field"],
        ),
        "variable.bind.layout_grid" => input(
            vec![
                ("nodeId", s(256)),
                ("index", u(1024)),
                (
                    "field",
                    en(&["sectionSize", "count", "offset", "gutterSize"]),
                ),
                (
                    "variableId",
                    json!({"type":["string","null"],"maxLength":256}),
                ),
            ],
            &["nodeId", "index", "field"],
        ),
        "design_system.export.css" | "design_system.export.tailwind" => {
            input(vec![("modeName", s(256)), ("name", s(256))], &[])
        }
        "node.export.jsx" | "node.export.storybook" => {
            input(vec![("nodeId", s(256)), ("name", s(256))], &["nodeId"])
        }
        other => unreachable!("semantic-more operation {other} lacks input schema"),
    };
    Some(schema)
}
fn loose_output(max: usize) -> Value {
    json!({"type":"object","maxProperties":max})
}
pub fn output_schema(name: &str) -> Option<Value> {
    if !OPERATIONS.contains(&name) {
        return None;
    }
    let schema = match name {
        "group.ungroup" => arr(128, loose_output(64)),
        "motion.playhead.get" => {
            json!({"type":"object","properties":{"position":{"type":["number","null"]}},"required":["position"],"additionalProperties":false})
        }
        "artifact.upload.begin" => {
            json!({"type":"object","properties":{"token":s(128),"bytes":u(16_777_216),"mediaType":s(128),"name":s(256)},"required":["token","bytes","mediaType","name"],"additionalProperties":false})
        }
        "artifact.upload.append" => {
            json!({"type":"object","properties":{"token":s(128),"bytes":u(16_777_216),"nextOffset":u(16_777_216)},"required":["token","bytes","nextOffset"],"additionalProperties":false})
        }
        "artifact.status" => {
            json!({"type":"object","properties":{"token":s(128),"bytes":u(16_777_216)},"required":["token","bytes"],"additionalProperties":false})
        }
        "image.create" | "image.inspect" => {
            json!({"type":"object","properties":{"hash":s(256),"width":u(100_000),"height":u(100_000)},"required":["hash","width","height"],"additionalProperties":false})
        }
        "image.export"
        | "design_system.export.css"
        | "design_system.export.tailwind"
        | "node.export.jsx"
        | "node.export.storybook" => {
            json!({"type":"object","properties":{"token":s(128),"bytes":u(16_777_216),"mediaType":s(128),"name":s(256)},"required":["token","bytes","mediaType","name"],"additionalProperties":false})
        }
        "video.create" => {
            json!({"type":"object","properties":{"hash":s(256)},"required":["hash"],"additionalProperties":false})
        }
        "media.fill.apply" => {
            json!({"type":"object","properties":{"applied":{"type":"boolean"},"nodeId":s(256),"hash":s(256)},"required":["applied","nodeId","hash"],"additionalProperties":false})
        }
        "buzz.media_content.set" => {
            json!({"type":"object","properties":{"updated":{"type":"boolean"},"index":u(1000)},"required":["updated","index"],"additionalProperties":false})
        }
        "node.plugin_data.get" => {
            json!({"type":"object","properties":{"key":s(128),"value":{"type":"string","maxLength":32768}},"required":["key","value"],"additionalProperties":false})
        }
        "node.plugin_data.set" => {
            json!({"type":"object","properties":{"key":s(128),"bytes":u(32768)},"required":["key","bytes"],"additionalProperties":false})
        }
        "file.thumbnail.set" => {
            json!({"type":"object","properties":{"set":{"type":"boolean"},"nodeId":{"type":["string","null"],"maxLength":256}},"required":["set","nodeId"],"additionalProperties":false})
        }
        "history.commit" => {
            json!({"type":"object","properties":{"committed":{"type":"boolean"}},"required":["committed"],"additionalProperties":false})
        }
        "history.undo" => {
            json!({"type":"object","properties":{"triggered":{"type":"boolean"}},"required":["triggered"],"additionalProperties":false})
        }
        "brush.load" => {
            json!({"type":"object","properties":{"loaded":{"type":"boolean"},"brushType":s(16)},"required":["loaded","brushType"],"additionalProperties":false})
        }
        "node.relaunch_data.set" => {
            json!({"type":"object","properties":{"count":u(32)},"required":["count"],"additionalProperties":false})
        }
        "figjam.table.cell.inspect" | "figjam.table.cell.text.set" => loose_output(16),
        "figjam.table.inspect"
        | "figjam.table.row.insert"
        | "figjam.table.row.remove"
        | "figjam.table.column.insert"
        | "figjam.table.column.remove"
        | "figjam.table.row.move"
        | "figjam.table.column.move"
        | "figjam.table.row.resize"
        | "figjam.table.column.resize" => loose_output(64),
        "analysis.colors" => json!({
            "type":"object",
            "properties":{"colors":arr(500, loose_output(16))},
            "required":["colors"],
            "additionalProperties":false
        }),
        "analysis.typography" => json!({
            "type":"object",
            "properties":{"typography":arr(500, loose_output(16))},
            "required":["typography"],
            "additionalProperties":false
        }),
        "analysis.spacing" => json!({
            "type":"object",
            "properties":{"spacing":arr(500, loose_output(8))},
            "required":["spacing"],
            "additionalProperties":false
        }),
        "analysis.clusters" => json!({
            "type":"object",
            "properties":{"clusters":arr(500, loose_output(16))},
            "required":["clusters"],
            "additionalProperties":false
        }),
        "validate.lint" => json!({
            "type":"object",
            "properties":{"findings":arr(1000, loose_output(16))},
            "required":["findings"],
            "additionalProperties":false
        }),
        "font.status" => json!({
            "type":"object",
            "properties":{"hasMissingFont":{"type":"boolean"}},
            "required":["hasMissingFont"],
            "additionalProperties":false
        }),
        "viewport.canvas_view.get" | "viewport.canvas_view.set" => json!({
            "type":"object",
            "properties":{"canvasView":en(&["grid", "single-asset"])},
            "required":["canvasView"],
            "additionalProperties":false
        }),
        "library.collection.extend" => loose_output(32),
        "variable.bind.paint" => json!({
            "type":"object",
            "properties":{
                "nodeId":s(256),
                "target":en(&["fills","strokes"]),
                "index":u(1024),
                "boundVariableId":{"type":["string","null"],"maxLength":256}
            },
            "required":["nodeId","target","index","boundVariableId"],
            "additionalProperties":false
        }),
        "variable.bind.effect" | "variable.bind.layout_grid" => json!({
            "type":"object",
            "properties":{
                "nodeId":s(256),
                "index":u(1024),
                "boundVariableId":{"type":["string","null"],"maxLength":256}
            },
            "required":["nodeId","index","boundVariableId"],
            "additionalProperties":false
        }),
        _ => loose_output(128),
    };
    Some(schema)
}
