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
    "instance.main_component.set",
    "media.fill.apply",
    "motion.playhead.get",
    "motion.apply",
    "motion.preset.apply",
    "motion.stagger",
    "node.plugin_data.get",
    "node.plugin_data.set",
    "node.relaunch_data.get",
    "node.relaunch_data.set",
    "page.divider.create",
    "slice.create",
    "video.create",
    "figjam.stuck_to.set",
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
    "node.properties.inspect",
    "node.properties.patch",
    "canvas.info",
    "canvas.next_position",
    "canvas.arrange",
    "slot.list",
    "slot.preferred.set",
    "slot.content.add",
    "slot.convert",
    "slot.settings.patch",
    "analysis.colors",
    "analysis.typography",
    "analysis.spacing",
    "analysis.clusters",
    "a11y.vision.analyze",
    "a11y.vision.preview",
    "verify.node",
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
    "text.range.inspect",
    "text.range.edit",
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
fn vision_mode_schema() -> Value {
    en(&["protanopia", "deuteranopia", "tritanopia"])
}
fn vision_rgb_schema(with_opacity: bool) -> Value {
    let mut properties = Map::new();
    properties.insert("r".into(), n(0.0, 1.0));
    properties.insert("g".into(), n(0.0, 1.0));
    properties.insert("b".into(), n(0.0, 1.0));
    let mut required = vec!["r", "g", "b"];
    if with_opacity {
        properties.insert("opacity".into(), n(0.0, 1.0));
        required.push("opacity");
    }
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn vision_pair_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "colorA":vision_rgb_schema(true),"colorB":vision_rgb_schema(true),
            "simulatedA":vision_rgb_schema(false),"simulatedB":vision_rgb_schema(false),
            "originalDistance":n(0.0,2.0),"simulatedDistance":n(0.0,2.0),
            "nodeIdsA":arr(32,s(256)),"nodeIdsB":arr(32,s(256))
        },
        "required":["colorA","colorB","simulatedA","simulatedB","originalDistance","simulatedDistance","nodeIdsA","nodeIdsB"],
        "additionalProperties":false
    })
}
pub(crate) fn slot_settings_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "stretchChildOnInsert":{"type":"boolean"},
            "displayEmptyByDefault":{"type":"boolean"},
            "minChildren":{"type":["integer","null"],"minimum":0,"maximum":1024},
            "maxChildren":{"type":["integer","null"],"minimum":0,"maximum":1024},
            "allowPreferredValuesOnly":{"type":"boolean"}
        },
        "additionalProperties":false,
        "maxProperties":5
    })
}
fn motion_property_name_schema() -> Value {
    en(&[
        "CORNER_RADIUS",
        "STROKE_WEIGHT",
        "STACK_SPACING",
        "STACK_PADDING_LEFT",
        "STACK_PADDING_TOP",
        "STACK_PADDING_RIGHT",
        "STACK_PADDING_BOTTOM",
        "WIDTH",
        "HEIGHT",
        "RECTANGLE_TOP_LEFT_CORNER_RADIUS",
        "RECTANGLE_TOP_RIGHT_CORNER_RADIUS",
        "RECTANGLE_BOTTOM_LEFT_CORNER_RADIUS",
        "RECTANGLE_BOTTOM_RIGHT_CORNER_RADIUS",
        "BORDER_TOP_WEIGHT",
        "BORDER_BOTTOM_WEIGHT",
        "BORDER_LEFT_WEIGHT",
        "BORDER_RIGHT_WEIGHT",
        "STACK_COUNTER_SPACING",
        "OPACITY",
        "GRID_ROW_GAP",
        "GRID_COLUMN_GAP",
        "TRANSLATION_X",
        "TRANSLATION_Y",
        "TRANSLATION_XY",
        "ROTATION",
        "SCALE_X",
        "SCALE_Y",
        "SCALE_XY",
        "PATH_TRIM_START",
        "PATH_TRIM_END",
    ])
}
fn motion_effect_name_schema() -> Value {
    en(&[
        "OFFSET_X",
        "OFFSET_Y",
        "RADIUS",
        "SPREAD",
        "COLOR",
        "REFRACTION_RADIUS",
        "SPECULAR_ANGLE",
        "SPECULAR_INTENSITY",
        "CHROMATIC_ABERRATION",
        "SPLAY",
        "REFRACTION_INTENSITY",
        "START_RADIUS",
        "NOISE_SIZE_X",
        "NOISE_SIZE_Y",
        "DENSITY",
        "EFFECT_OPACITY",
        "SECONDARY_COLOR",
    ])
}
pub(crate) fn motion_field_schema() -> Value {
    json!({"oneOf":[
        {"type":"object","properties":{"type":{"const":"PROPERTY"},"name":motion_property_name_schema()},"required":["type","name"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"INDEXED_ITEM"},"collection":{"enum":["fills","strokes"]},"index":u(1024),"propertyId":s(256)},"required":["type","collection","index"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"INDEXED_ITEM"},"collection":{"const":"effects"},"index":u(1024),"field":motion_effect_name_schema()},"required":["type","collection","index","field"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"INDEXED_ITEM"},"collection":{"const":"effects"},"index":u(1024),"propertyId":s(256)},"required":["type","collection","index","propertyId"],"additionalProperties":false}
    ]})
}
fn motion_rgba_schema() -> Value {
    json!({"type":"object","properties":{
        "r":n(0.0,1.0),"g":n(0.0,1.0),"b":n(0.0,1.0),"a":n(0.0,1.0)
    },"required":["r","g","b","a"],"additionalProperties":false})
}
fn motion_easing_schema() -> Value {
    json!({"oneOf":[
        {"type":"object","properties":{"type":{"enum":[
            "EASE_IN","EASE_OUT","EASE_IN_AND_OUT","LINEAR","EASE_IN_BACK","EASE_OUT_BACK",
            "EASE_IN_AND_OUT_BACK","GENTLE","QUICK","BOUNCY","SLOW","HOLD"
        ]}},"required":["type"],"additionalProperties":false},
        {"type":"object","properties":{
            "type":{"const":"CUSTOM_SPRING"},
            "easingFunctionSpring":{"type":"object","properties":{"bounce":n(0.0,1.0)},"required":["bounce"],"additionalProperties":false}
        },"required":["type","easingFunctionSpring"],"additionalProperties":false},
        {"type":"object","properties":{
            "type":{"const":"CUSTOM_CUBIC_BEZIER"},
            "easingFunctionCubicBezier":{"type":"object","properties":{
                "x1":n(-100.0,100.0),"y1":n(-100.0,100.0),"x2":n(-100.0,100.0),"y2":n(-100.0,100.0)
            },"required":["x1","y1","x2","y2"],"additionalProperties":false}
        },"required":["type","easingFunctionCubicBezier"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"VARIABLE_ALIAS"},"id":s(256)},"required":["type","id"],"additionalProperties":false}
    ]})
}
fn motion_keyframe_value_schema() -> Value {
    json!({"oneOf":[
        {"type":"object","properties":{"type":{"const":"FLOAT"},"value":{"type":"number"}},"required":["type","value"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"BOOL"},"value":{"type":"boolean"}},"required":["type","value"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"TEXT_DATA"},"value":{"type":"string","maxLength":65536}},"required":["type","value"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"VECTOR"},"value":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false}},"required":["type","value"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"COLOR"},"value":motion_rgba_schema()},"required":["type","value"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"CIRCLE"},"value":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"},"radius":{"type":"number"}},"required":["x","y","radius"],"additionalProperties":false}},"required":["type","value"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"LINE"},"value":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"},"x2":{"type":"number"},"y2":{"type":"number"}},"required":["x","y","x2","y2"],"additionalProperties":false}},"required":["type","value"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"CIRCLE_POINT"},"value":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"},"radius":{"type":"number"},"angle":{"type":"number"}},"required":["x","y","radius","angle"],"additionalProperties":false}},"required":["type","value"],"additionalProperties":false},
        {"type":"object","properties":{"type":{"const":"COLOR_POINT"},"value":{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"},"color":motion_rgba_schema()},"required":["x","y","color"],"additionalProperties":false}},"required":["type","value"],"additionalProperties":false}
    ]})
}
pub(crate) fn motion_track_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "id":s(256),
            "baseValue":motion_keyframe_value_schema(),
            "keyframes":arr(1024,json!({
                "type":"object",
                "properties":{
                    "id":s(256),
                    "timelinePosition":n(0.0,3600.0),
                    "easing":motion_easing_schema(),
                    "value":motion_keyframe_value_schema()
                },
                "required":["timelinePosition","value"],
                "additionalProperties":false
            }))
        },
        "required":["keyframes"],
        "additionalProperties":false
    })
}
fn motion_macro_easing_schema() -> Value {
    json!({"oneOf":[
        en(&["linear","ease-in","ease-out","ease-in-out","ease-in-back","ease-out-back",
            "ease-in-out-back","gentle","quick","bouncy","slow","hold"]),
        motion_easing_schema()
    ]})
}
fn motion_preset_schema() -> Value {
    en(&[
        "fade-in",
        "fade-out",
        "fade-up",
        "fade-down",
        "slide-left",
        "slide-right",
        "pop",
        "spin",
    ])
}
fn motion_alias_schema() -> Value {
    en(&[
        "x",
        "translateX",
        "y",
        "translateY",
        "translate",
        "move",
        "opacity",
        "fade",
        "rotate",
        "rotation",
        "scale",
        "scaleX",
        "scaleY",
        "radius",
        "cornerRadius",
        "width",
        "w",
        "height",
        "h",
        "strokeWeight",
        "gap",
        "spacing",
        "trimStart",
        "trimEnd",
    ])
}
pub fn input_schema(name: &str) -> Option<Value> {
    if !OPERATIONS.contains(&name) {
        return None;
    }
    let schema = match name {
        "file.thumbnail.get" | "motion.playhead.get" | "history.commit" | "history.undo" => {
            no_args()
        }
        "motion.apply" => input(
            vec![
                (
                    "tracks",
                    arr(
                        64,
                        json!({
                            "type":"object",
                            "properties":{"nodeId":s(256),"field":motion_field_schema(),"track":motion_track_schema()},
                            "required":["nodeId","field","track"],
                            "additionalProperties":false
                        }),
                    ),
                ),
                ("duration", n(0.0001, 3600.0)),
            ],
            &["tracks"],
        ),
        "motion.preset.apply" => input(
            vec![
                ("nodeId", s(256)),
                ("preset", motion_preset_schema()),
                ("duration", n(0.0001, 3600.0)),
                ("at", n(0.0, 3600.0)),
                ("easing", motion_macro_easing_schema()),
                ("distance", n(-100_000.0, 100_000.0)),
                ("scaleFrom", n(-1000.0, 1000.0)),
            ],
            &["nodeId", "preset"],
        ),
        "motion.stagger" => input(
            vec![
                ("nodeIds", arr(128, s(256))),
                ("preset", motion_preset_schema()),
                ("field", motion_alias_schema()),
                (
                    "from",
                    json!({"oneOf":[{"type":"number"},{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false}]}),
                ),
                (
                    "to",
                    json!({"oneOf":[{"type":"number"},{"type":"object","properties":{"x":{"type":"number"},"y":{"type":"number"}},"required":["x","y"],"additionalProperties":false}]}),
                ),
                ("step", n(0.0, 3600.0)),
                ("duration", n(0.0001, 3600.0)),
                ("at", n(0.0, 3600.0)),
                ("easing", motion_macro_easing_schema()),
                ("distance", n(-100_000.0, 100_000.0)),
                ("scaleFrom", n(-1000.0, 1000.0)),
            ],
            &["nodeIds"],
        ),
        "group.create" => input(
            vec![
                ("nodeIds", arr(128, s(256))),
                ("parentId", s(256)),
                ("index", u(100_000)),
            ],
            &["nodeIds"],
        ),
        "group.ungroup" | "figjam.table.inspect" => node_id(),
        "instance.main_component.set" => input(
            vec![
                ("nodeId", s(256)),
                (
                    "componentId",
                    json!({"type":["string","null"],"maxLength":256}),
                ),
            ],
            &["nodeId", "componentId"],
        ),
        "figjam.stuck_to.set" => input(
            vec![
                ("nodeId", s(256)),
                (
                    "targetNodeId",
                    json!({"type":["string","null"],"maxLength":256}),
                ),
            ],
            &["nodeId", "targetNodeId"],
        ),
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
        "node.properties.inspect" => input(
            vec![
                ("nodeId", s(256)),
                ("properties", arr(128, s(128))),
                ("offset", u(10_000)),
                ("limit", u(128)),
            ],
            &["nodeId"],
        ),
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
        "slot.convert" => input(
            vec![
                ("nodeId", s(256)),
                ("name", s(256)),
                ("componentKeys", arr(64, s(256))),
                ("description", json!({"type":"string","maxLength":4096})),
                ("slotSettings", slot_settings_schema()),
            ],
            &["nodeId"],
        ),
        "slot.settings.patch" => input(
            vec![("nodeId", s(256)), ("slotSettings", slot_settings_schema())],
            &["nodeId", "slotSettings"],
        ),
        "a11y.vision.analyze" => input(
            vec![
                ("rootNodeId", s(256)),
                ("modes", arr(3, vision_mode_schema())),
                ("threshold", n(0.0, 2.0)),
                ("minOriginalDistance", n(0.0, 2.0)),
                ("maxPairs", u(500)),
            ],
            &[],
        ),
        "a11y.vision.preview" => input(
            vec![
                ("nodeId", s(256)),
                ("modes", arr(3, vision_mode_schema())),
                ("gap", n(0.0, 100_000.0)),
                ("namePrefix", s(64)),
            ],
            &["nodeId"],
        ),
        "verify.node" => input(
            vec![("nodeId", s(256)), ("scale", n(0.1, 4.0)), ("name", s(256))],
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
        "text.range.inspect" => input(
            vec![("nodeId", s(256)), ("start", u(65_536)), ("end", u(65_536))],
            &["nodeId", "start", "end"],
        ),
        "text.range.edit" => input(
            vec![
                ("nodeId", s(256)),
                ("start", u(65_536)),
                ("end", u(65_536)),
                ("characters", json!({"type":"string","maxLength":65_536})),
                ("useStyle", en(&["BEFORE", "AFTER"])),
            ],
            &["nodeId", "start"],
        ),
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
        "motion.apply" => json!({
            "type":"object",
            "properties":{
                "duration":n(0.0,3600.0),
                "results":arr(64,json!({
                    "type":"object",
                    "properties":{"nodeId":s(256),"field":motion_field_schema(),"end":n(0.0,3600.0)},
                    "required":["nodeId","field","end"],
                    "additionalProperties":false
                }))
            },
            "required":["duration","results"],
            "additionalProperties":false
        }),
        "motion.preset.apply" => json!({
            "type":"object",
            "properties":{
                "nodeId":s(256),"preset":motion_preset_schema(),"duration":n(0.0,3600.0),
                "fields":arr(8,motion_field_schema())
            },
            "required":["nodeId","preset","duration","fields"],
            "additionalProperties":false
        }),
        "motion.stagger" => json!({
            "type":"object",
            "properties":{
                "duration":n(0.0,3600.0),
                "results":arr(128,json!({
                    "type":"object",
                    "properties":{"nodeId":s(256),"offset":n(0.0,3600.0),"fields":arr(8,motion_field_schema())},
                    "required":["nodeId","offset","fields"],
                    "additionalProperties":false
                }))
            },
            "required":["duration","results"],
            "additionalProperties":false
        }),
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
        "instance.main_component.set" => json!({
            "type":"object",
            "properties":{
                "nodeId":s(256),
                "mainComponentId":{"type":["string","null"],"maxLength":256},
                "overridesCleared":{"type":"boolean"}
            },
            "required":["nodeId","mainComponentId","overridesCleared"],
            "additionalProperties":false
        }),
        "figjam.stuck_to.set" => json!({
            "type":"object",
            "properties":{
                "nodeId":s(256),
                "targetNodeId":{"type":["string","null"],"maxLength":256}
            },
            "required":["nodeId","targetNodeId"],
            "additionalProperties":false
        }),
        "slot.convert" => json!({
            "type":"object",
            "properties":{
                "propertyName":s(512),
                "node":loose_output(64),
                "slotSettings":{"oneOf":[{"type":"null"},slot_settings_schema()]}
            },
            "required":["propertyName","node","slotSettings"],
            "additionalProperties":false
        }),
        "slot.settings.patch" => json!({
            "type":"object",
            "properties":{
                "propertyName":s(512),
                "slotSettings":slot_settings_schema()
            },
            "required":["propertyName","slotSettings"],
            "additionalProperties":false
        }),
        "node.properties.inspect" => json!({
            "type":"object",
            "properties":{
                "nodeId":s(256),
                "nodeType":s(64),
                "values":obj(128),
                "unavailable":arr(128,s(128)),
                "offset":u(10_000),
                "nextOffset":{"type":["integer","null"],"minimum":0,"maximum":10_000},
                "totalProperties":u(10_000)
            },
            "required":["nodeId","nodeType","values","unavailable","offset","nextOffset","totalProperties"],
            "additionalProperties":false
        }),
        "node.properties.patch" => json!({
            "type":"object",
            "properties":{
                "nodeId":s(256),
                "nodeType":s(64),
                "changed":arr(32,s(128)),
                "values":obj(32)
            },
            "required":["nodeId","nodeType","changed","values"],
            "additionalProperties":false
        }),
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
        "a11y.vision.analyze" => json!({
            "type":"object",
            "properties":{
                "model":s(64),"metric":s(64),"rootNodeId":s(256),"colorCount":u(500),
                "threshold":n(0.0,2.0),"minOriginalDistance":n(0.0,2.0),
                "results":arr(3,json!({
                    "type":"object",
                    "properties":{
                        "mode":vision_mode_schema(),"pairs":arr(500,vision_pair_schema()),
                        "truncated":{"type":"boolean"}
                    },
                    "required":["mode","pairs","truncated"],"additionalProperties":false
                }))
            },
            "required":["model","metric","rootNodeId","colorCount","threshold","minOriginalDistance","results"],
            "additionalProperties":false
        }),
        "a11y.vision.preview" => json!({
            "type":"object",
            "properties":{
                "sourceNodeId":s(256),
                "previews":arr(3,json!({
                    "type":"object",
                    "properties":{
                        "mode":vision_mode_schema(),"node":loose_output(64),"transformedPaints":u(10000)
                    },
                    "required":["mode","node","transformedPaints"],"additionalProperties":false
                }))
            },
            "required":["sourceNodeId","previews"],"additionalProperties":false
        }),
        "verify.node" => json!({
            "type":"object",
            "properties":{
                "token":s(128),"bytes":u(16_777_216),"mediaType":s(128),"name":s(256),
                "nodeId":s(256),"scale":n(0.1,4.0),"nodeCount":u(2000),"structure":loose_output(128)
            },
            "required":["token","bytes","mediaType","name","nodeId","scale","nodeCount","structure"],
            "additionalProperties":false
        }),
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
        "text.range.inspect" => loose_output(64),
        "text.range.edit" => json!({
            "type":"object",
            "properties":{"start":u(65_536),"end":u(65_536),"characters":{"type":"string","maxLength":65_536}},
            "required":["start","end","characters"],
            "additionalProperties":false
        }),
        _ => loose_output(128),
    };
    Some(schema)
}
