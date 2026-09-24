use serde_json::{Map, Value, json};

fn string_schema(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn number_schema(min: f64, max: f64) -> Value {
    json!({"type":"number","minimum":min,"maximum":max})
}
fn integer_schema(max: u64) -> Value {
    json!({"type":"integer","minimum":0,"maximum":max})
}
fn loose_value() -> Value {
    json!({})
}
fn variable_alias_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "type":{"const":"VARIABLE_ALIAS"},
            "id":string_schema(256)
        },
        "required":["type","id"],
        "additionalProperties":false
    })
}
fn color_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "r":number_schema(0.0,1.0),
            "g":number_schema(0.0,1.0),
            "b":number_schema(0.0,1.0),
            "a":number_schema(0.0,1.0)
        },
        "required":["r","g","b"],
        "additionalProperties":false
    })
}
fn variable_value_schema() -> Value {
    json!({
        "oneOf":[
            {"type":"boolean"},
            {"type":"string","maxLength":65536},
            {"type":"number"},
            color_schema(),
            variable_alias_schema(),
            {
                "type":"object",
                "properties":{
                    "color":{"oneOf":[color_schema(),variable_alias_schema()]},
                    "opacity":{"oneOf":[number_schema(0.0,1.0),variable_alias_schema()]}
                },
                "required":["color","opacity"],
                "additionalProperties":false
            },
            {
                "type":"object",
                "properties":{
                    "type":{"type":"string","enum":[
                        "EASE_IN","EASE_OUT","EASE_IN_AND_OUT","LINEAR",
                        "EASE_IN_BACK","EASE_OUT_BACK","EASE_IN_AND_OUT_BACK",
                        "CUSTOM_CUBIC_BEZIER","GENTLE","QUICK","BOUNCY","SLOW",
                        "CUSTOM_SPRING","HOLD"
                    ]}
                },
                "required":["type"],
                "additionalProperties":true,
                "maxProperties":16
            }
        ]
    })
}
fn bounded_object(max: usize) -> Value {
    json!({"type":"object","maxProperties":max})
}
fn bounded_array(max: usize, items: Value) -> Value {
    json!({"type":"array","maxItems":max,"items":items})
}
fn input_object(fields: Vec<(&str, Value)>, required: &[&str]) -> Value {
    let mut properties = Map::new();
    properties.insert("session_id".into(), string_schema(128));
    properties.insert("expected_revision".into(), integer_schema(u64::MAX));
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
    input_object(vec![], &[])
}
fn node_id() -> Value {
    input_object(vec![("nodeId", string_schema(256))], &["nodeId"])
}
fn named_create() -> Value {
    input_object(vec![("name", string_schema(256))], &[])
}
fn node_summary() -> Value {
    json!({
        "type":"object",
        "properties":{
            "id":string_schema(256),
            "type":string_schema(64),
            "name":{"type":"string","maxLength":512},
            "visible":{"type":"boolean"},
            "locked":{"type":"boolean"},
            "x":{"type":"number"},
            "y":{"type":"number"},
            "width":{"type":"number","minimum":0},
            "height":{"type":"number","minimum":0},
            "layoutMode":{"type":"string","maxLength":64}
        },
        "required":["id","type","name"],
        "additionalProperties":false
    })
}

pub fn input_schema(name: &str) -> Value {
    match name {
        "doctor"
        | "pairing.begin"
        | "session.list"
        | "document.status"
        | "document.inspect"
        | "page.list"
        | "page.current.get"
        | "selection.get"
        | "selection.clear"
        | "variable.collection.list"
        | "variable.list"
        | "design_system.extract"
        | "snapshot.selection"
        | "snapshot.design_system"
        | "prototype.flow.list"
        | "motion.styles.list" => no_args(),

        "page.create" | "frame.create" | "section.create" | "component.create" => named_create(),

        "page.current.set" | "page.inspect" | "page.remove" => {
            input_object(vec![("pageId", string_schema(256))], &["pageId"])
        }

        "page.rename" => input_object(
            vec![("pageId", string_schema(256)), ("name", string_schema(256))],
            &["pageId", "name"],
        ),

        "selection.set" => input_object(
            vec![("nodeIds", bounded_array(200, string_schema(256)))],
            &["nodeIds"],
        ),

        "node.get"
        | "node.children"
        | "node.ancestors"
        | "node.tree"
        | "node.remove"
        | "node.clone"
        | "layout.inspect"
        | "text.inspect"
        | "component.inspect"
        | "component_set.inspect"
        | "variant.list"
        | "instance.inspect"
        | "instance.detach"
        | "prototype.reaction.list"
        | "prototype.reaction.clear"
        | "motion.node.inspect"
        | "motion.keyframes.list"
        | "motion.timelines.list"
        | "dev.css" => node_id(),

        "node.search" => input_object(
            vec![
                ("name", string_schema(256)),
                ("nodeType", string_schema(64)),
                ("limit", integer_schema(200)),
            ],
            &[],
        ),

        "node.patch" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("name", string_schema(256)),
                ("visible", json!({"type":"boolean"})),
                ("locked", json!({"type":"boolean"})),
                ("opacity", number_schema(0.0, 1.0)),
                ("x", number_schema(-1_000_000.0, 1_000_000.0)),
                ("y", number_schema(-1_000_000.0, 1_000_000.0)),
            ],
            &["nodeId"],
        ),

        "node.rename" => input_object(
            vec![("nodeId", string_schema(256)), ("name", string_schema(256))],
            &["nodeId", "name"],
        ),

        "node.move" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("x", number_schema(-1_000_000.0, 1_000_000.0)),
                ("y", number_schema(-1_000_000.0, 1_000_000.0)),
            ],
            &["nodeId", "x", "y"],
        ),

        "node.resize" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("width", number_schema(0.01, 100_000.0)),
                ("height", number_schema(0.01, 100_000.0)),
            ],
            &["nodeId", "width", "height"],
        ),

        "node.rotate" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("rotation", number_schema(-360_000.0, 360_000.0)),
            ],
            &["nodeId", "rotation"],
        ),

        "node.reparent" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("parentId", string_schema(256)),
            ],
            &["nodeId", "parentId"],
        ),

        "node.reorder" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("index", integer_schema(100_000)),
            ],
            &["nodeId", "index"],
        ),

        "rect.create" | "ellipse.create" | "line.create" | "polygon.create" | "star.create" => {
            input_object(
                vec![
                    ("name", string_schema(256)),
                    ("x", number_schema(-1_000_000.0, 1_000_000.0)),
                    ("y", number_schema(-1_000_000.0, 1_000_000.0)),
                    ("width", number_schema(0.01, 100_000.0)),
                    ("height", number_schema(0.01, 100_000.0)),
                ],
                &[],
            )
        }

        "text.create" => input_object(
            vec![
                ("characters", json!({"type":"string","maxLength":65_536})),
                ("name", string_schema(256)),
            ],
            &[],
        ),

        "text.patch" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("characters", json!({"type":"string","maxLength":65_536})),
                ("name", string_schema(256)),
            ],
            &["nodeId"],
        ),

        "svg.import" => input_object(
            vec![(
                "svg",
                json!({"type":"string","minLength":1,"maxLength":1_048_576}),
            )],
            &["svg"],
        ),

        "layout.patch" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                (
                    "layoutMode",
                    json!({"type":"string","enum":["NONE","HORIZONTAL","VERTICAL","GRID"]}),
                ),
                (
                    "primaryAxisSizingMode",
                    json!({"type":"string","enum":["FIXED","AUTO"]}),
                ),
                (
                    "counterAxisSizingMode",
                    json!({"type":"string","enum":["FIXED","AUTO"]}),
                ),
                (
                    "primaryAxisAlignItems",
                    json!({"type":"string","enum":["MIN","MAX","CENTER","SPACE_BETWEEN","SPACE_EVENLY","SPACE_AROUND"]}),
                ),
                (
                    "counterAxisAlignItems",
                    json!({"type":"string","enum":["MIN","MAX","CENTER","BASELINE"]}),
                ),
                (
                    "layoutWrap",
                    json!({"type":"string","enum":["NO_WRAP","WRAP"]}),
                ),
                ("itemSpacing", number_schema(-10_000.0, 100_000.0)),
                ("paddingTop", number_schema(0.0, 100_000.0)),
                ("paddingRight", number_schema(0.0, 100_000.0)),
                ("paddingBottom", number_schema(0.0, 100_000.0)),
                ("paddingLeft", number_schema(0.0, 100_000.0)),
            ],
            &["nodeId"],
        ),

        "paint.patch" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("r", number_schema(0.0, 1.0)),
                ("g", number_schema(0.0, 1.0)),
                ("b", number_schema(0.0, 1.0)),
                ("opacity", number_schema(0.0, 1.0)),
            ],
            &["nodeId", "r", "g", "b"],
        ),

        "stroke.patch" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("r", number_schema(0.0, 1.0)),
                ("g", number_schema(0.0, 1.0)),
                ("b", number_schema(0.0, 1.0)),
                ("opacity", number_schema(0.0, 1.0)),
                ("weight", number_schema(0.0, 10_000.0)),
            ],
            &["nodeId", "r", "g", "b"],
        ),

        "effects.patch" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("effects", bounded_array(32, bounded_object(32))),
            ],
            &["nodeId", "effects"],
        ),

        "component.from_node" => input_object(vec![("nodeId", string_schema(256))], &["nodeId"]),

        "component_set.create" => input_object(
            vec![
                ("componentIds", bounded_array(64, string_schema(256))),
                ("name", string_schema(256)),
            ],
            &["componentIds"],
        ),

        "instance.create" => {
            input_object(vec![("componentId", string_schema(256))], &["componentId"])
        }

        "instance.swap" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("componentId", string_schema(256)),
            ],
            &["nodeId", "componentId"],
        ),

        "variable.collection.create" => input_object(vec![("name", string_schema(256))], &["name"]),

        "variable.create" => input_object(
            vec![
                ("collectionId", string_schema(256)),
                ("name", string_schema(256)),
                (
                    "resolvedType",
                    json!({"type":"string","enum":["BOOLEAN","FLOAT","STRING","COLOR","EASING","TIMING"]}),
                ),
            ],
            &["collectionId", "name", "resolvedType"],
        ),

        "variable.set_value" => input_object(
            vec![
                ("variableId", string_schema(256)),
                ("modeId", string_schema(256)),
                ("value", variable_value_schema()),
            ],
            &["variableId", "modeId", "value"],
        ),

        "variable.set_alias" => input_object(
            vec![
                ("variableId", string_schema(256)),
                ("modeId", string_schema(256)),
                ("aliasVariableId", string_schema(256)),
            ],
            &["variableId", "modeId", "aliasVariableId"],
        ),

        "variable.bind" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("field", string_schema(128)),
                ("variableId", string_schema(256)),
            ],
            &["nodeId", "field", "variableId"],
        ),

        "mode.list" => input_object(
            vec![("collectionId", string_schema(256))],
            &["collectionId"],
        ),

        "mode.create" => input_object(
            vec![
                ("collectionId", string_schema(256)),
                ("name", string_schema(256)),
            ],
            &["collectionId", "name"],
        ),

        "snapshot.page" => input_object(
            vec![
                ("pageId", string_schema(256)),
                (
                    "mode",
                    json!({"type":"string","enum":["identity","portable"]}),
                ),
            ],
            &[],
        ),

        "snapshot.subtree" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                (
                    "mode",
                    json!({"type":"string","enum":["identity","portable"]}),
                ),
            ],
            &["nodeId"],
        ),

        "prototype.reaction.set" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("reactions", bounded_array(64, bounded_object(32))),
            ],
            &["nodeId", "reactions"],
        ),

        "prototype.reaction.add" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("reaction", bounded_object(32)),
            ],
            &["nodeId", "reaction"],
        ),

        "prototype.reaction.remove" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("index", integer_schema(63)),
            ],
            &["nodeId", "index"],
        ),

        "motion.spring.normalize" => input_object(
            vec![
                ("mass", number_schema(0.0001, 10_000.0)),
                ("stiffness", number_schema(0.0001, 100_000.0)),
                ("damping", number_schema(0.0001, 100_000.0)),
            ],
            &["mass", "stiffness", "damping"],
        ),

        "motion.style.apply" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("styleId", string_schema(256)),
                ("duration", number_schema(0.0, 3_600.0)),
                ("timelineOffset", number_schema(0.0, 3_600.0)),
                ("props", bounded_object(64)),
            ],
            &["nodeId", "styleId", "duration"],
        ),

        "motion.style.remove" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("styleId", string_schema(256)),
            ],
            &["nodeId", "styleId"],
        ),

        "motion.keyframe.apply" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("field", bounded_object(16)),
                (
                    "track",
                    json!({
                        "type":"object",
                        "properties":{
                            "baseValue":loose_value(),
                            "keyframes":bounded_array(1024, bounded_object(16))
                        },
                        "required":["keyframes"],
                        "additionalProperties":false
                    }),
                ),
            ],
            &["nodeId", "field", "track"],
        ),

        "motion.keyframe.remove" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("field", bounded_object(16)),
            ],
            &["nodeId", "field"],
        ),

        "motion.timeline.set_duration" => input_object(
            vec![
                ("nodeId", string_schema(256)),
                ("timelineId", string_schema(256)),
                ("duration", number_schema(0.0001, 3_600.0)),
            ],
            &["nodeId", "timelineId", "duration"],
        ),

        "figjam.sticky.create" | "figjam.shape.create" | "figjam.section.create" => {
            input_object(vec![("name", string_schema(256))], &[])
        }

        "figjam.connector.create" => input_object(
            vec![("from", string_schema(256)), ("to", string_schema(256))],
            &["from", "to"],
        ),

        "figjam.code_block.create" => input_object(
            vec![("code", json!({"type":"string","maxLength":65_536}))],
            &[],
        ),

        _ => crate::semantic_complete_schemas::input_schema(name)
            .or_else(|| crate::semantic_more_schemas::input_schema(name))
            .or_else(|| crate::semantic_admin_schemas::input_schema(name))
            .unwrap_or_else(|| json!({"not":{}})),
    }
}

fn doctor_output() -> Value {
    json!({
        "type":"object",
        "properties":{
            "healthy":{"type":"boolean"},
            "bridge_protocol":{"type":"integer"},
            "listen_host":{"type":"string"},
            "listen_port":{"type":"integer"},
            "pairing_required":{"type":"boolean"},
            "connected_sessions":{"type":"integer","minimum":0,"maximum":64},
            "motion":{"type":"string"},
            "plugin_api":{"type":"string"}
        },
        "required":["healthy","bridge_protocol","listen_host","listen_port","pairing_required","connected_sessions","motion","plugin_api"],
        "additionalProperties":false
    })
}

fn pairing_output() -> Value {
    json!({
        "type":"object",
        "properties":{
            "listen_host":{"type":"string","const":"127.0.0.1"},
            "listen_port":{"type":"integer","minimum":1,"maximum":65535},
            "pairing_code":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "pairing_code_ephemeral":{"type":"boolean"},
            "sessions":bounded_array(64, bounded_object(16))
        },
        "required":["listen_host","listen_port","pairing_code","pairing_code_ephemeral","sessions"],
        "additionalProperties":false
    })
}

fn session_list_output() -> Value {
    bounded_array(
        64,
        json!({
            "type":"object",
            "properties":{
                "session_id":string_schema(128),
                "document_id":string_schema(256),
                "generation":{"type":"integer","minimum":1},
                "revision":{"type":"integer","minimum":0},
                "editor_type":string_schema(64),
                "capabilities":bounded_array(256, string_schema(128))
            },
            "required":["session_id","document_id","generation","revision","editor_type","capabilities"],
            "additionalProperties":false
        }),
    )
}

pub fn output_schema(name: &str) -> Value {
    match name {
        "doctor" => doctor_output(),
        "pairing.begin" => pairing_output(),
        "session.list" => session_list_output(),
        "document.status" => json!({
            "type":"object",
            "properties":{
                "documentId":string_schema(256),
                "editorType":string_schema(64),
                "revision":{"type":"integer","minimum":0},
                "currentPage":string_schema(256)
            },
            "required":["documentId","editorType","revision","currentPage"],
            "additionalProperties":false
        }),
        "document.inspect" => bounded_object(16),
        "page.list" | "selection.get" | "node.children" | "node.ancestors" | "node.search"
        | "variant.list" => bounded_array(200, node_summary()),
        "page.create"
        | "page.rename"
        | "page.current.get"
        | "page.current.set"
        | "node.get"
        | "node.patch"
        | "node.rename"
        | "node.move"
        | "node.resize"
        | "node.rotate"
        | "node.clone"
        | "node.reparent"
        | "node.reorder"
        | "frame.create"
        | "section.create"
        | "rect.create"
        | "ellipse.create"
        | "line.create"
        | "polygon.create"
        | "star.create"
        | "svg.import"
        | "figjam.sticky.create"
        | "figjam.shape.create"
        | "figjam.connector.create"
        | "figjam.section.create"
        | "figjam.code_block.create" => node_summary(),
        "page.remove" | "node.remove" => json!({
            "type":"object",
            "properties":{"removed":{"type":"boolean"}},
            "required":["removed"],
            "additionalProperties":false
        }),
        "selection.set" | "selection.clear" => json!({
            "type":"object",
            "properties":{"count":{"type":"integer","minimum":0,"maximum":200}},
            "required":["count"],
            "additionalProperties":false
        }),
        "page.inspect"
        | "node.tree"
        | "snapshot.page"
        | "snapshot.subtree"
        | "snapshot.design_system"
        | "design_system.extract" => bounded_object(512),
        "snapshot.selection" => bounded_array(200, node_summary()),
        "layout.inspect" | "layout.patch" => bounded_object(32),
        "paint.patch" | "stroke.patch" | "effects.patch" => node_summary(),
        "text.create" | "text.inspect" | "text.patch" => bounded_object(32),
        "component.create"
        | "component.from_node"
        | "component.inspect"
        | "component_set.create"
        | "component_set.inspect"
        | "instance.create"
        | "instance.inspect"
        | "instance.swap"
        | "instance.detach" => bounded_object(64),
        "variable.collection.list" | "variable.list" => bounded_array(200, bounded_object(32)),
        "variable.collection.create"
        | "variable.create"
        | "variable.set_value"
        | "variable.set_alias"
        | "variable.bind"
        | "mode.create" => bounded_object(32),
        "mode.list" => bounded_array(64, bounded_object(16)),
        "prototype.reaction.list" => bounded_array(64, bounded_object(32)),
        "prototype.flow.list" => bounded_array(200, bounded_object(16)),
        "prototype.reaction.set"
        | "prototype.reaction.add"
        | "prototype.reaction.remove"
        | "prototype.reaction.clear" => json!({
            "type":"object",
            "properties":{"count":{"type":"integer","minimum":0,"maximum":64}},
            "required":["count"],
            "additionalProperties":false
        }),
        "motion.styles.list" => bounded_array(256, bounded_object(32)),
        "motion.node.inspect" => bounded_object(32),
        "motion.keyframes.list" | "motion.timelines.list" => {
            bounded_array(1024, bounded_object(64))
        }
        "motion.spring.normalize" => json!({
            "type":"object",
            "properties":{"bounce":loose_value()},
            "required":["bounce"],
            "additionalProperties":false
        }),
        "motion.style.apply" | "motion.keyframe.apply" => json!({
            "type":"object",
            "properties":{"applied":{"type":"boolean"}},
            "required":["applied"],
            "additionalProperties":false
        }),
        "motion.style.remove" | "motion.keyframe.remove" => json!({
            "type":"object",
            "properties":{"removed":{"type":"boolean"}},
            "required":["removed"],
            "additionalProperties":false
        }),
        "motion.timeline.set_duration" => json!({
            "type":"object",
            "properties":{"duration":{"type":"number","minimum":0}},
            "required":["duration"],
            "additionalProperties":false
        }),
        "dev.css" => bounded_object(256),
        _ => crate::semantic_complete_schemas::output_schema(name)
            .or_else(|| crate::semantic_more_schemas::output_schema(name))
            .or_else(|| crate::semantic_admin_schemas::output_schema(name))
            .unwrap_or_else(|| json!({"not":{}})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_inputs_reject_unknown_properties() {
        for name in [
            "document.status",
            "page.create",
            "node.get",
            "node.move",
            "text.create",
            "prototype.reaction.set",
            "motion.style.apply",
            "figjam.connector.create",
        ] {
            let schema = input_schema(name);
            assert_eq!(schema["additionalProperties"], Value::Bool(false), "{name}");
        }
    }

    #[test]
    fn schemas_are_operation_specific() {
        assert_ne!(input_schema("node.move"), input_schema("node.resize"));
        assert_ne!(output_schema("doctor"), output_schema("page.list"));
    }
}
