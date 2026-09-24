use serde_json::{Map, Value, json};

pub const OPERATIONS: &[&str] = &[
    "annotation.categories.list",
    "annotation.category.create",
    "annotation.category.patch",
    "annotation.category.remove",
    "annotation.node.inspect",
    "annotation.node.set",
    "artifact.read",
    "artifact.release",
    "boolean.exclude",
    "boolean.intersect",
    "boolean.subtract",
    "boolean.union",
    "buzz.asset_type.get",
    "buzz.asset_type.set",
    "buzz.frame.create",
    "buzz.instance.create",
    "buzz.media_content.inspect",
    "buzz.smart_resize",
    "buzz.text_content.inspect",
    "buzz.text_content.set",
    "canvas.grid.inspect",
    "canvas.grid.set",
    "canvas.nodes.move",
    "canvas.row.create",
    "component.description.patch",
    "component.instances.list",
    "component.property.add",
    "component.property.delete",
    "component.property.edit",
    "compose.apply",
    "compose.batch",
    "design_system.import",
    "dev.resources.add",
    "dev.resources.edit",
    "dev.resources.list",
    "dev.resources.remove",
    "document.diff",
    "document.snapshot",
    "export.node",
    "figjam.diagram.create",
    "figjam.gif.create",
    "figjam.link_preview.create",
    "figjam.table.create",
    "figjam.timer.pause",
    "figjam.timer.resume",
    "figjam.timer.start",
    "figjam.timer.status",
    "figjam.timer.stop",
    "file.version.save",
    "font.list",
    "font.variation_axes",
    "instance.properties.patch",
    "library.component.import",
    "library.component_set.import",
    "library.style.import",
    "library.variable.import",
    "library.variable_collections.list",
    "library.variables.list",
    "mode.remove",
    "mode.rename",
    "motion.export",
    "node.flatten",
    "node.inspect.full",
    "node.outline_stroke",
    "prototype.validate",
    "selection.colors",
    "shader.apply_effect",
    "shader.apply_fill",
    "shader.apply_stroke",
    "shader.import",
    "shader.list",
    "slides.row.create",
    "slides.slide.create",
    "slides.view.get",
    "slides.view.set",
    "slot.create",
    "slot.inspect",
    "slot.reset",
    "style.apply",
    "style.create",
    "style.inspect",
    "style.list",
    "style.patch",
    "style.remove",
    "text.hyperlink.set",
    "text.path.create",
    "text.path.inspect",
    "text.range.patch",
    "text.runs.inspect",
    "text.variable.bind_range",
    "transform_group.create",
    "transform_group.inspect",
    "validate.a11y",
    "validate.components",
    "validate.design_system",
    "validate.layout",
    "validate.variables",
    "variable.code_syntax.remove",
    "variable.code_syntax.set",
    "variable.collection.inspect",
    "variable.collection.remove",
    "variable.collection.rename",
    "variable.inspect",
    "variable.mode.clear_explicit",
    "variable.mode.set_explicit",
    "variable.remove",
    "variable.rename",
    "variable.scopes.set",
    "vector.create",
    "vector.inspect",
    "vector.network.set",
    "viewport.center",
    "viewport.fit",
    "viewport.inspect",
    "viewport.zoom",
];

fn s(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn n(min: f64, max: f64) -> Value {
    json!({"type":"number","minimum":min,"maximum":max})
}
fn u(max: u64) -> Value {
    json!({"type":"integer","minimum":0,"maximum":max})
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
fn node_id() -> Value {
    input(vec![("nodeId", s(256))], &["nodeId"])
}
fn finding_output() -> Value {
    json!({
        "type":"object",
        "properties":{
            "findings":arr(500,obj(32)),
            "truncated":{"type":"boolean"}
        },
        "required":["findings"],
        "additionalProperties":false
    })
}

pub fn input_schema(name: &str) -> Option<Value> {
    if !OPERATIONS.contains(&name) {
        return None;
    }
    let schema = match name {
        "document.snapshot" => input(vec![("mode", en(&["identity", "portable"]))], &[]),
        "document.diff" => input(
            vec![
                ("before", obj(4096)),
                ("after", obj(4096)),
                ("limit", u(1000)),
            ],
            &["before", "after"],
        ),
        "node.inspect.full"
        | "node.outline_stroke"
        | "vector.inspect"
        | "text.path.inspect"
        | "slot.inspect"
        | "component.instances.list"
        | "annotation.node.inspect"
        | "buzz.asset_type.get"
        | "buzz.text_content.inspect"
        | "buzz.media_content.inspect"
        | "transform_group.inspect" => node_id(),
        "compose.apply" => input(vec![("root", obj(64))], &["root"]),
        "compose.batch" => input(vec![("roots", arr(128, obj(64)))], &["roots"]),
        "vector.create" => input(
            vec![
                ("name", s(256)),
                ("x", n(-1_000_000.0, 1_000_000.0)),
                ("y", n(-1_000_000.0, 1_000_000.0)),
                ("width", n(0.01, 100_000.0)),
                ("height", n(0.01, 100_000.0)),
                ("vectorNetwork", obj(8)),
            ],
            &[],
        ),
        "vector.network.set" => input(
            vec![("nodeId", s(256)), ("vectorNetwork", obj(8))],
            &["nodeId", "vectorNetwork"],
        ),
        "node.flatten" | "boolean.union" | "boolean.subtract" | "boolean.intersect"
        | "boolean.exclude" => input(
            vec![
                ("nodeIds", arr(128, s(256))),
                ("parentId", s(256)),
                ("index", u(100_000)),
            ],
            &["nodeIds"],
        ),
        "transform_group.create" => input(
            vec![
                ("nodeIds", arr(128, s(256))),
                ("parentId", s(256)),
                ("index", u(100_000)),
                ("modifiers", arr(128, obj(32))),
            ],
            &["nodeIds", "modifiers"],
        ),
        "text.runs.inspect" => input(
            vec![("nodeId", s(256)), ("fields", arr(32, s(64)))],
            &["nodeId"],
        ),
        "text.range.patch" => input(
            vec![
                ("nodeId", s(256)),
                ("start", u(65_536)),
                ("end", u(65_536)),
                ("fontSize", n(0.1, 10_000.0)),
                ("fontName", obj(8)),
                ("fills", arr(64, obj(32))),
                ("fillStyleId", json!({"type":"string","maxLength":256})),
                ("textStyleId", json!({"type":"string","maxLength":256})),
                ("letterSpacing", obj(8)),
                ("textDecoration", s(64)),
                ("textDecorationStyle", s(64)),
                ("textDecorationOffset", obj(8)),
                ("textDecorationThickness", obj(8)),
                ("textDecorationColor", obj(16)),
                ("textDecorationSkipInk", json!({"type":"boolean"})),
                ("textCase", s(64)),
                ("lineHeight", obj(8)),
                ("listOptions", obj(16)),
                ("listSpacing", n(0.0, 100_000.0)),
                ("indentation", n(0.0, 100_000.0)),
                ("paragraphIndent", n(0.0, 100_000.0)),
                ("paragraphSpacing", n(0.0, 100_000.0)),
                ("textWrapStyle", s(64)),
            ],
            &["nodeId", "start", "end"],
        ),
        "text.hyperlink.set" => input(
            vec![
                ("nodeId", s(256)),
                ("start", u(65_536)),
                ("end", u(65_536)),
                ("hyperlink", obj(8)),
            ],
            &["nodeId", "start", "end"],
        ),
        "text.variable.bind_range" => input(
            vec![
                ("nodeId", s(256)),
                ("start", u(65_536)),
                ("end", u(65_536)),
                ("field", s(128)),
                ("variableId", s(256)),
            ],
            &["nodeId", "start", "end", "field"],
        ),
        "text.path.create" => input(
            vec![
                ("nodeId", s(256)),
                ("startSegment", u(100_000)),
                ("startPosition", n(0.0, 1.0)),
                ("characters", json!({"type":"string","maxLength":65536})),
            ],
            &["nodeId"],
        ),
        "font.list"
        | "style.list"
        | "library.variable_collections.list"
        | "shader.list"
        | "selection.colors"
        | "prototype.validate"
        | "validate.layout"
        | "validate.variables"
        | "validate.components"
        | "validate.design_system"
        | "figjam.timer.status"
        | "figjam.timer.pause"
        | "figjam.timer.resume"
        | "figjam.timer.stop"
        | "canvas.grid.inspect"
        | "slides.view.get"
        | "viewport.inspect" => no_args(),
        "font.variation_axes" => input(vec![("family", s(256))], &["family"]),
        "style.inspect" | "style.remove" => input(vec![("styleId", s(256))], &["styleId"]),
        "style.create" => input(
            vec![
                ("styleType", en(&["PAINT", "TEXT", "EFFECT", "GRID"])),
                ("name", s(256)),
                ("description", json!({"type":"string","maxLength":4096})),
            ],
            &["styleType", "name"],
        ),
        "style.patch" => input(
            vec![
                ("styleId", s(256)),
                ("name", s(256)),
                ("description", json!({"type":"string","maxLength":4096})),
                ("paints", arr(64, obj(32))),
                ("effects", arr(32, obj(32))),
                ("layoutGrids", arr(32, obj(32))),
                ("fontName", obj(8)),
                ("fontSize", n(0.1, 10_000.0)),
            ],
            &["styleId"],
        ),
        "style.apply" => input(
            vec![
                ("nodeId", s(256)),
                ("styleId", s(256)),
                ("field", en(&["fill", "stroke", "text", "effect", "grid"])),
            ],
            &["nodeId", "styleId", "field"],
        ),

        "component.property.add" => input(
            vec![
                ("nodeId", s(256)),
                ("name", s(256)),
                (
                    "propertyType",
                    en(&["BOOLEAN", "TEXT", "INSTANCE_SWAP", "VARIANT", "SLOT"]),
                ),
                ("defaultValue", json!({})),
                ("options", obj(16)),
            ],
            &["nodeId", "name", "propertyType", "defaultValue"],
        ),
        "component.property.edit" => input(
            vec![
                ("nodeId", s(256)),
                ("propertyName", s(256)),
                ("patch", obj(16)),
            ],
            &["nodeId", "propertyName", "patch"],
        ),
        "component.property.delete" => input(
            vec![("nodeId", s(256)), ("propertyName", s(256))],
            &["nodeId", "propertyName"],
        ),
        "component.description.patch" => input(
            vec![
                ("nodeId", s(256)),
                ("description", json!({"type":"string","maxLength":4096})),
                (
                    "descriptionMarkdown",
                    json!({"type":"string","maxLength":16384}),
                ),
            ],
            &["nodeId"],
        ),
        "instance.properties.patch" => input(
            vec![("nodeId", s(256)), ("properties", obj(64))],
            &["nodeId", "properties"],
        ),
        "slot.create" => input(
            vec![("componentId", s(256)), ("name", s(256))],
            &["componentId"],
        ),
        "slot.reset" => node_id(),
        "library.variables.list" => input(vec![("collectionKey", s(256))], &["collectionKey"]),
        "library.component.import"
        | "library.component_set.import"
        | "library.style.import"
        | "library.variable.import" => input(vec![("key", s(256))], &["key"]),
        "variable.inspect"
        | "variable.rename"
        | "variable.remove"
        | "variable.scopes.set"
        | "variable.code_syntax.set"
        | "variable.code_syntax.remove" => {
            let mut fields = vec![("variableId", s(256))];
            match name {
                "variable.rename" => fields.push(("name", s(256))),
                "variable.scopes.set" => fields.push(("scopes", arr(32, s(64)))),
                "variable.code_syntax.set" => {
                    fields.push(("platform", en(&["WEB", "ANDROID", "iOS"])));
                    fields.push(("value", json!({"type":"string","maxLength":1024})));
                }
                "variable.code_syntax.remove" => {
                    fields.push(("platform", en(&["WEB", "ANDROID", "iOS"])))
                }
                _ => {}
            }
            let required = match name {
                "variable.rename" => vec!["variableId", "name"],
                "variable.scopes.set" => vec!["variableId", "scopes"],
                "variable.code_syntax.set" => vec!["variableId", "platform", "value"],
                "variable.code_syntax.remove" => vec!["variableId", "platform"],
                _ => vec!["variableId"],
            };
            input(fields, &required)
        }
        "variable.collection.inspect"
        | "variable.collection.rename"
        | "variable.collection.remove"
        | "mode.rename"
        | "mode.remove" => {
            let mut fields = vec![("collectionId", s(256))];
            if matches!(name, "mode.rename" | "mode.remove") {
                fields.push(("modeId", s(256)));
            }
            if matches!(name, "variable.collection.rename" | "mode.rename") {
                fields.push(("name", s(256)));
            }
            let required = match name {
                "variable.collection.rename" => vec!["collectionId", "name"],
                "mode.rename" => vec!["collectionId", "modeId", "name"],
                "mode.remove" => vec!["collectionId", "modeId"],
                _ => vec!["collectionId"],
            };
            input(fields, &required)
        }
        "variable.mode.set_explicit" | "variable.mode.clear_explicit" => input(
            vec![
                ("nodeId", s(256)),
                ("collectionId", s(256)),
                ("modeId", s(256)),
            ],
            if name.ends_with("set_explicit") {
                &["nodeId", "collectionId", "modeId"]
            } else {
                &["nodeId", "collectionId"]
            },
        ),
        "design_system.import" => input(vec![("designSystem", obj(512))], &["designSystem"]),
        "validate.a11y" => input(vec![("minTouchTarget", n(1.0, 512.0))], &[]),
        "shader.import" => input(vec![("shaderId", s(256))], &["shaderId"]),
        "shader.apply_fill" | "shader.apply_stroke" | "shader.apply_effect" => input(
            vec![
                ("nodeId", s(256)),
                ("shaderId", s(256)),
                ("properties", obj(64)),
            ],
            &["nodeId", "shaderId"],
        ),
        "viewport.center" => input(
            vec![
                ("x", n(-10_000_000.0, 10_000_000.0)),
                ("y", n(-10_000_000.0, 10_000_000.0)),
            ],
            &["x", "y"],
        ),
        "viewport.zoom" => input(vec![("zoom", n(0.01, 256.0))], &["zoom"]),
        "viewport.fit" => input(vec![("nodeIds", arr(128, s(256)))], &["nodeIds"]),
        "annotation.categories.list" => no_args(),
        "annotation.category.create" => input(
            vec![
                ("label", s(256)),
                (
                    "color",
                    en(&[
                        "yellow", "orange", "red", "pink", "violet", "blue", "teal", "green",
                    ]),
                ),
            ],
            &["label", "color"],
        ),
        "annotation.category.patch" => input(
            vec![
                ("categoryId", s(256)),
                ("label", s(256)),
                (
                    "color",
                    en(&[
                        "yellow", "orange", "red", "pink", "violet", "blue", "teal", "green",
                    ]),
                ),
            ],
            &["categoryId"],
        ),
        "annotation.category.remove" => input(vec![("categoryId", s(256))], &["categoryId"]),
        "annotation.node.set" => input(
            vec![("nodeId", s(256)), ("annotations", arr(64, obj(32)))],
            &["nodeId", "annotations"],
        ),
        "dev.resources.list" => input(
            vec![
                ("nodeId", s(256)),
                ("includeChildren", json!({"type":"boolean"})),
            ],
            &["nodeId"],
        ),
        "dev.resources.add" => input(
            vec![("nodeId", s(256)), ("url", s(2048)), ("name", s(256))],
            &["nodeId", "url"],
        ),
        "dev.resources.edit" => input(
            vec![
                ("nodeId", s(256)),
                ("url", s(2048)),
                ("newUrl", s(2048)),
                ("name", s(256)),
            ],
            &["nodeId", "url"],
        ),
        "dev.resources.remove" => input(
            vec![("nodeId", s(256)), ("url", s(2048))],
            &["nodeId", "url"],
        ),
        "file.version.save" => input(
            vec![
                ("title", s(256)),
                ("description", json!({"type":"string","maxLength":4096})),
            ],
            &["title"],
        ),
        "figjam.table.create" => input(
            vec![("rows", u(50)), ("columns", u(50)), ("name", s(256))],
            &[],
        ),
        "figjam.link_preview.create" => input(vec![("url", s(2048))], &["url"]),
        "figjam.gif.create" => input(vec![("imageHash", s(256))], &["imageHash"]),
        "figjam.timer.start" => input(vec![("seconds", u(86_400))], &["seconds"]),
        "figjam.diagram.create" => input(
            vec![("nodes", arr(128, obj(32))), ("edges", arr(256, obj(16)))],
            &["nodes"],
        ),
        "canvas.grid.set" => input(vec![("rows", arr(100, arr(100, s(256))))], &["rows"]),
        "canvas.row.create" => input(vec![("rowIndex", u(10_000))], &[]),
        "canvas.nodes.move" => input(
            vec![
                ("nodeIds", arr(128, s(256))),
                ("rowIndex", u(10_000)),
                ("columnIndex", u(10_000)),
            ],
            &["nodeIds"],
        ),
        "slides.slide.create" => input(
            vec![("row", u(10_000)), ("column", u(10_000)), ("name", s(256))],
            &[],
        ),
        "slides.row.create" => input(vec![("row", u(10_000))], &[]),
        "slides.view.set" => input(vec![("view", en(&["grid", "single-slide"]))], &["view"]),
        "buzz.frame.create" => input(
            vec![("row", u(10_000)), ("column", u(10_000)), ("name", s(256))],
            &[],
        ),
        "buzz.instance.create" => input(
            vec![
                ("componentId", s(256)),
                ("row", u(10_000)),
                ("column", u(10_000)),
            ],
            &["componentId", "row"],
        ),
        "buzz.asset_type.set" => input(
            vec![("nodeId", s(256)), ("assetType", s(128))],
            &["nodeId", "assetType"],
        ),
        "buzz.text_content.set" => input(
            vec![
                ("nodeId", s(256)),
                ("index", u(1000)),
                ("value", json!({"type":"string","maxLength":65536})),
            ],
            &["nodeId", "index", "value"],
        ),
        "buzz.smart_resize" => input(
            vec![
                ("nodeId", s(256)),
                ("width", n(0.01, 100_000.0)),
                ("height", n(0.01, 100_000.0)),
            ],
            &["nodeId", "width", "height"],
        ),
        "export.node" | "motion.export" => input(
            vec![
                ("nodeId", s(256)),
                (
                    "format",
                    en(&["PNG", "JPG", "SVG", "PDF", "MP4", "GIF", "WEBM"]),
                ),
                ("fps", u(60)),
                ("quality", en(&["LOW", "MEDIUM", "HIGH"])),
                ("loopCount", u(1000)),
                ("constraint", obj(8)),
                ("name", s(256)),
            ],
            &["nodeId"],
        ),
        "artifact.read" => input(
            vec![
                ("token", s(128)),
                ("offset", u(16_777_216)),
                ("length", u(196_608)),
            ],
            &["token"],
        ),
        "artifact.release" => input(vec![("token", s(128))], &["token"]),
        other => unreachable!("semantic-complete operation {other} lacks an input schema"),
    };
    Some(schema)
}

pub fn output_schema(name: &str) -> Option<Value> {
    if !OPERATIONS.contains(&name) {
        return None;
    }
    let schema = match name {
        "style.list"
        | "font.list"
        | "library.variable_collections.list"
        | "library.variables.list"
        | "annotation.categories.list"
        | "text.runs.inspect"
        | "buzz.text_content.inspect"
        | "buzz.media_content.inspect" => arr(1000, obj(64)),
        "validate.a11y"
        | "validate.layout"
        | "validate.variables"
        | "validate.components"
        | "prototype.validate" => finding_output(),
        "validate.design_system" => json!({
            "type":"object",
            "properties":{
                "a11y":finding_output(),
                "layout":finding_output(),
                "variables":finding_output(),
                "components":finding_output()
            },
            "required":["a11y","layout","variables","components"],
            "additionalProperties":false
        }),
        "document.diff" => json!({
            "type":"object",
            "properties":{"changes":arr(1000,obj(16))},
            "required":["changes"],
            "additionalProperties":false
        }),
        "artifact.read" => json!({
            "type":"object",
            "properties":{
                "token":s(128),"offset":u(16_777_216),"nextOffset":u(16_777_216),
                "totalBytes":u(16_777_216),"eof":{"type":"boolean"},
                "base64":{"type":"string","maxLength":300000}
            },
            "required":["token","offset","nextOffset","totalBytes","eof","base64"],
            "additionalProperties":false
        }),
        "export.node" | "motion.export" => json!({
            "type":"object",
            "properties":{"token":s(128),"bytes":u(16_777_216),"mediaType":s(128),"name":s(256)},
            "required":["token","bytes","mediaType","name"],
            "additionalProperties":false
        }),
        "artifact.release"
        | "style.remove"
        | "component.property.delete"
        | "variable.remove"
        | "variable.collection.remove"
        | "mode.remove"
        | "annotation.category.remove" => json!({
            "type":"object",
            "properties":{"removed":{"type":"boolean"},"released":{"type":"boolean"}},
            "additionalProperties":false,
            "maxProperties":2
        }),
        "document.snapshot"
        | "node.inspect.full"
        | "compose.apply"
        | "vector.create"
        | "vector.inspect"
        | "node.flatten"
        | "node.outline_stroke"
        | "boolean.union"
        | "boolean.subtract"
        | "boolean.intersect"
        | "boolean.exclude"
        | "transform_group.create"
        | "transform_group.inspect"
        | "text.path.create"
        | "text.path.inspect"
        | "style.inspect"
        | "style.create"
        | "style.patch"
        | "slot.create"
        | "slot.inspect"
        | "slot.reset"
        | "library.component.import"
        | "library.component_set.import"
        | "library.style.import"
        | "library.variable.import"
        | "variable.inspect"
        | "variable.collection.inspect"
        | "shader.import"
        | "figjam.table.create"
        | "figjam.link_preview.create"
        | "figjam.gif.create"
        | "slides.slide.create"
        | "slides.row.create"
        | "buzz.frame.create"
        | "buzz.instance.create" => obj(128),
        "compose.batch"
        | "figjam.diagram.create"
        | "design_system.import"
        | "canvas.grid.inspect"
        | "viewport.inspect"
        | "selection.colors"
        | "file.version.save" => obj(512),
        _ => obj(128),
    };
    Some(schema)
}
