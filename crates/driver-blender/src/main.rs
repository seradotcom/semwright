//! Sandboxed first-party Blender driver.
//!
//! Curated operations reuse the existing allowlisted Blender command implementation. Runtime
//! introspection exposes bounded RNA/operator/add-on metadata but never arbitrary Python or generic
//! operator invocation.
use async_trait::async_trait;
use semwright_driver_sdk::{Capability, Driver, descriptor_digest, serve};
use semwright_protocol::{read_frame, write_frame};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk, unique_id};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    net::UnixStream,
    process::{Child, Command},
    time::{sleep, timeout},
};

const DRIVER_ID: &str = "blender";
const DRIVER_SCOPE: &str = "driver:blender";
const WORKSPACE: &str = "/workspace/workspace";
const LEGACY_DESCRIPTORS: &str = include_str!("../../../schemas/commands.json");
const COMMANDS_PY: &str = include_str!("../../../adapters/blender/semwright_blender/commands.py");
const COMMANDS_JSON: &str =
    include_str!("../../../adapters/blender/semwright_blender/commands.json");
const VALIDATION_PY: &str =
    include_str!("../../../adapters/blender/semwright_blender/validation.py");
const BRIDGE_PY: &str = include_str!("bridge.py");
const SEMANTIC_PY: &str = include_str!("semantic.py");

fn blender_binary() -> Result<&'static str> {
    ["/usr/local/bin/blender", "/usr/bin/blender"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
        .ok_or_else(|| Error::unavailable("A supported Blender binary is required"))
}

fn curated_capabilities() -> Result<Vec<Capability>> {
    let rows: Vec<CommandDescriptor> = serde_json::from_str(LEGACY_DESCRIPTORS)?;
    let mut out = Vec::new();
    for mut descriptor in rows
        .into_iter()
        .filter(|descriptor| descriptor.name.starts_with("blender."))
    {
        descriptor.name = format!("driver.{}", descriptor.name);
        descriptor.requires = vec![DRIVER_SCOPE.into()];
        descriptor.backends = vec![DRIVER_SCOPE.into()];
        let object_type = descriptor
            .name
            .split('.')
            .nth(2)
            .filter(|name| {
                matches!(
                    *name,
                    "scene" | "object" | "collection" | "material" | "render" | "file"
                )
            })
            .map(|name| vec![name.into()])
            .unwrap_or_default();
        let mut tags = vec!["blender".into(), "native".into(), "curated".into()];
        match descriptor.name.as_str() {
            "driver.blender.file.save" => {
                tags.push(semwright_driver_sdk::artifact_output_tag("model/3d")?)
            }
            "driver.blender.render" => {
                tags.push(semwright_driver_sdk::artifact_output_tag("image/raster")?)
            }
            _ => {}
        }
        out.push(Capability {
            descriptor,
            aliases: vec![],
            tags,
            object_types: object_type,
        });
    }
    if out.is_empty() {
        return Err(Error::new(
            ErrorCode::Internal,
            "Built-in Blender descriptors are absent",
        ));
    }
    Ok(out)
}

fn descriptor(
    name: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
    object_types: &[&str],
) -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: name.into(),
            version: "1".into(),
            description: description.into(),
            input_schema,
            output_schema,
            requires: vec![DRIVER_SCOPE.into()],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 10_000,
            dry_run: true,
            interactive_consent: false,
            backends: vec![DRIVER_SCOPE.into()],
        },
        aliases: vec![],
        tags: vec!["blender".into(), "rna".into(), "introspection".into()],
        object_types: object_types.iter().map(|value| (*value).into()).collect(),
    }
}
fn query_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "query":{"type":"string","maxLength":256},
            "limit":{"type":"integer","minimum":1,"maximum":256,"default":50}
        },
        "additionalProperties":false
    })
}

fn introspection_capabilities() -> Vec<Capability> {
    vec![
        descriptor(
            "driver.blender.introspect.summary",
            "Inspect Blender version and bounded RNA/operator/add-on counts",
            json!({"type":"object","additionalProperties":false}),
            json!({
                "type":"object",
                "properties":{
                    "version":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"integer","minimum":0,"maximum":99}},
                    "version_string":{"type":"string","maxLength":128},
                    "rna_types":{"type":"integer","minimum":0,"maximum":100000},
                    "operator_categories":{"type":"integer","minimum":0,"maximum":4096},
                    "operators":{"type":"integer","minimum":0,"maximum":100000},
                    "available_addons":{"type":"integer","minimum":0,"maximum":10000},
                    "enabled_addons":{"type":"integer","minimum":0,"maximum":10000},
                    "arbitrary_python":{"const":false},
                    "generic_operator_invoke":{"const":false}
                },
                "required":["version","version_string","rna_types","operator_categories","operators","available_addons","enabled_addons","arbitrary_python","generic_operator_invoke"],
                "additionalProperties":false
            }),
            &["rna", "operator", "addon"],
        ),
        descriptor(
            "driver.blender.introspect.operators",
            "Search Blender operators and report context availability without invoking them",
            query_schema(),
            json!({
                "type":"object",
                "properties":{
                    "items":{"type":"array","maxItems":256,"items":{
                        "type":"object",
                        "properties":{
                            "id":{"type":"string","maxLength":256},
                            "name":{"type":"string","maxLength":512},
                            "description":{"type":"string","maxLength":2048},
                            "available":{"type":"boolean"},
                            "properties":{"type":"integer","minimum":0,"maximum":4096}
                        },
                        "required":["id","name","description","available","properties"],
                        "additionalProperties":false
                    }},
                    "truncated":{"type":"boolean"}
                },
                "required":["items","truncated"],
                "additionalProperties":false
            }),
            &["operator"],
        ),
        descriptor(
            "driver.blender.introspect.operator.describe",
            "Describe one Blender operator RNA schema without invoking it",
            json!({
                "type":"object",
                "properties":{"id":{"type":"string","minLength":3,"maxLength":256,"pattern":"^[a-z0-9_]+\\.[a-z0-9_]+$"}},
                "required":["id"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "id":{"type":"string","maxLength":256},
                    "name":{"type":"string","maxLength":512},
                    "description":{"type":"string","maxLength":2048},
                    "available":{"type":"boolean"},
                    "properties":{"type":"array","maxItems":128,"items":{"type":"object"}}
                },
                "required":["id","name","description","available","properties"],
                "additionalProperties":false
            }),
            &["operator"],
        ),
        descriptor(
            "driver.blender.introspect.types",
            "Search Blender RNA types and their bounded property counts",
            query_schema(),
            json!({
                "type":"object",
                "properties":{
                    "items":{"type":"array","maxItems":256,"items":{
                        "type":"object",
                        "properties":{
                            "identifier":{"type":"string","maxLength":256},
                            "name":{"type":"string","maxLength":512},
                            "description":{"type":"string","maxLength":2048},
                            "base":{"type":["string","null"],"maxLength":256},
                            "properties":{"type":"integer","minimum":0,"maximum":4096}
                        },
                        "required":["identifier","name","description","base","properties"],
                        "additionalProperties":false
                    }},
                    "truncated":{"type":"boolean"}
                },
                "required":["items","truncated"],
                "additionalProperties":false
            }),
            &["rna"],
        ),
        descriptor(
            "driver.blender.introspect.addons",
            "List bounded add-on modules visible to this sandboxed Blender runtime",
            query_schema(),
            json!({
                "type":"object",
                "properties":{
                    "items":{"type":"array","maxItems":256,"items":{
                        "type":"object",
                        "properties":{
                            "module":{"type":"string","maxLength":512},
                            "name":{"type":"string","maxLength":512},
                            "version":{"type":"string","maxLength":128},
                            "enabled":{"type":"boolean"}
                        },
                        "required":["module","name","version","enabled"],
                        "additionalProperties":false
                    }},
                    "truncated":{"type":"boolean"}
                },
                "required":["items","truncated"],
                "additionalProperties":false
            }),
            &["addon"],
        ),
    ]
}

fn semantic_ref_schema() -> Value {
    json!({"type":"string","minLength":16,"maxLength":1536,"pattern":"^blender-rna/v1/"})
}

fn semantic_root_schema() -> Value {
    json!({"type":"string","enum":["actions","armatures","brushes","cache_files","cameras","collections","curves","fonts","grease_pencils","hair_curves","images","lattices","lights","lightprobes","linestyles","masks","materials","meshes","metaballs","movieclips","node_groups","objects","paint_curves","palettes","particles","pointclouds","scenes","shape_keys","sounds","speakers","textures","volumes","worlds"]})
}

fn semantic_value_schema() -> Value {
    // JSON Schema integer is a subset of number (e.g. 1.0 validates as both).
    // Use anyOf so ordinary Blender float/vector values are not rejected by
    // strict output validation merely because they are mathematically integral.
    json!({"anyOf":[
        {"type":"boolean"},
        {"type":"integer"},
        {"type":"number"},
        {"type":"string","maxLength":2048},
        {"type":"array","maxItems":32,"items":{"anyOf":[{"type":"boolean"},{"type":"integer"},{"type":"number"},{"type":"string","maxLength":2048}]}}
    ]})
}

fn semantic_property_descriptor_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "id":{"type":"string","maxLength":256},
            "name":{"type":"string","maxLength":512},
            "description":{"type":"string","maxLength":2048},
            "type":{"type":"string","maxLength":64},
            "subtype":{"type":"string","maxLength":64},
            "array_length":{"type":"integer","minimum":0,"maximum":4096},
            "animatable":{"type":"boolean"},
            "status":{"type":"string","enum":["managed","read_only","relation","runtime_owned","unsupported_by_design"]},
            "writable":{"type":"boolean"},
            "relation_kind":{"type":"string","enum":["pointer","collection"]},
            "relation_mutation":{"type":"string","enum":["pointer_set","domain_specific"]},
            "reason":{"type":"string","maxLength":2048},
            "minimum":{"type":"number"},
            "maximum":{"type":"number"},
            "enum":{"type":"array","maxItems":128,"items":{"type":"object","properties":{"id":{"type":"string","maxLength":256},"name":{"type":"string","maxLength":512}},"required":["id","name"],"additionalProperties":false}}
        },
        "required":["id","name","description","type","subtype","array_length","animatable","status","writable"],
        "additionalProperties":false
    })
}

fn semantic_descriptor(
    name: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
    risk: Risk,
    idempotency: Idempotency,
) -> Capability {
    let mut capability = descriptor(name, description, input_schema, output_schema, &["rna"]);
    capability.descriptor.risk = risk;
    capability.descriptor.idempotency = idempotency;
    capability.descriptor.dry_run = matches!(risk, Risk::ReadOnly);
    capability.tags = vec!["blender".into(), "rna".into(), "semantic".into()];
    capability
}

fn semantic_capabilities() -> Vec<Capability> {
    let property = semantic_property_descriptor_schema();
    let value = semantic_value_schema();
    let property_id =
        json!({"type":"string","minLength":1,"maxLength":256,"pattern":"^[A-Za-z_][A-Za-z0-9_]*$"});
    vec![
        semantic_descriptor(
            "driver.blender.semantic.summary",
            "Describe the version-pinned bounded Blender RNA semantic substrate",
            json!({"type":"object","additionalProperties":false}),
            json!({"type":"object","properties":{
                "schema":{"const":"blender-rna-semantic/v1"},
                "blender_version":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"integer","minimum":0,"maximum":99}},
                "generation":{"type":"integer","minimum":1},
                "roots":{"type":"array","maxItems":64,"items":{"type":"object","properties":{"root":semantic_root_schema(),"rna_type":{"type":"string","maxLength":256}},"required":["root","rna_type"],"additionalProperties":false}},
                "generic_mutation":{"type":"array","maxItems":16,"items":{"type":"string","maxLength":128}},
                "arbitrary_python":{"const":false},
                "generic_operator_invoke":{"const":false}
            },"required":["schema","blender_version","generation","roots","generic_mutation","arbitrary_python","generic_operator_invoke"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.semantic.types",
            "Search allowlisted persistent Blender RNA root types",
            query_schema(),
            json!({"type":"object","properties":{
                "items":{"type":"array","maxItems":256,"items":{"type":"object","properties":{"root":semantic_root_schema(),"identifier":{"type":"string","maxLength":256},"name":{"type":"string","maxLength":512},"properties":{"type":"integer","minimum":0,"maximum":4096}},"required":["root","identifier","name","properties"],"additionalProperties":false}},
                "truncated":{"type":"boolean"}
            },"required":["items","truncated"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.semantic.type.describe",
            "Describe the complete bounded property classification for one Blender RNA root type",
            json!({"type":"object","properties":{"root":semantic_root_schema()},"required":["root"],"additionalProperties":false}),
            json!({"type":"object","properties":{
                "root":semantic_root_schema(),"identifier":{"type":"string","maxLength":256},"name":{"type":"string","maxLength":512},"description":{"type":"string","maxLength":2048},
                "properties":{"type":"array","maxItems":1024,"items":property.clone()},"truncated":{"type":"boolean"}
            },"required":["root","identifier","name","description","properties","truncated"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.semantic.objects",
            "List revision-bound refs for persistent Blender datablocks in an allowlisted RNA root",
            json!({"type":"object","properties":{"root":semantic_root_schema(),"query":{"type":"string","maxLength":256},"limit":{"type":"integer","minimum":1,"maximum":256,"default":50},"offset":{"type":"integer","minimum":0,"maximum":1000000,"default":0}},"required":["root"],"additionalProperties":false}),
            json!({"type":"object","properties":{
                "items":{"type":"array","maxItems":256,"items":{"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":256},"rna_type":{"type":"string","maxLength":256}},"required":["ref","name","rna_type"],"additionalProperties":false}},
                "truncated":{"type":"boolean"},"generation":{"type":"integer","minimum":1},"offset":{"type":"integer","minimum":0,"maximum":1000000}
            },"required":["items","truncated","generation","offset"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.semantic.object.describe",
            "Describe one revision-bound Blender RNA datablock and classify its properties",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"root":semantic_root_schema(),"name":{"type":"string","maxLength":256},"rna_type":{"type":"string","maxLength":256},"properties":{"type":"array","maxItems":1024,"items":property.clone()},"truncated":{"type":"boolean"}},"required":["ref","root","name","rna_type","properties","truncated"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.semantic.relations",
            "Traverse one RNA pointer/collection relation and return revision-bound child refs",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"limit":{"type":"integer","minimum":1,"maximum":256,"default":50},"offset":{"type":"integer","minimum":0,"maximum":1000000,"default":0}},"required":["ref","property"],"additionalProperties":false}),
            json!({"type":"object","properties":{
                "items":{"type":"array","maxItems":256,"items":{"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":256},"rna_type":{"type":"string","maxLength":256},"index":{"type":"integer","minimum":0,"maximum":1000000}},"required":["ref","name","rna_type","index"],"additionalProperties":false}},
                "truncated":{"type":"boolean"},"generation":{"type":"integer","minimum":1},"offset":{"type":"integer","minimum":0,"maximum":1000000},"relation":property_id.clone(),
                "mutation":{"type":"string","enum":["pointer_set","link_unlink","domain_specific"]}
            },"required":["items","truncated","generation","offset","relation","mutation"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.semantic.query",
            "Filter a persistent RNA root by one bounded scalar property and return semantic refs",
            json!({"type":"object","properties":{"root":semantic_root_schema(),"property":property_id.clone(),"operator":{"type":"string","enum":["eq","ne","lt","lte","gt","gte","contains"]},"value":{"anyOf":[{"type":"boolean"},{"type":"integer"},{"type":"number"},{"type":"string","maxLength":2048}]},"limit":{"type":"integer","minimum":1,"maximum":256,"default":50},"offset":{"type":"integer","minimum":0,"maximum":1000000,"default":0}},"required":["root","property","operator","value"],"additionalProperties":false}),
            json!({"type":"object","properties":{"items":{"type":"array","maxItems":256,"items":{"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":256},"value":{"anyOf":[{"type":"boolean"},{"type":"integer"},{"type":"number"},{"type":"string","maxLength":2048}]}},"required":["ref","name","value"],"additionalProperties":false}},"offset":{"type":"integer","minimum":0,"maximum":1000000},"truncated":{"type":"boolean"},"generation":{"type":"integer","minimum":1},"property":property_id.clone(),"operator":{"type":"string","enum":["eq","ne","lt","lte","gt","gte","contains"]}},"required":["items","offset","truncated","generation","property","operator"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.semantic.rna.describe",
            "Describe any concrete Blender RNA type using the bounded semantic property classifier",
            json!({"type":"object","properties":{"identifier":{"type":"string","minLength":1,"maxLength":256,"pattern":"^[A-Za-z_][A-Za-z0-9_]*$"}},"required":["identifier"],"additionalProperties":false}),
            json!({"type":"object","properties":{"identifier":{"type":"string","maxLength":256},"name":{"type":"string","maxLength":512},"description":{"type":"string","maxLength":2048},"base":{"type":["string","null"],"maxLength":256},"properties":{"type":"array","maxItems":1024,"items":property.clone()},"truncated":{"type":"boolean"}},"required":["identifier","name","description","base","properties","truncated"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.semantic.rename",
            "Rename one named RNA object while rotating revision-bound references safely",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128}},"required":["ref","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"changed":{"type":"boolean"},"generation":{"type":"integer","minimum":1}},"required":["ref","name","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.semantic.custom.list",
            "List bounded Blender custom properties on one RNA object",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"limit":{"type":"integer","minimum":1,"maximum":256,"default":50},"offset":{"type":"integer","minimum":0,"maximum":1000000,"default":0}},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"items":{"type":"array","maxItems":256,"items":{"type":"object","properties":{"key":{"type":"string","maxLength":128},"status":{"type":"string","enum":["managed","unsupported_by_design"]},"value":{"oneOf":[semantic_value_schema(),{"type":"null"}]}},"required":["key","status","value"],"additionalProperties":false}},"offset":{"type":"integer","minimum":0,"maximum":1000000},"truncated":{"type":"boolean"},"generation":{"type":"integer","minimum":1}},"required":["ref","items","offset","truncated","generation"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.semantic.custom.get",
            "Read one bounded Blender custom property",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"key":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z_][A-Za-z0-9_.:-]*$"}},"required":["ref","key"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"key":{"type":"string","maxLength":128},"value":semantic_value_schema()},"required":["ref","key","value"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.semantic.custom.set",
            "Set one bounded JSON-compatible Blender custom property",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"key":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z_][A-Za-z0-9_.:-]*$"},"value":semantic_value_schema()},"required":["ref","key","value"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"key":{"type":"string","maxLength":128},"value":semantic_value_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","key","value","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.semantic.custom.remove",
            "Remove one bounded Blender custom property",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"key":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z_][A-Za-z0-9_.:-]*$"}},"required":["ref","key"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"key":{"type":"string","maxLength":128},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","key","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.asset.load",
            "Load one allowlisted file-backed Blender asset from the owner-granted workspace",
            json!({"type":"object","properties":{
                "root":{"type":"string","enum":["cache_files","fonts","images","movieclips","sounds","volumes"]},
                "path":{"type":"string","minLength":1,"maxLength":4096,"pattern":"^[^/].*$"},
                "name":{"type":"string","minLength":1,"maxLength":128}
            },"required":["root","path","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{
                "ref":semantic_ref_schema(),"root":{"type":"string","enum":["cache_files","fonts","images","movieclips","sounds","volumes"]},
                "name":{"type":"string","maxLength":128},"bytes":{"type":"integer","minimum":0,"maximum":536870912},
                "path":{"type":"string","maxLength":4096},"changed":{"const":true},"generation":{"type":"integer","minimum":1}
            },"required":["ref","root","name","bytes","path","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.semantic.object.create",
            "Create one Blender Object with optional typed data and Collection refs at construction time",
            json!({"type":"object","properties":{
                "name":{"type":"string","minLength":1,"maxLength":128},
                "data_ref":{"oneOf":[semantic_ref_schema(),{"type":"null"}]},
                "collection_ref":{"oneOf":[semantic_ref_schema(),{"type":"null"}]}
            },"required":["name"],"additionalProperties":false}),
            json!({"type":"object","properties":{
                "ref":semantic_ref_schema(),
                "name":{"type":"string","maxLength":128},
                "object_type":{"type":"string","maxLength":64},
                "data_ref":{"oneOf":[semantic_ref_schema(),{"type":"null"}]},
                "changed":{"const":true},
                "generation":{"type":"integer","minimum":1}
            },"required":["ref","name","object_type","data_ref","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.semantic.datablock.create",
            "Create one bounded persistent Blender datablock in a supported semantic root",
            json!({"type":"object","properties":{"root":semantic_root_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"kind":{"type":"string","minLength":1,"maxLength":128}},"required":["root","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"root":semantic_root_schema(),"name":{"type":"string","maxLength":128},"rna_type":{"type":"string","maxLength":256},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","root","name","rna_type","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.semantic.datablock.remove",
            "Remove one root Blender datablock through its revision-bound semantic ref",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"root":semantic_root_schema(),"name":{"type":"string","maxLength":256},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["root","name","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.semantic.relation.link",
            "Link one referenced datablock into an RNA collection relation exposing Blender link semantics",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"target_ref":semantic_ref_schema()},"required":["ref","property","target_ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"target_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","property","target_ref","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.semantic.relation.unlink",
            "Unlink one referenced datablock from an RNA collection relation exposing Blender unlink semantics",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"target_ref":semantic_ref_schema()},"required":["ref","property","target_ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"target_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","property","target_ref","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.semantic.relation.set",
            "Set one safe RNA pointer relation to another revision-bound semantic ref or null",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"target_ref":{"oneOf":[semantic_ref_schema(),{"type":"null"}]}},"required":["ref","property","target_ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"target_ref":{"oneOf":[semantic_ref_schema(),{"type":"null"}]},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","property","target_ref","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.armature.bone.add",
            "Create one persistent Armature bone through a controlled Edit Mode transaction",
            json!({"type":"object","properties":{
                "object_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},
                "head":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","minimum":-1000000,"maximum":1000000}},
                "tail":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","minimum":-1000000,"maximum":1000000}},
                "parent_name":{"type":["string","null"],"maxLength":128},"connected":{"type":"boolean","default":false}
            },"required":["object_ref","name","head","tail"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.armature.bone.remove",
            "Remove one revision-bound persistent Armature bone",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"object_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["object_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.armature.bone.parent.set",
            "Set or clear one persistent Armature bone parent using refs from the same armature",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"parent_ref":{"oneOf":[semantic_ref_schema(),{"type":"null"}]},"connected":{"type":"boolean","default":false}},"required":["ref","parent_ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"parent_name":{"type":["string","null"],"maxLength":128},"connected":{"type":"boolean"},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","parent_name","connected","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.mesh.summary",
            "Inspect bounded mesh topology counts through a semantic Mesh ref",
            json!({"type":"object","properties":{"mesh_ref":semantic_ref_schema()},"required":["mesh_ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"vertices":{"type":"integer","minimum":0},"edges":{"type":"integer","minimum":0},"faces":{"type":"integer","minimum":0},"shape_keys":{"type":"integer","minimum":0}},"required":["ref","vertices","edges","faces","shape_keys"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.mesh.geometry.replace",
            "Replace one bounded Mesh topology with validated vertex/edge/face data",
            json!({"type":"object","properties":{
                "mesh_ref":semantic_ref_schema(),
                "vertices":{"type":"array","maxItems":10000,"items":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","minimum":-1000000,"maximum":1000000}}},
                "edges":{"type":"array","maxItems":30000,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"integer","minimum":0,"maximum":9999}}},
                "faces":{"type":"array","maxItems":10000,"items":{"type":"array","minItems":3,"maxItems":64,"items":{"type":"integer","minimum":0,"maximum":9999}}}
            },"required":["mesh_ref","vertices","edges","faces"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"vertices":{"type":"integer","minimum":0},"edges":{"type":"integer","minimum":0},"faces":{"type":"integer","minimum":0},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","vertices","edges","faces","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.mesh.attribute.add",
            "Create one typed mesh attribute whose data elements remain reachable through RNA refs",
            json!({"type":"object","properties":{"mesh_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"data_type":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Z][A-Z0-9_]*$"},"domain":{"type":"string","enum":["POINT","EDGE","FACE","CORNER"]}},"required":["mesh_ref","name","data_type","domain"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"data_type":{"type":"string","maxLength":64},"domain":{"type":"string","maxLength":32},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","data_type","domain","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.mesh.attribute.remove",
            "Remove one revision-bound mesh attribute",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"mesh_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["mesh_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.mesh.uv_layer.add",
            "Create one named UV layer on a referenced Mesh",
            json!({"type":"object","properties":{"mesh_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"do_init":{"type":"boolean","default":true}},"required":["mesh_ref","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.mesh.uv_layer.remove",
            "Remove one revision-bound UV layer",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"mesh_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["mesh_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.vertex_group.add",
            "Create one named vertex group on a mesh Object",
            json!({"type":"object","properties":{"object_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128}},"required":["object_ref","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.vertex_group.remove",
            "Remove one revision-bound vertex group",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"object_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["object_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.vertex_group.weights.set",
            "Set bounded vertex weights on one referenced mesh Object vertex group",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"indices":{"type":"array","maxItems":10000,"items":{"type":"integer","minimum":0,"maximum":99999999}},"weight":{"type":"number","minimum":0.0,"maximum":1.0},"mode":{"type":"string","enum":["REPLACE","ADD","SUBTRACT"],"default":"REPLACE"}},"required":["ref","indices","weight"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"vertices":{"type":"integer","minimum":0,"maximum":10000},"weight":{"type":"number","minimum":0.0,"maximum":1.0},"mode":{"type":"string","enum":["REPLACE","ADD","SUBTRACT"]},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","vertices","weight","mode","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.vertex_group.weights.remove",
            "Remove bounded vertex memberships from one referenced vertex group",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"indices":{"type":"array","maxItems":10000,"items":{"type":"integer","minimum":0,"maximum":99999999}}},"required":["ref","indices"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"vertices":{"type":"integer","minimum":0,"maximum":10000},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","vertices","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.shape_key.add",
            "Add one named shape key to a referenced Object with supported geometry data",
            json!({"type":"object","properties":{"object_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"from_mix":{"type":"boolean","default":false}},"required":["object_ref","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.shape_key.remove",
            "Remove one revision-bound shape key",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"object_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["object_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.curve.spline.add",
            "Add one bounded POLY, BEZIER or NURBS spline to a Curve datablock",
            json!({"type":"object","properties":{"curve_ref":semantic_ref_schema(),"type":{"type":"string","enum":["POLY","BEZIER","NURBS"]},"points":{"type":"integer","minimum":1,"maximum":10000,"default":1}},"required":["curve_ref","type"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"type":{"type":"string","enum":["POLY","BEZIER","NURBS"]},"points":{"type":"integer","minimum":1,"maximum":10000},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","type","points","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.curve.spline.remove",
            "Remove one revision-bound spline from a Curve datablock",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"curve_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["curve_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.pose_constraint.add",
            "Create one typed constraint on a referenced PoseBone",
            json!({"type":"object","properties":{"pose_bone_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"type":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Z][A-Z0-9_]*$"}},"required":["pose_bone_ref","name","type"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"type":{"type":"string","maxLength":64},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","type","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.pose_constraint.remove",
            "Remove one revision-bound PoseBone constraint",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"pose_bone_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["pose_bone_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.bone_collection.add",
            "Create one BoneCollection inside a referenced Armature",
            json!({"type":"object","properties":{"armature_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"parent_ref":{"oneOf":[semantic_ref_schema(),{"type":"null"}]}},"required":["armature_ref","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.bone_collection.remove",
            "Remove one revision-bound BoneCollection",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"armature_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["armature_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.bone_collection.assign",
            "Assign one referenced Bone to a BoneCollection in the same Armature",
            json!({"type":"object","properties":{"collection_ref":semantic_ref_schema(),"bone_ref":semantic_ref_schema()},"required":["collection_ref","bone_ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"collection_ref":semantic_ref_schema(),"bone_ref":semantic_ref_schema(),"assigned":{"const":true},"changed":{"type":"boolean"},"generation":{"type":"integer","minimum":1}},"required":["collection_ref","bone_ref","assigned","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.bone_collection.unassign",
            "Remove one referenced Bone from a BoneCollection in the same Armature",
            json!({"type":"object","properties":{"collection_ref":semantic_ref_schema(),"bone_ref":semantic_ref_schema()},"required":["collection_ref","bone_ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"collection_ref":semantic_ref_schema(),"bone_ref":semantic_ref_schema(),"assigned":{"const":false},"changed":{"type":"boolean"},"generation":{"type":"integer","minimum":1}},"required":["collection_ref","bone_ref","assigned","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.action.slot.add",
            "Create one typed Blender 4.5 Action slot",
            json!({"type":"object","properties":{"action_ref":semantic_ref_schema(),"id_type":{"type":"string","enum":["OBJECT","MESH","CURVE","SURFACE","FONT","META","ARMATURE","LATTICE","CAMERA","LIGHT","SPEAKER","MATERIAL","WORLD","SCENE","KEY","POINTCLOUD","CURVES","GREASEPENCIL","VOLUME"]},"name":{"type":"string","minLength":1,"maxLength":128}},"required":["action_ref","id_type","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"id_type":{"type":"string","maxLength":64},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","id_type","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.action.slot.remove",
            "Remove one revision-bound Action slot and its associated animation",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"action_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["action_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.action.layer.add",
            "Create one named Action animation layer",
            json!({"type":"object","properties":{"action_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128}},"required":["action_ref","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.action.layer.remove",
            "Remove one revision-bound Action layer",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"action_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["action_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.action.strip.add",
            "Create the bounded KEYFRAME strip in a referenced ActionLayer",
            json!({"type":"object","properties":{"layer_ref":semantic_ref_schema()},"required":["layer_ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"type":{"const":"KEYFRAME"},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","type","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.action.strip.remove",
            "Remove one revision-bound Action keyframe strip",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"layer_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["layer_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.action.channelbag.ensure",
            "Ensure one ActionChannelbag for a referenced keyframe strip and Action slot",
            json!({"type":"object","properties":{"strip_ref":semantic_ref_schema(),"slot_ref":semantic_ref_schema()},"required":["strip_ref","slot_ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"created":{"type":"boolean"},"generation":{"type":"integer","minimum":1}},"required":["ref","created","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.action.fcurve.ensure",
            "Ensure one bounded FCurve inside an ActionChannelbag",
            json!({"type":"object","properties":{"channelbag_ref":semantic_ref_schema(),"data_path":{"type":"string","minLength":1,"maxLength":512},"index":{"type":"integer","minimum":0,"maximum":31,"default":0},"group_name":{"type":"string","maxLength":128,"default":""}},"required":["channelbag_ref","data_path"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"data_path":{"type":"string","maxLength":512},"index":{"type":"integer","minimum":0,"maximum":31},"created":{"type":"boolean"},"generation":{"type":"integer","minimum":1}},"required":["ref","data_path","index","created","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.action.fcurve.remove",
            "Remove one revision-bound FCurve from its ActionChannelbag",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"channelbag_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["channelbag_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.nla.track.add",
            "Create one named NLA track for a persistent animation-data owner",
            json!({"type":"object","properties":{"owner_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"previous_ref":{"oneOf":[semantic_ref_schema(),{"type":"null"}]}},"required":["owner_ref","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.nla.track.remove",
            "Remove one revision-bound NLA track",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"owner_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["owner_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.nla.strip.add",
            "Create one Action-backed NLA strip on a referenced track",
            json!({"type":"object","properties":{"track_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"start":{"type":"integer","minimum":-1000000,"maximum":1000000},"action_ref":semantic_ref_schema()},"required":["track_ref","name","start","action_ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"start":{"type":"integer","minimum":-1000000,"maximum":1000000},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","start","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.nla.strip.remove",
            "Remove one revision-bound NLA strip",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"track_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["track_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.fcurve.keyframe.add",
            "Insert or replace one bounded keyframe point on a referenced FCurve",
            json!({"type":"object","properties":{"fcurve_ref":semantic_ref_schema(),"frame":{"type":"number","minimum":-1000000.0,"maximum":1000000.0},"value":{"type":"number","minimum":-1000000000.0,"maximum":1000000000.0},"keyframe_type":{"type":"string","enum":["KEYFRAME","BREAKDOWN","MOVING_HOLD","EXTREME","JITTER","GENERATED"],"default":"KEYFRAME"}},"required":["fcurve_ref","frame","value"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"frame":{"type":"number"},"value":{"type":"number"},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","frame","value","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.fcurve.keyframe.remove",
            "Remove one revision-bound FCurve keyframe point",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"fcurve_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["fcurve_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.fcurve.modifier.add",
            "Create one built-in modifier on a referenced FCurve",
            json!({"type":"object","properties":{"fcurve_ref":semantic_ref_schema(),"type":{"type":"string","enum":["GENERATOR","FNGENERATOR","ENVELOPE","CYCLES","NOISE","LIMITS","STEPPED"]}},"required":["fcurve_ref","type"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"type":{"type":"string","maxLength":64},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","type","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.fcurve.modifier.remove",
            "Remove one revision-bound FCurve modifier",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"fcurve_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["fcurve_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.scene.view_layer.add",
            "Create one named ViewLayer inside a referenced Scene",
            json!({"type":"object","properties":{"scene_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128}},"required":["scene_ref","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.scene.view_layer.remove",
            "Remove one revision-bound ViewLayer while retaining at least one layer",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"scene_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["scene_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.scene.view_layer.move",
            "Move one referenced ViewLayer to a bounded index",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"to_index":{"type":"integer","minimum":0,"maximum":4096}},"required":["ref","to_index"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"index":{"type":"integer","minimum":0,"maximum":4096},"changed":{"type":"boolean"},"generation":{"type":"integer","minimum":1}},"required":["ref","index","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.scene.marker.add",
            "Create one uniquely named timeline marker in a referenced Scene",
            json!({"type":"object","properties":{"scene_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"frame":{"type":"integer","minimum":-1048574,"maximum":1048574}},"required":["scene_ref","name","frame"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"frame":{"type":"integer","minimum":-1048574,"maximum":1048574},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","frame","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.scene.marker.remove",
            "Remove one revision-bound timeline marker",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"scene_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["scene_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.sequence.ensure",
            "Ensure a referenced Scene owns a Sequence Editor and return its semantic ref",
            json!({"type":"object","properties":{"scene_ref":semantic_ref_schema()},"required":["scene_ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"created":{"type":"boolean"},"generation":{"type":"integer","minimum":1}},"required":["ref","created","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.sequence.media.add",
            "Create one scoped image, movie or sound strip from a workspace-relative file",
            json!({"type":"object","properties":{
                "scene_ref":semantic_ref_schema(),
                "kind":{"type":"string","enum":["IMAGE","MOVIE","SOUND"]},
                "name":{"type":"string","minLength":1,"maxLength":128},
                "path":{"type":"string","minLength":1,"maxLength":4096},
                "channel":{"type":"integer","minimum":1,"maximum":128},
                "frame_start":{"type":"integer","minimum":-1048574,"maximum":1048574},
                "fit_method":{"type":"string","enum":["FIT","FILL","STRETCH","ORIGINAL"],"default":"ORIGINAL"}
            },"required":["scene_ref","kind","name","path","channel","frame_start"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"kind":{"type":"string","enum":["IMAGE","MOVIE","SOUND"]},"name":{"type":"string","maxLength":128},"channel":{"type":"integer","minimum":1,"maximum":128},"frame_start":{"type":"integer","minimum":-1048574,"maximum":1048574},"path":{"type":"string","maxLength":4096},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","kind","name","channel","frame_start","path","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.sequence.datablock.add",
            "Create one Scene, MovieClip or Mask strip from a referenced persistent datablock",
            json!({"type":"object","properties":{"scene_ref":semantic_ref_schema(),"kind":{"type":"string","enum":["SCENE","CLIP","MASK"]},"name":{"type":"string","minLength":1,"maxLength":128},"source_ref":semantic_ref_schema(),"channel":{"type":"integer","minimum":1,"maximum":128},"frame_start":{"type":"integer","minimum":-1048574,"maximum":1048574}},"required":["scene_ref","kind","name","source_ref","channel","frame_start"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"kind":{"type":"string","enum":["SCENE","CLIP","MASK"]},"name":{"type":"string","maxLength":128},"channel":{"type":"integer","minimum":1,"maximum":128},"frame_start":{"type":"integer","minimum":-1048574,"maximum":1048574},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","kind","name","channel","frame_start","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.sequence.meta.add",
            "Create one empty meta strip in a referenced Scene Sequence Editor",
            json!({"type":"object","properties":{"scene_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"channel":{"type":"integer","minimum":1,"maximum":128},"frame_start":{"type":"integer","minimum":-1048574,"maximum":1048574}},"required":["scene_ref","name","channel","frame_start"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"channel":{"type":"integer","minimum":1,"maximum":128},"frame_start":{"type":"integer","minimum":-1048574,"maximum":1048574},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","channel","frame_start","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.sequence.effect.add",
            "Create one bounded built-in effect strip with optional same-Scene input strip refs",
            json!({"type":"object","properties":{
                "scene_ref":semantic_ref_schema(),
                "name":{"type":"string","minLength":1,"maxLength":128},
                "type":{"type":"string","enum":["CROSS","ADD","SUBTRACT","ALPHA_OVER","ALPHA_UNDER","GAMMA_CROSS","MULTIPLY","WIPE","GLOW","TRANSFORM","COLOR","SPEED","MULTICAM","ADJUSTMENT","GAUSSIAN_BLUR","TEXT","COLORMIX"]},
                "channel":{"type":"integer","minimum":1,"maximum":128},
                "frame_start":{"type":"integer","minimum":-1048574,"maximum":1048574},
                "frame_end":{"type":"integer","minimum":0,"maximum":1048574,"default":0},
                "input1_ref":{"oneOf":[semantic_ref_schema(),{"type":"null"}]},
                "input2_ref":{"oneOf":[semantic_ref_schema(),{"type":"null"}]}
            },"required":["scene_ref","name","type","channel","frame_start"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"effect_type":{"type":"string","maxLength":64},"channel":{"type":"integer","minimum":1,"maximum":128},"frame_start":{"type":"integer","minimum":-1048574,"maximum":1048574},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","effect_type","channel","frame_start","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.sequence.strip.remove",
            "Remove one revision-bound Sequence strip",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"editor_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["editor_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.sequence.modifier.add",
            "Create one typed modifier on a referenced Sequence strip",
            json!({"type":"object","properties":{"strip_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"type":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Z][A-Z0-9_]*$"}},"required":["strip_ref","name","type"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"type":{"type":"string","maxLength":64},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","type","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.sequence.modifier.remove",
            "Remove one revision-bound Sequence strip modifier",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"strip_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["strip_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.modifier.add",
            "Create one typed Blender modifier on an Object without generic operator invocation",
            json!({"type":"object","properties":{"object_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"type":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Z][A-Z0-9_]*$"}},"required":["object_ref","name","type"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"type":{"type":"string","maxLength":64},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","type","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.modifier.remove",
            "Remove one revision-bound Blender modifier",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"object_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["object_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.constraint.add",
            "Create one typed Blender constraint on an Object without generic operator invocation",
            json!({"type":"object","properties":{"object_ref":semantic_ref_schema(),"name":{"type":"string","minLength":1,"maxLength":128},"type":{"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Z][A-Z0-9_]*$"}},"required":["object_ref","name","type"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"type":{"type":"string","maxLength":64},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","type","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.constraint.remove",
            "Remove one revision-bound Blender constraint",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"object_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["object_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.animation.keyframe.insert",
            "Insert a keyframe only for a generically managed animatable RNA property",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"frame":{"type":"number","minimum":-1000000,"maximum":1000000},"index":{"type":"integer","minimum":-1,"maximum":31,"default":-1}},"required":["ref","property","frame"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"frame":{"type":"number"},"index":{"type":"integer","minimum":-1,"maximum":31},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","property","frame","index","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.animation.keyframe.delete",
            "Delete one keyframe from a managed animatable RNA property",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"frame":{"type":"number","minimum":-1000000,"maximum":1000000},"index":{"type":"integer","minimum":-1,"maximum":31,"default":-1}},"required":["ref","property","frame"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"frame":{"type":"number"},"index":{"type":"integer","minimum":-1,"maximum":31},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","property","frame","index","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.node.types",
            "Discover bounded built-in Blender node classes and explicit executable-code exclusions",
            query_schema(),
            json!({"type":"object","properties":{"items":{"type":"array","maxItems":256,"items":{"type":"object","properties":{"id":{"type":"string","maxLength":256},"name":{"type":"string","maxLength":512},"status":{"type":"string","enum":["managed","unsupported_by_design"]},"reason":{"type":["string","null"],"maxLength":2048}},"required":["id","name","status","reason"],"additionalProperties":false}},"truncated":{"type":"boolean"}},"required":["items","truncated"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.node.interface.socket.add",
            "Create one input or output socket in a referenced NodeTreeInterface",
            json!({"type":"object","properties":{
                "interface_ref":semantic_ref_schema(),
                "name":{"type":"string","minLength":1,"maxLength":128},
                "in_out":{"type":"string","enum":["INPUT","OUTPUT"],"default":"INPUT"},
                "socket_type":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z][A-Za-z0-9_]*$","default":"DEFAULT"},
                "description":{"type":"string","maxLength":2048,"default":""},
                "parent_ref":{"oneOf":[semantic_ref_schema(),{"type":"null"}]}
            },"required":["interface_ref","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"item_type":{"const":"SOCKET"},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","item_type","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.node.interface.panel.add",
            "Create one panel in a referenced NodeTreeInterface",
            json!({"type":"object","properties":{
                "interface_ref":semantic_ref_schema(),
                "name":{"type":"string","minLength":1,"maxLength":128},
                "description":{"type":"string","maxLength":2048,"default":""},
                "default_closed":{"type":"boolean","default":false}
            },"required":["interface_ref","name"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"item_type":{"const":"PANEL"},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","item_type","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.node.interface.item.remove",
            "Remove one revision-bound NodeTree interface item",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"move_content_to_parent":{"type":"boolean","default":true}},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"interface_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["interface_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.node.interface.item.move",
            "Move one NodeTree interface item within its current parent",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"to_position":{"type":"integer","minimum":0,"maximum":4096}},"required":["ref","to_position"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"position":{"type":"integer","minimum":0,"maximum":4096},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","position","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.node.interface.item.move_to_parent",
            "Move one NodeTree interface item into a referenced panel",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"parent_ref":semantic_ref_schema(),"to_position":{"type":"integer","minimum":0,"maximum":4096}},"required":["ref","parent_ref","to_position"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"position":{"type":"integer","minimum":0,"maximum":4096},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","position","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.node.add",
            "Create one bounded non-script node inside a referenced Blender NodeTree",
            json!({"type":"object","properties":{"tree_ref":semantic_ref_schema(),"type":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z][A-Za-z0-9_]*$"},"name":{"type":"string","minLength":1,"maxLength":128}},"required":["tree_ref","type"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"name":{"type":"string","maxLength":128},"node_type":{"type":"string","maxLength":256},"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","name","node_type","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.node.remove",
            "Remove one revision-bound node from its Blender NodeTree",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"tree_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["tree_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.node.link",
            "Create one typed link between two NodeSocket refs in the same NodeTree",
            json!({"type":"object","properties":{"tree_ref":semantic_ref_schema(),"from_socket_ref":semantic_ref_schema(),"to_socket_ref":semantic_ref_schema()},"required":["tree_ref","from_socket_ref","to_socket_ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
        ),
        semantic_descriptor(
            "driver.blender.node.unlink",
            "Remove one revision-bound NodeLink",
            json!({"type":"object","properties":{"ref":semantic_ref_schema()},"required":["ref"],"additionalProperties":false}),
            json!({"type":"object","properties":{"tree_ref":semantic_ref_schema(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["tree_ref","changed","generation"],"additionalProperties":false}),
            Risk::Destructive,
            Idempotency::Destructive,
        ),
        semantic_descriptor(
            "driver.blender.semantic.property.get",
            "Read one bounded scalar/enum/array RNA property from a revision-bound datablock",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone()},"required":["ref","property"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"value":value.clone(),"descriptor":property.clone()},"required":["ref","property","value","descriptor"],"additionalProperties":false}),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
        ),
        semantic_descriptor(
            "driver.blender.semantic.property.set",
            "Set one RNA property through the bounded typed semantic codec and rotate generation",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"value":value.clone()},"required":["ref","property","value"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone(),"value":value.clone(),"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","property","value","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
        semantic_descriptor(
            "driver.blender.semantic.property.reset",
            "Reset one generically managed RNA property to its Blender default and rotate generation",
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id.clone()},"required":["ref","property"],"additionalProperties":false}),
            json!({"type":"object","properties":{"ref":semantic_ref_schema(),"property":property_id,"value":value,"changed":{"const":true},"generation":{"type":"integer","minimum":1}},"required":["ref","property","value","changed","generation"],"additionalProperties":false}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
        ),
    ]
}

fn capabilities() -> Result<Vec<Capability>> {
    let mut values = curated_capabilities()?;
    values.extend(introspection_capabilities());
    values.extend(semantic_capabilities());
    Ok(values)
}

fn capability(command: &str) -> Result<Capability> {
    capabilities()?
        .into_iter()
        .find(|capability| capability.descriptor.name == command)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, "Blender capability is not registered"))
}
fn io_step(step: &str, error: std::io::Error) -> Error {
    Error::new(
        ErrorCode::BackendFailed,
        format!("{step} failed ({:?})", error.kind()),
    )
}

fn private_runtime() -> Result<PathBuf> {
    let path = PathBuf::from(format!("/tmp/semwright-blender-{}", unique_id()));
    fs::create_dir(&path).map_err(|error| io_step("Blender runtime creation", error))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .map_err(|error| io_step("Blender runtime permissions", error))?;
    Ok(path)
}

fn stage_runtime(runtime: &Path) -> Result<(PathBuf, PathBuf)> {
    let package = runtime.join("semwright_blender_runtime");
    fs::create_dir(&package).map_err(|error| io_step("Blender package creation", error))?;
    fs::write(package.join("__init__.py"), b"")
        .map_err(|error| io_step("Blender package init", error))?;
    fs::write(package.join("commands.py"), COMMANDS_PY)
        .map_err(|error| io_step("Blender commands staging", error))?;
    fs::write(package.join("commands.json"), COMMANDS_JSON)
        .map_err(|error| io_step("Blender command schemas staging", error))?;
    fs::write(package.join("validation.py"), VALIDATION_PY)
        .map_err(|error| io_step("Blender validation staging", error))?;
    fs::write(package.join("semantic.py"), SEMANTIC_PY)
        .map_err(|error| io_step("Blender semantic staging", error))?;
    let bridge = runtime.join("bridge.py");
    fs::write(&bridge, BRIDGE_PY).map_err(|error| io_step("Blender bridge staging", error))?;
    Ok((bridge, runtime.join("bridge.sock")))
}

async fn request(socket: &Path, command: &str, args: Value, seconds: u64) -> Result<Value> {
    let mut stream = UnixStream::connect(socket)
        .await
        .map_err(|error| io_step("Blender driver socket connection", error))?;
    write_frame(&mut stream, &json!({"command":command,"args":args})).await?;
    let response: Value = timeout(Duration::from_secs(seconds), read_frame(&mut stream))
        .await
        .map_err(|_| Error::new(ErrorCode::Timeout, "Blender operation timed out"))??;
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        return response
            .get("data")
            .cloned()
            .filter(Value::is_object)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Blender driver result must be an object",
                )
            });
    }
    let code = match response.pointer("/error/code").and_then(Value::as_str) {
        Some("NotFound") => ErrorCode::NotFound,
        Some("Conflict") => ErrorCode::Conflict,
        Some("InvalidArgument") => ErrorCode::InvalidArgument,
        Some("PolicyDenied") => ErrorCode::PolicyDenied,
        Some("Unsupported") => ErrorCode::Unsupported,
        Some("Timeout") => ErrorCode::Timeout,
        Some("StaleReference") => ErrorCode::StaleReference,
        Some("Unavailable") => ErrorCode::Unavailable,
        _ => ErrorCode::BackendFailed,
    };
    Err(Error::new(code, "Blender rejected the typed operation"))
}
struct BlenderDriver {
    child: Child,
    runtime: PathBuf,
    socket: PathBuf,
    version: String,
}

impl BlenderDriver {
    async fn start() -> Result<Self> {
        let blender = blender_binary()?;
        let workspace = fs::metadata(WORKSPACE)
            .map_err(|_| Error::unavailable("Blender driver requires the workspace mount"))?;
        if !workspace.is_dir() {
            return Err(Error::unavailable(
                "Blender workspace mount is not a directory",
            ));
        }

        let runtime = private_runtime()?;
        let (bridge, socket) = stage_runtime(&runtime)?;

        let version_output = timeout(
            Duration::from_secs(5),
            Command::new(blender).arg("--version").output(),
        )
        .await
        .map_err(|_| Error::new(ErrorCode::Timeout, "Blender version probe timed out"))??;
        if !version_output.status.success() || version_output.stdout.len() > 4096 {
            return Err(Error::unavailable("Blender version probe failed"));
        }
        let version_text = String::from_utf8_lossy(&version_output.stdout);
        let version = version_text
            .lines()
            .next()
            .unwrap_or("Blender unknown")
            .trim()
            .chars()
            .take(128)
            .collect::<String>();

        let mut command = Command::new(blender);
        command
            .args([
                "--background",
                "--factory-startup",
                "--disable-autoexec",
                "--python",
            ])
            .arg(&bridge)
            .arg("--")
            .arg(&socket)
            .arg(WORKSPACE)
            .arg(&runtime)
            .env("PYTHONNOUSERSITE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            // Normal DriverProvider execution discards the driver's stderr at the host boundary.
            // Direct owner/CI probes may capture it, which preserves bounded Blender startup
            // diagnostics without contaminating the framed stdout protocol.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|error| io_step("Blender background launch", error))?;
        if let Some(mut stdout) = child.stdout.take() {
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut stdout, &mut tokio::io::sink()).await;
            });
        }
        for _ in 0..240 {
            if child
                .try_wait()
                .map_err(|error| io_step("Blender process status", error))?
                .is_some()
            {
                return Err(Error::unavailable(
                    "Sandbox-owned Blender exited during startup",
                ));
            }
            if socket.exists()
                && request(&socket, "driver.blender.introspect.summary", json!({}), 2)
                    .await
                    .is_ok()
            {
                return Ok(Self {
                    child,
                    runtime,
                    socket,
                    version,
                });
            }
            sleep(Duration::from_millis(50)).await;
        }
        let _ = child.kill().await;
        let _ = fs::remove_dir_all(&runtime);
        Err(Error::new(
            ErrorCode::Timeout,
            "Blender background bridge startup timed out",
        ))
    }
}

impl Drop for BlenderDriver {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = fs::remove_dir_all(&self.runtime);
    }
}

#[async_trait]
impl Driver for BlenderDriver {
    fn id(&self) -> &str {
        DRIVER_ID
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        capabilities()
    }

    async fn execute(&mut self, command: &str, digest: &str, args: Value) -> Result<Value> {
        let capability = capability(command)?;
        if descriptor_digest(&capability.descriptor)? != digest {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Blender capability descriptor changed",
            ));
        }
        let input =
            jsonschema::validator_for(&capability.descriptor.input_schema).map_err(|_| {
                Error::new(ErrorCode::Internal, "Invalid embedded Blender input schema")
            })?;
        if !input.is_valid(&args) {
            return Err(Error::invalid(
                "Blender capability arguments do not match the strict schema",
            ));
        }
        let value = request(
            &self.socket,
            command,
            args,
            (capability.descriptor.timeout_ms / 1000).saturating_add(2),
        )
        .await?;
        let output =
            jsonschema::validator_for(&capability.descriptor.output_schema).map_err(|_| {
                Error::new(
                    ErrorCode::Internal,
                    "Invalid embedded Blender output schema",
                )
            })?;
        if let Some(error) = output.iter_errors(&value).next() {
            eprintln!(
                "Blender output schema rejected command={} at {}: {:?}",
                command, error.instance_path, error.kind
            );
            return Err(Error::new(
                ErrorCode::PluginProtocolError,
                "Blender driver produced output outside its descriptor schema",
            ));
        }
        Ok(value)
    }

    async fn health(&mut self) -> Result<Value> {
        let summary = request(
            &self.socket,
            "driver.blender.introspect.summary",
            json!({}),
            5,
        )
        .await?;
        Ok(json!({
            "healthy":true,
            "runtime":self.version,
            "version":summary["version_string"],
            "arbitrary_python":false,
            "generic_operator_invoke":false
        }))
    }
}

#[tokio::main]
async fn main() {
    let result = match BlenderDriver::start().await {
        Ok(driver) => serve(driver).await,
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_commands_are_migrated_without_generic_python_escape() {
        let capabilities = capabilities().unwrap();
        let names = capabilities
            .iter()
            .map(|capability| capability.descriptor.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(names.contains("driver.blender.object.create"));
        assert!(names.contains("driver.blender.render"));
        assert!(names.contains("driver.blender.introspect.operator.describe"));
        assert!(names.contains("driver.blender.semantic.property.set"));
        assert!(names.contains("driver.blender.semantic.type.describe"));
        assert!(!names.iter().any(|name| name.contains("python")));
        assert!(!names.iter().any(|name| name.ends_with("operator.invoke")));
        for capability in capabilities {
            assert_eq!(capability.descriptor.requires, [DRIVER_SCOPE]);
            assert_eq!(capability.descriptor.backends, [DRIVER_SCOPE]);
        }
    }

    #[test]
    fn catalog_declares_generic_artifact_outputs() {
        let capabilities = capabilities().unwrap();
        let saved = capabilities
            .iter()
            .find(|capability| capability.descriptor.name == "driver.blender.file.save")
            .unwrap();
        assert!(saved.tags.iter().any(|tag| tag == "artifact-out:model/3d"));
        let render = capabilities
            .iter()
            .find(|capability| capability.descriptor.name == "driver.blender.render")
            .unwrap();
        assert!(
            render
                .tags
                .iter()
                .any(|tag| tag == "artifact-out:image/raster")
        );
    }

    #[test]
    fn introspection_inputs_are_bounded() {
        for capability in introspection_capabilities() {
            let serialized = serde_json::to_string(&capability.descriptor.input_schema).unwrap();
            assert!(!serialized.contains("additionalProperties\":true"));
            assert!(capability.descriptor.risk == Risk::ReadOnly);
        }
    }

    #[test]
    fn semantic_catalog_is_unique_bounded_and_non_executable() {
        let catalog = capabilities().unwrap();
        let mut names = std::collections::BTreeSet::new();
        for capability in &catalog {
            assert!(
                names.insert(capability.descriptor.name.clone()),
                "duplicate Blender capability {}",
                capability.descriptor.name
            );
            assert_ne!(capability.descriptor.risk, Risk::CodeExecution);
            jsonschema::validator_for(&capability.descriptor.input_schema)
                .expect("Blender input schema must compile");
            jsonschema::validator_for(&capability.descriptor.output_schema)
                .expect("Blender output schema must compile");
        }
        let identity = semwright_types::ProviderIdentity::external(
            semwright_types::SourceKind::Driver,
            DRIVER_ID,
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
        for capability in &catalog {
            capability.validate_for(&identity).unwrap_or_else(|error| {
                panic!(
                    "{} failed owner validation: {error}",
                    capability.descriptor.name
                )
            });
        }
        let digest = semwright_driver_sdk::capabilities_digest(&catalog).unwrap();
        let response = semwright_driver_sdk::Response::Capabilities {
            id: "b".repeat(64),
            capabilities: catalog,
            digest,
        };
        let bytes = semwright_protocol::encode(&response).unwrap();
        eprintln!(
            "Blender capability frame bytes={} budget={}",
            bytes.len(),
            semwright_types::MAX_FRAME
        );
        assert!(
            bytes.len() <= semwright_types::MAX_FRAME,
            "Blender catalog is {} bytes but protocol budget is {}",
            bytes.len(),
            semwright_types::MAX_FRAME
        );
        assert!(
            bytes.len() < 900_000,
            "Blender capability frame must preserve at least ~148 KiB protocol headroom"
        );
    }

    #[test]
    fn semantic_value_schema_accepts_integral_blender_floats_and_vectors() {
        let validator = jsonschema::validator_for(&semantic_value_schema()).unwrap();
        for value in [
            json!(1),
            json!(1.0),
            json!(0.5),
            json!([1.0, 2.0, 3.0]),
            json!([0.0, 0.25, 1.0]),
            json!("label"),
            json!(true),
        ] {
            assert!(
                validator.is_valid(&value),
                "semantic value rejected: {value}"
            );
        }
        assert!(!validator.is_valid(&Value::Null));
        assert!(!validator.is_valid(&json!({"x":1})));
    }

    #[test]
    fn semantic_source_has_no_generic_python_or_operator_escape() {
        for token in [
            "eval(",
            "exec(",
            "__import__(",
            "subprocess",
            "os.system(",
            r#"generic_operator_invoke":true"#,
        ] {
            assert!(
                !SEMANTIC_PY.contains(token),
                "semantic substrate contains forbidden escape token {token}"
            );
        }
        assert!(!semantic_capabilities().iter().any(|capability| {
            capability.descriptor.name.contains("python")
                || capability.descriptor.name.ends_with("operator.invoke")
        }));
    }

    #[test]
    fn staged_runtime_contains_all_embedded_support_files() {
        let runtime = tempfile::tempdir().unwrap();
        let (bridge, socket) = stage_runtime(runtime.path()).unwrap();
        let package = runtime.path().join("semwright_blender_runtime");

        for file in [
            "__init__.py",
            "commands.py",
            "commands.json",
            "validation.py",
            "semantic.py",
        ] {
            assert!(package.join(file).is_file(), "missing staged file {file}");
        }
        assert_eq!(bridge, runtime.path().join("bridge.py"));
        assert_eq!(socket, runtime.path().join("bridge.sock"));

        let schemas: Value =
            serde_json::from_slice(&std::fs::read(package.join("commands.json")).unwrap()).unwrap();
        assert!(schemas.get("blender.status").is_some());
        assert!(schemas.get("blender.render").is_some());
    }
}
