use serde_json::{Map, Value, json};

fn string(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn boolean() -> Value {
    json!({"type":"boolean"})
}
fn integer(max: u64) -> Value {
    json!({"type":"integer","minimum":0,"maximum":max})
}
fn number(min: f64, max: f64) -> Value {
    json!({"type":"number","minimum":min,"maximum":max})
}
fn enum_string(values: &[&str]) -> Value {
    json!({"type":"string","enum":values})
}
fn array(max: usize, items: Value) -> Value {
    json!({"type":"array","maxItems":max,"items":items})
}
fn payload(max: usize) -> Value {
    json!({"type":"object","maxProperties":max})
}
fn input(fields: Vec<(&str, Value)>, required: &[&str]) -> Value {
    let mut properties = Map::new();
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
fn id(field: &'static str) -> Value {
    input(vec![(field, string(512))], &[field])
}
fn file_key() -> Value {
    id("fileKey")
}
fn paged_id(field: &'static str) -> Value {
    input(
        vec![
            (field, string(512)),
            ("pageSize", integer(1000)),
            ("before", integer(u64::MAX)),
            ("after", integer(u64::MAX)),
            ("cursor", string(2048)),
        ],
        &[field],
    )
}
fn strict_dev_resource_create() -> Value {
    json!({
        "type":"object",
        "properties":{
            "name":string(512),
            "url":string(2048),
            "fileKey":string(512),
            "nodeId":string(512)
        },
        "required":["name","url","fileKey","nodeId"],
        "additionalProperties":false
    })
}
fn strict_dev_resource_update() -> Value {
    json!({
        "type":"object",
        "properties":{
            "id":string(512),
            "name":string(512),
            "url":string(2048)
        },
        "required":["id"],
        "additionalProperties":false
    })
}
fn analytics(group: &[&str], dated: bool) -> Value {
    let mut fields = vec![
        ("fileKey", string(512)),
        ("cursor", string(2048)),
        ("groupBy", enum_string(group)),
    ];
    if dated {
        fields.push(("startDate", string(32)));
        fields.push(("endDate", string(32)));
    }
    input(fields, &["fileKey", "groupBy"])
}
fn envelope() -> Value {
    json!({
        "type":"object",
        "properties":{
            "source":{"type":"string","const":"figma_rest"},
            "operation":string(160),
            "operation_id":string(160),
            "scope":string(160),
            "deprecated":{"type":"boolean"},
            "data":{}
        },
        "required":["source","operation","operation_id","scope","deprecated","data"],
        "additionalProperties":false
    })
}

pub fn input_schema(name: &str) -> Option<Value> {
    let schema = match name {
        "cloud.status" => no_args(),
        "cloud.discovery.text_events" => input(
            vec![
                ("startDate", string(64)),
                ("endDate", string(64)),
                (
                    "fileTtlInSeconds",
                    json!({"type":"integer","minimum":60,"maximum":86400}),
                ),
            ],
            &["startDate"],
        ),
        "cloud.file.metadata"
        | "cloud.file.components"
        | "cloud.file.component_sets"
        | "cloud.file.styles"
        | "cloud.image_fills.list"
        | "cloud.versions.list"
        | "cloud.comments.list"
        | "cloud.variables.local"
        | "cloud.variables.published" => match name {
            "cloud.versions.list" => input(
                vec![
                    ("fileKey", string(512)),
                    ("pageSize", integer(50)),
                    ("before", integer(u64::MAX)),
                    ("after", integer(u64::MAX)),
                ],
                &["fileKey"],
            ),
            "cloud.comments.list" => input(
                vec![("fileKey", string(512)), ("asMarkdown", boolean())],
                &["fileKey"],
            ),
            _ => file_key(),
        },
        "cloud.file.get" => input(
            vec![
                ("fileKey", string(512)),
                ("version", string(512)),
                ("ids", array(200, string(512))),
                ("depth", integer(99)),
                ("geometry", boolean()),
                ("pluginData", array(64, string(256))),
                ("branchData", boolean()),
            ],
            &["fileKey"],
        ),
        "cloud.file.nodes" => input(
            vec![
                ("fileKey", string(512)),
                ("ids", array(200, string(512))),
                ("version", string(512)),
                ("depth", integer(99)),
                ("geometry", boolean()),
                ("pluginData", array(64, string(256))),
            ],
            &["fileKey", "ids"],
        ),
        "cloud.images.render" => input(
            vec![
                ("fileKey", string(512)),
                ("ids", array(200, string(512))),
                ("version", string(512)),
                ("scale", number(0.01, 4.0)),
                ("format", enum_string(&["jpg", "png", "svg", "pdf"])),
                ("svgOutlineText", boolean()),
                ("svgIncludeId", boolean()),
                ("svgIncludeNodeId", boolean()),
                ("svgSimplifyStroke", boolean()),
                ("contentsOnly", boolean()),
                ("useAbsoluteBounds", boolean()),
            ],
            &["fileKey", "ids"],
        ),
        "cloud.comment.create" => input(
            vec![
                ("fileKey", string(512)),
                ("message", string(10_000)),
                ("commentId", string(512)),
                ("clientMeta", payload(32)),
            ],
            &["fileKey", "message"],
        ),
        "cloud.comment.delete" => input(
            vec![("fileKey", string(512)), ("commentId", string(512))],
            &["fileKey", "commentId"],
        ),
        "cloud.reactions.list" => input(
            vec![
                ("fileKey", string(512)),
                ("commentId", string(512)),
                ("cursor", string(2048)),
            ],
            &["fileKey", "commentId"],
        ),
        "cloud.reaction.add" | "cloud.reaction.delete" => input(
            vec![
                ("fileKey", string(512)),
                ("commentId", string(512)),
                ("emoji", string(64)),
            ],
            &["fileKey", "commentId", "emoji"],
        ),
        "cloud.team.folders"
        | "cloud.team.components"
        | "cloud.team.component_sets"
        | "cloud.team.styles"
        | "cloud.legacy.team.projects"
        | "cloud.legacy.team.webhooks" => {
            if matches!(
                name,
                "cloud.team.components" | "cloud.team.component_sets" | "cloud.team.styles"
            ) {
                paged_id("teamId")
            } else {
                id("teamId")
            }
        }
        "cloud.folder.files" => input(
            vec![("folderId", string(512)), ("branchData", boolean())],
            &["folderId"],
        ),
        "cloud.folder.subfolders" | "cloud.folder.metadata" => id("folderId"),
        "cloud.legacy.project.files" => input(
            vec![("projectId", string(512)), ("branchData", boolean())],
            &["projectId"],
        ),
        "cloud.legacy.project.metadata" => id("projectId"),
        "cloud.library.component.get"
        | "cloud.library.component_set.get"
        | "cloud.library.style.get" => id("key"),
        "cloud.webhooks.list" => input(
            vec![
                ("context", enum_string(&["team", "project", "file"])),
                ("contextId", string(512)),
                ("planApiId", string(512)),
                ("cursor", string(2048)),
            ],
            &[],
        ),
        "cloud.webhook.get" | "cloud.webhook.delete" | "cloud.webhook.requests" => id("webhookId"),
        "cloud.webhook.create" => input(
            vec![
                ("eventType", string(128)),
                ("context", enum_string(&["team", "project", "file"])),
                ("contextId", string(512)),
                ("endpoint", string(2048)),
                ("passcode", string(100)),
                ("status", string(64)),
                ("description", string(150)),
            ],
            &["eventType", "context", "contextId", "endpoint", "passcode"],
        ),
        "cloud.webhook.update" => input(
            vec![
                ("webhookId", string(512)),
                ("eventType", string(128)),
                ("endpoint", string(2048)),
                ("passcode", string(100)),
                ("status", string(64)),
                ("description", string(150)),
            ],
            &["webhookId"],
        ),
        "cloud.activity_logs.list" => input(
            vec![
                ("events", array(64, string(128))),
                ("startTime", integer(u64::MAX)),
                ("endTime", integer(u64::MAX)),
                ("limit", integer(1000)),
                ("order", enum_string(&["asc", "desc"])),
            ],
            &[],
        ),
        "cloud.developer_logs.query" => input(
            vec![
                (
                    "tokenType",
                    enum_string(&["plan_access_token", "developer_token", "oauth_token"]),
                ),
                ("tokens", array(64, string(512))),
                ("tokenNames", array(64, string(512))),
                ("userEmails", array(64, string(512))),
                ("ipAddresses", array(64, string(128))),
                ("eventSource", enum_string(&["rest_api", "mcp_server"])),
                (
                    "dateRange",
                    enum_string(&["last_24h", "last_7d", "last_30d"]),
                ),
                ("limit", integer(100)),
                ("cursor", string(2048)),
            ],
            &[],
        ),
        "cloud.ai_usage.daily" => input(
            vec![
                ("startDate", string(32)),
                ("endDate", string(32)),
                ("userEmail", string(512)),
                ("limit", integer(1000)),
                ("cursor", string(2048)),
            ],
            &["startDate", "endDate"],
        ),
        "cloud.payments.lookup" => input(
            vec![
                ("pluginPaymentToken", string(2048)),
                ("userId", string(512)),
                ("communityFileId", string(512)),
                ("pluginId", string(512)),
                ("widgetId", string(512)),
            ],
            &[],
        ),
        "cloud.user.current" => no_args(),
        "cloud.oembed.get" => input(
            vec![
                ("url", string(2048)),
                ("maxWidth", integer(10_000)),
                ("maxHeight", integer(10_000)),
            ],
            &["url"],
        ),
        "cloud.dev_resources.list" => input(
            vec![
                ("fileKey", string(512)),
                ("nodeIds", array(200, string(512))),
            ],
            &["fileKey"],
        ),
        "cloud.dev_resources.create" => input(
            vec![("resources", array(200, strict_dev_resource_create()))],
            &["resources"],
        ),
        "cloud.dev_resources.update" => input(
            vec![("resources", array(200, strict_dev_resource_update()))],
            &["resources"],
        ),
        "cloud.dev_resource.delete" => input(
            vec![("fileKey", string(512)), ("devResourceId", string(512))],
            &["fileKey", "devResourceId"],
        ),
        "cloud.variables.mutate" => input(
            vec![
                ("fileKey", string(512)),
                ("variableCollections", array(200, payload(32))),
                ("variableModes", array(400, payload(32))),
                ("variables", array(1000, payload(48))),
                ("variableModeValues", array(4000, payload(24))),
            ],
            &["fileKey"],
        ),
        "cloud.library_analytics.component.actions" => analytics(&["component", "team"], true),
        "cloud.library_analytics.component.usages" => analytics(&["component", "file"], false),
        "cloud.library_analytics.style.actions" => analytics(&["style", "team"], true),
        "cloud.library_analytics.style.usages" => analytics(&["style", "file"], false),
        "cloud.library_analytics.variable.actions" => analytics(&["variable", "team"], true),
        "cloud.library_analytics.variable.usages" => analytics(&["variable", "file"], false),
        _ => return None,
    };
    Some(schema)
}

pub fn output_schema(name: &str) -> Option<Value> {
    if name == "cloud.status" {
        return Some(json!({
            "type":"object",
            "properties":{
                "configured":{"type":"boolean"},
                "credential_transport":{"type":"string"},
                "api_origin":{"type":"string"},
                "token_exposed_to_agent":{"type":"boolean"}
            },
            "required":["configured","credential_transport","api_origin","token_exposed_to_agent"],
            "additionalProperties":false
        }));
    }
    crate::rest_catalog::metadata(name).map(|_| envelope())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_official_rest_capability_has_strict_input_and_output() {
        for operation in crate::rest_catalog::REST_OPERATIONS
            .iter()
            .chain(crate::rest_catalog::REST_DOCUMENTED_EXTRAS.iter())
        {
            let input = input_schema(operation.capability)
                .unwrap_or_else(|| panic!("missing input schema: {}", operation.capability));
            assert_eq!(input["type"], "object", "{}", operation.capability);
            assert_eq!(
                input["additionalProperties"], false,
                "{}",
                operation.capability
            );
            let output = output_schema(operation.capability)
                .unwrap_or_else(|| panic!("missing output schema: {}", operation.capability));
            assert_eq!(output["type"], "object", "{}", operation.capability);
            assert_eq!(
                output["additionalProperties"], false,
                "{}",
                operation.capability
            );
        }
    }

    #[test]
    fn mutation_payloads_are_not_generic_http() {
        let schema = input_schema("cloud.variables.mutate").unwrap();
        assert!(schema["properties"].get("method").is_none());
        assert!(schema["properties"].get("url").is_none());
        assert!(schema["properties"].get("headers").is_none());
        assert!(schema["properties"].get("variableModeValues").is_some());
    }
}
