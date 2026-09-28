use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use std::collections::BTreeMap;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Node {
    id: String,
    kind: String,
    name: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    children: Vec<String>,
    props: BTreeMap<String, Value>,
}

struct Fake {
    revision: u64,
    nodes: BTreeMap<String, Node>,
    page: String,
    motion: bool,
    collections: BTreeMap<String, Value>,
    variables: BTreeMap<String, Value>,
    reactions: BTreeMap<String, Vec<Value>>,
    artifacts: BTreeMap<String, Vec<u8>>,
}

fn summary(node: &Node) -> Value {
    let mut value = json!({
        "id": node.id,
        "type": node.kind,
        "name": node.name,
        "x": node.x,
        "y": node.y,
        "width": node.w,
        "height": node.h,
        "visible": true,
        "locked": false
    });
    if let Some(layout) = node.props.get("layoutMode") {
        value["layoutMode"] = layout.clone();
    }
    value
}

impl Fake {
    fn new() -> Self {
        let page = "0:1".to_string();
        let mut nodes = BTreeMap::new();
        nodes.insert(
            page.clone(),
            Node {
                id: page.clone(),
                kind: "PAGE".into(),
                name: "Page 1".into(),
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
                children: vec![],
                props: BTreeMap::new(),
            },
        );
        Self {
            revision: 0,
            nodes,
            page,
            motion: true,
            collections: BTreeMap::new(),
            variables: BTreeMap::new(),
            reactions: BTreeMap::new(),
            artifacts: BTreeMap::new(),
        }
    }

    fn semantic_measure(&self, root_id: &str) -> Result<Value> {
        let mut queue = vec![root_id.to_owned()];
        let mut measured = Vec::new();
        while let Some(id) = queue.pop() {
            if measured.len() >= 512 {
                break;
            }
            let node = self
                .nodes
                .get(&id)
                .context("semantic root/node not found")?;
            queue.extend(node.children.iter().rev().cloned());
            measured.push(json!({
                "nodeId": node.id,
                "logicalId": node.props.get("logicalId").cloned().unwrap_or(Value::Null),
                "type": node.kind,
                "name": node.name,
                "box": {"x":node.x,"y":node.y,"width":node.w,"height":node.h},
                "layoutMode": node.props.get("layoutMode").cloned().unwrap_or(json!("NONE")),
                "itemSpacing": node.props.get("itemSpacing").cloned().unwrap_or(json!(0)),
                "text": if node.kind == "TEXT" {
                    json!({
                        "characters":node.props.get("characters").cloned().unwrap_or(json!("")),
                        "textAutoResize":"HEIGHT"
                    })
                } else {
                    Value::Null
                }
            }));
        }
        let truncated = measured.len() >= 512;
        Ok(json!({
            "rootNodeId":root_id,
            "nodes":measured,
            "truncated":truncated,
            "observedRevision":self.revision
        }))
    }

    fn semantic_validate(&self, root_id: &str, spec: Option<&Value>) -> Result<Value> {
        let _ = self.nodes.get(root_id).context("semantic root not found")?;
        let mut logical = BTreeMap::<String, String>::new();
        for node in self.nodes.values() {
            if let Some(id) = node.props.get("logicalId").and_then(Value::as_str) {
                logical.insert(id.to_owned(), node.id.clone());
            }
        }
        let mut findings = Vec::<Value>::new();
        let mut unknown = 0u64;
        if let Some(spec) = spec {
            for declared in spec
                .get("nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let logical_id = declared.get("id").and_then(Value::as_str).unwrap_or("");
                let Some(node_id) = logical.get(logical_id) else {
                    findings.push(json!({
                        "severity":"error",
                        "category":"missing_declared_node",
                        "confidence_class":"DETERMINISTIC",
                        "subject_logical_id":logical_id,
                        "related_node_ids":[],
                        "expected":true,
                        "actual":false,
                        "evidence":{"logicalId":logical_id},
                        "suggested_repairs":[]
                    }));
                    continue;
                };
                let node = self
                    .nodes
                    .get(node_id)
                    .expect("logical index points at node");
                if declared.get("kind").and_then(Value::as_str) == Some("text")
                    && node.kind != "TEXT"
                {
                    findings.push(json!({
                        "severity":"error",
                        "category":"native_text_violation",
                        "confidence_class":"DETERMINISTIC",
                        "subject_node_id":node.id,
                        "subject_logical_id":logical_id,
                        "related_node_ids":[],
                        "expected":"TEXT",
                        "actual":node.kind,
                        "evidence":{},
                        "suggested_repairs":[]
                    }));
                }
            }
            for relation in spec
                .get("relationships")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let kind = relation.get("kind").and_then(Value::as_str).unwrap_or("");
                if !matches!(kind, "minimum_gap" | "maximum_gap") {
                    continue;
                }
                let subject = relation
                    .get("subject")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let object = relation.get("object").and_then(Value::as_str).unwrap_or("");
                let (Some(subject_id), Some(object_id)) =
                    (logical.get(subject), logical.get(object))
                else {
                    unknown += 1;
                    continue;
                };
                let parent = self.nodes.values().find(|node| {
                    node.children.contains(subject_id) && node.children.contains(object_id)
                });
                let Some(parent) = parent else {
                    unknown += 1;
                    continue;
                };
                let actual = parent
                    .props
                    .get("itemSpacing")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                let expected = relation.get("value").and_then(Value::as_f64).unwrap_or(0.0);
                let failed = if kind == "minimum_gap" {
                    actual + 0.5 < expected
                } else {
                    actual - 0.5 > expected
                };
                if failed {
                    findings.push(json!({
                        "severity":"warning",
                        "category":"declared_spacing",
                        "confidence_class":"DETERMINISTIC",
                        "subject_node_id":parent.id,
                        "subject_logical_id":subject,
                        "related_node_ids":[object_id],
                        "expected":{"kind":kind,"value":expected},
                        "actual":actual,
                        "evidence":{"parentNodeId":parent.id,"autoLayout":true},
                        "suggested_repairs":[{"kind":"set_auto_layout_gap","gap":expected}]
                    }));
                }
            }
        }
        let failures = findings.len() as u64;
        Ok(json!({
            "status": if failures > 0 {"FAIL"} else if unknown > 0 {"UNKNOWN"} else {"PASS"},
            "findings":findings,
            "summary":{
                "deterministicFailures":failures,
                "unknown":unknown,
                "checkedNodes":self.nodes.len(),
                "truncated":false
            },
            "observedRevision":self.revision
        }))
    }

    fn semantic_apply_declared(node: &mut Node, declared: &Value) -> Result<()> {
        let logical_id = declared
            .get("id")
            .and_then(Value::as_str)
            .context("logical id")?;
        if let Some(name) = declared.get("name").and_then(Value::as_str) {
            node.name = name.to_owned();
        }
        if let Some(sizing) = declared.get("sizing") {
            if let Some(width) = sizing
                .get("width")
                .filter(|axis| axis.get("mode").and_then(Value::as_str) == Some("fixed"))
                .and_then(|axis| axis.get("value"))
                .and_then(Value::as_f64)
            {
                node.w = width;
            }
            if let Some(height) = sizing
                .get("height")
                .filter(|axis| axis.get("mode").and_then(Value::as_str) == Some("fixed"))
                .and_then(|axis| axis.get("value"))
                .and_then(Value::as_f64)
            {
                node.h = height;
            }
        }
        node.props.insert("logicalId".into(), json!(logical_id));
        if let Some(text) = declared.get("text") {
            node.props.insert(
                "characters".into(),
                text.get("characters").cloned().unwrap_or(json!("")),
            );
        }
        let declared_kind = declared
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("frame");
        let layout_mode = match declared_kind {
            "stack" => "VERTICAL",
            "row" | "split" => "HORIZONTAL",
            "grid" => "GRID",
            "overlay" => "NONE",
            _ => declared
                .get("layout")
                .and_then(|layout| layout.get("direction"))
                .and_then(Value::as_str)
                .map(|value| match value {
                    "vertical" => "VERTICAL",
                    "horizontal" => "HORIZONTAL",
                    "grid" => "GRID",
                    _ => "NONE",
                })
                .unwrap_or("NONE"),
        };
        node.props.insert("layoutMode".into(), json!(layout_mode));
        node.props.insert(
            "itemSpacing".into(),
            declared
                .get("layout")
                .and_then(|layout| layout.get("gap"))
                .cloned()
                .unwrap_or(json!(0)),
        );
        Ok(())
    }

    fn execute(&mut self, op: &str, args: &Value) -> Result<Value> {
        match op {
            "document.status" => Ok(json!({
                "documentId":"fake-doc",
                "editorType":"figma",
                "revision":self.revision,
                "currentPage":self.page
            })),
            "composition.inspect" => {
                let root = args
                    .get("root_node_id")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.page);
                Ok(json!({
                    "editorType":"figma",
                    "root":self.semantic_measure(root)?,
                    "designSystem":{"components":[],"variables":[],"textStyles":[]},
                    "limits":{"maxNodes":512,"maxFindings":1000}
                }))
            }
            "composition.plan" => {
                let spec = args.get("spec").context("composition spec")?;
                let nodes = spec
                    .get("nodes")
                    .and_then(Value::as_array)
                    .context("composition nodes")?;
                let mut creates = Vec::new();
                let mut modifies = Vec::new();
                for node in nodes {
                    let logical_id = node.get("id").and_then(Value::as_str).unwrap_or("");
                    let resolved = json!({
                        "component_id":null,
                        "text_style_id":null,
                        "fill_variable_id":null
                    });
                    if let Some(existing) = node.get("existing_node_id").and_then(Value::as_str) {
                        let target = self
                            .nodes
                            .get(existing)
                            .context("existing semantic node missing")?;
                        let expected =
                            match node.get("kind").and_then(Value::as_str).unwrap_or("frame") {
                                "text" => "TEXT",
                                "section" => "SECTION",
                                "component_instance" => "INSTANCE",
                                "shape" | "media" => "RECTANGLE",
                                _ => "FRAME",
                            };
                        if target.kind != expected {
                            bail!("existing semantic node kind mismatch");
                        }
                        modifies.push(json!({
                            "node_id":existing,
                            "logical_id":logical_id,
                            "resolved":resolved,
                            "action":{"kind":"apply_intent"}
                        }));
                    } else {
                        creates.push(json!({
                            "logical_id":logical_id,
                            "parent_logical_id":node.get("parent").cloned().unwrap_or(Value::Null),
                            "resolved":resolved
                        }));
                    }
                }
                Ok(json!({
                    "version":1,
                    "creates":creates,
                    "modifies":modifies,
                    "deletes":[],
                    "expected_effects":[
                        "native_figma_nodes",
                        "native_text_for_copy",
                        "auto_layout_when_declared",
                        "post_write_measurement"
                    ],
                    "postconditions":[
                        "document_identity_unchanged",
                        "revision_advanced_only_by_authorized_apply"
                    ],
                    "required_scopes":["driver:figma"],
                    "risk":"mutating_reversible"
                }))
            }
            "composition.apply" => {
                let spec = args
                    .get("plan")
                    .and_then(|plan| plan.get("spec"))
                    .context("semantic plan spec")?
                    .clone();
                let nodes = spec
                    .get("nodes")
                    .and_then(Value::as_array)
                    .context("composition nodes")?
                    .clone();
                let mut logical = BTreeMap::<String, String>::new();
                let mut created = Vec::<Value>::new();
                let mut modified = Vec::<Value>::new();
                for declared in nodes {
                    let logical_id = declared
                        .get("id")
                        .and_then(Value::as_str)
                        .context("logical id")?
                        .to_owned();
                    if let Some(existing) = declared.get("existing_node_id").and_then(Value::as_str)
                    {
                        let node = self
                            .nodes
                            .get_mut(existing)
                            .context("existing semantic node missing")?;
                        Self::semantic_apply_declared(node, &declared)?;
                        logical.insert(logical_id.clone(), existing.to_owned());
                        modified.push(json!({
                            "logicalId":logical_id,
                            "nodeId":existing,
                            "type":node.kind,
                            "name":node.name
                        }));
                        continue;
                    }
                    let parent_id = match declared.get("parent").and_then(Value::as_str) {
                        Some(parent) => logical
                            .get(parent)
                            .cloned()
                            .context("semantic parent must be created first")?,
                        None => spec
                            .get("target")
                            .and_then(|target| target.get("parent_node_id"))
                            .and_then(Value::as_str)
                            .unwrap_or(&self.page)
                            .to_owned(),
                    };
                    let id = format!("1:{}", self.nodes.len() + 1);
                    let declared_kind = declared
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("frame");
                    let kind = match declared_kind {
                        "text" => "TEXT",
                        "section" => "SECTION",
                        "component_instance" => "INSTANCE",
                        "shape" | "media" => "RECTANGLE",
                        _ => "FRAME",
                    };
                    let sizing = declared.get("sizing");
                    let width = sizing
                        .and_then(|value| value.get("width"))
                        .filter(|axis| axis.get("mode").and_then(Value::as_str) == Some("fixed"))
                        .and_then(|axis| axis.get("value"))
                        .and_then(Value::as_f64)
                        .unwrap_or(100.0);
                    let height = sizing
                        .and_then(|value| value.get("height"))
                        .filter(|axis| axis.get("mode").and_then(Value::as_str) == Some("fixed"))
                        .and_then(|axis| axis.get("value"))
                        .and_then(Value::as_f64)
                        .unwrap_or(100.0);
                    let mut props = BTreeMap::new();
                    props.insert("logicalId".into(), json!(logical_id));
                    if let Some(text) = declared.get("text") {
                        props.insert(
                            "characters".into(),
                            text.get("characters").cloned().unwrap_or(json!("")),
                        );
                    }
                    let layout_mode = match declared_kind {
                        "stack" => "VERTICAL",
                        "row" | "split" => "HORIZONTAL",
                        "grid" => "GRID",
                        "overlay" => "NONE",
                        _ => declared
                            .get("layout")
                            .and_then(|layout| layout.get("direction"))
                            .and_then(Value::as_str)
                            .map(|value| match value {
                                "vertical" => "VERTICAL",
                                "horizontal" => "HORIZONTAL",
                                "grid" => "GRID",
                                _ => "NONE",
                            })
                            .unwrap_or("NONE"),
                    };
                    props.insert("layoutMode".into(), json!(layout_mode));
                    props.insert(
                        "itemSpacing".into(),
                        declared
                            .get("layout")
                            .and_then(|layout| layout.get("gap"))
                            .cloned()
                            .unwrap_or(json!(0)),
                    );
                    let node = Node {
                        id: id.clone(),
                        kind: kind.into(),
                        name: declared
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or(&logical_id)
                            .into(),
                        x: 0.0,
                        y: 0.0,
                        w: width,
                        h: height,
                        children: vec![],
                        props,
                    };
                    self.nodes.insert(id.clone(), node);
                    self.nodes
                        .get_mut(&parent_id)
                        .context("semantic parent missing")?
                        .children
                        .push(id.clone());
                    logical.insert(logical_id.clone(), id.clone());
                    created.push(json!({
                        "logicalId":logical_id,
                        "nodeId":id,
                        "type":kind,
                        "name":declared.get("name").cloned().unwrap_or(json!(""))
                    }));
                }
                self.revision += 1;
                let root_ids = spec
                    .get("nodes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|node| node.get("parent").is_none_or(Value::is_null))
                    .filter_map(|node| node.get("id").and_then(Value::as_str))
                    .filter_map(|id| logical.get(id).cloned())
                    .collect::<Vec<_>>();
                Ok(json!({
                    "applied":true,
                    "rootNodeIds":root_ids,
                    "created":created,
                    "modified":modified,
                    "logicalToNode":logical,
                    "observedRevision":self.revision,
                    "effects":[
                        "native_figma_nodes",
                        "native_text_for_copy",
                        "auto_layout_when_declared",
                        "post_write_measurement"
                    ]
                }))
            }
            "composition.measure" => {
                let root = args
                    .get("root_node_id")
                    .and_then(Value::as_str)
                    .context("root_node_id")?;
                self.semantic_measure(root)
            }
            "composition.validate" => {
                let root = args
                    .get("root_node_id")
                    .and_then(Value::as_str)
                    .context("root_node_id")?;
                self.semantic_validate(root, args.get("spec"))
            }
            "composition.repair.plan" => {
                let mut modifies = Vec::new();
                for finding in args
                    .get("findings")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let repairs = finding
                        .get("suggested_repairs")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    if repairs.len() != 1 {
                        continue;
                    }
                    let Some(node_id) = finding.get("subject_node_id").and_then(Value::as_str)
                    else {
                        continue;
                    };
                    let repair = &repairs[0];
                    if repair.get("kind").and_then(Value::as_str) == Some("set_auto_layout_gap") {
                        modifies.push(json!({
                            "node_id":node_id,
                            "logical_id":finding.get("subject_logical_id").cloned().unwrap_or(Value::Null),
                            "action":{
                                "kind":"set_auto_layout_gap",
                                "gap":repair.get("gap").cloned().unwrap_or(json!(0))
                            }
                        }));
                    }
                }
                Ok(json!({
                    "version":1,
                    "creates":[],
                    "modifies":modifies,
                    "deletes":[],
                    "expected_effects":["bounded_deterministic_repair"],
                    "postconditions":["fresh_measurement_required","fresh_validation_required"],
                    "required_scopes":["driver:figma"],
                    "risk":"mutating_reversible"
                }))
            }
            "composition.repair.apply" => {
                let changes = args
                    .get("plan")
                    .and_then(|plan| plan.get("changeset"))
                    .and_then(|set| set.get("modifies"))
                    .and_then(Value::as_array)
                    .context("repair changes")?
                    .clone();
                let mut modified = Vec::new();
                for change in changes {
                    let node_id = change
                        .get("node_id")
                        .and_then(Value::as_str)
                        .context("repair node")?;
                    let action = change.get("action").context("repair action")?;
                    if action.get("kind").and_then(Value::as_str) != Some("set_auto_layout_gap") {
                        bail!("unsupported fake repair");
                    }
                    let gap = action
                        .get("gap")
                        .and_then(Value::as_f64)
                        .context("repair gap")?;
                    self.nodes
                        .get_mut(node_id)
                        .context("repair target missing")?
                        .props
                        .insert("itemSpacing".into(), json!(gap));
                    modified.push(json!({"nodeId":node_id,"action":"set_auto_layout_gap"}));
                }
                self.revision += 1;
                Ok(json!({
                    "applied":true,
                    "modified":modified,
                    "observedRevision":self.revision
                }))
            }
            "composition.verify" => {
                let root = args
                    .get("root_node_id")
                    .and_then(Value::as_str)
                    .context("root_node_id")?;
                let measurement = self.semantic_measure(root)?;
                let validation = self.semantic_validate(root, args.get("spec"))?;
                let bytes = vec![137, 80, 78, 71, 13, 10, 26, 10, 1, 2, 3];
                let token = Uuid::new_v4().simple().to_string();
                let length = bytes.len();
                self.artifacts.insert(token.clone(), bytes);
                Ok(json!({
                    "token":token,
                    "bytes":length,
                    "mediaType":"image/png",
                    "name":args.get("name").and_then(Value::as_str).unwrap_or("semantic-authoring.png"),
                    "nodeId":root,
                    "scale":args.get("scale").and_then(Value::as_f64).unwrap_or(1.0),
                    "measurement":measurement,
                    "validation":validation,
                    "observedRevision":self.revision
                }))
            }
            "page.list" => Ok(json!([summary(
                self.nodes.get(&self.page).expect("page fixture")
            )])),
            "variable.collection.list" => {
                Ok(Value::Array(self.collections.values().cloned().collect()))
            }
            "variable.list" => Ok(Value::Array(self.variables.values().cloned().collect())),
            "variable.collection.create" => {
                let id = format!("vc:{}", self.collections.len() + 1);
                let value = json!({
                    "id": id,
                    "name": args["name"].as_str().unwrap_or("Collection"),
                    "modes": [{"modeId":"m:1","name":"Mode 1"}],
                    "defaultModeId": "m:1",
                    "variableIds": []
                });
                self.collections.insert(id, value.clone());
                self.revision += 1;
                Ok(value)
            }
            "variable.create" => {
                let collection_id = args["collectionId"].as_str().context("collectionId")?;
                let collection = self
                    .collections
                    .get_mut(collection_id)
                    .context("collection not found")?;
                let id = format!("v:{}", self.variables.len() + 1);
                let value = json!({
                    "id": id,
                    "name": args["name"].as_str().unwrap_or("Variable"),
                    "resolvedType": args["resolvedType"].as_str().unwrap_or("STRING"),
                    "valuesByMode": {}
                });
                collection["variableIds"]
                    .as_array_mut()
                    .context("variableIds")?
                    .push(json!(id));
                self.variables.insert(id, value.clone());
                self.revision += 1;
                Ok(value)
            }
            "variable.set_value" => {
                let id = args["variableId"].as_str().context("variableId")?;
                let mode = args["modeId"].as_str().context("modeId")?;
                let variable = self.variables.get_mut(id).context("variable not found")?;
                variable["valuesByMode"][mode] = args.get("value").cloned().unwrap_or(Value::Null);
                self.revision += 1;
                Ok(json!({"id":id,"modeId":mode,"updated":true}))
            }
            "design_system.extract" | "snapshot.design_system" => Ok(json!({
                "collections": self.collections.values().cloned().collect::<Vec<_>>(),
                "variables": self.variables.values().cloned().collect::<Vec<_>>(),
                "components": [],
                "styles": {}
            })),
            "node.get" => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                Ok(summary(self.nodes.get(id).context("not found")?))
            }
            "snapshot.subtree" => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                let node = self.nodes.get(id).context("not found")?;
                Ok(json!({
                    "id": node.id,
                    "type": node.kind,
                    "name": node.name,
                    "x": node.x,
                    "y": node.y,
                    "width": node.w,
                    "height": node.h,
                    "children": node.children
                }))
            }
            "node.children" => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                let node = self.nodes.get(id).context("not found")?;
                Ok(Value::Array(
                    node.children
                        .iter()
                        .filter_map(|child| self.nodes.get(child))
                        .map(summary)
                        .collect(),
                ))
            }
            "node.search" => {
                let name = args
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_lowercase();
                Ok(Value::Array(
                    self.nodes
                        .values()
                        .filter(|node| node.name.to_lowercase().contains(&name))
                        .map(summary)
                        .collect(),
                ))
            }
            "frame.create" | "rect.create" | "ellipse.create" | "text.create"
            | "component.create" => {
                let id = format!("1:{}", self.nodes.len() + 1);
                let kind = op.split('.').next().unwrap_or("node").to_uppercase();
                let mut props = BTreeMap::new();
                if kind == "TEXT" {
                    props.insert(
                        "characters".into(),
                        args.get("characters").cloned().unwrap_or(json!("")),
                    );
                }
                let node = Node {
                    id: id.clone(),
                    kind,
                    name: args["name"].as_str().unwrap_or("Untitled").into(),
                    x: args.get("x").and_then(Value::as_f64).unwrap_or(0.0),
                    y: args.get("y").and_then(Value::as_f64).unwrap_or(0.0),
                    w: args.get("width").and_then(Value::as_f64).unwrap_or(100.0),
                    h: args.get("height").and_then(Value::as_f64).unwrap_or(100.0),
                    children: vec![],
                    props,
                };
                self.nodes.insert(id.clone(), node.clone());
                self.nodes
                    .get_mut(&self.page)
                    .expect("page fixture")
                    .children
                    .push(id);
                self.revision += 1;
                Ok(summary(&node))
            }
            "text.patch" => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                let node = self.nodes.get_mut(id).context("not found")?;
                if node.kind != "TEXT" {
                    bail!("not text");
                }
                if let Some(name) = args.get("name").and_then(Value::as_str) {
                    node.name = name.to_owned();
                }
                if let Some(characters) = args.get("characters").and_then(Value::as_str) {
                    node.props.insert("characters".into(), json!(characters));
                }
                self.revision += 1;
                Ok(summary(node))
            }
            "node.reparent" => {
                let id = args["nodeId"].as_str().context("nodeId")?.to_owned();
                let parent_id = args["parentId"].as_str().context("parentId")?.to_owned();
                if !self.nodes.contains_key(&id) || !self.nodes.contains_key(&parent_id) {
                    bail!("not found");
                }
                let old_parent = self
                    .nodes
                    .iter()
                    .find(|(_, node)| node.children.iter().any(|child| child == &id))
                    .map(|(parent, _)| parent.clone());
                if let Some(old_parent) = old_parent {
                    self.nodes
                        .get_mut(&old_parent)
                        .expect("old parent exists")
                        .children
                        .retain(|child| child != &id);
                }
                self.nodes
                    .get_mut(&parent_id)
                    .expect("new parent exists")
                    .children
                    .push(id.clone());
                self.revision += 1;
                Ok(json!({"nodeId":id,"parentId":parent_id}))
            }
            "svg.import" => {
                let svg = args["svg"].as_str().context("svg")?;
                if svg.len() > 1_048_576 {
                    bail!("svg too large");
                }
                let id = format!("1:{}", self.nodes.len() + 1);
                let mut props = BTreeMap::new();
                props.insert("sourceKind".into(), json!("svg_import"));
                props.insert("sourceBytes".into(), json!(svg.len()));
                let node = Node {
                    id: id.clone(),
                    kind: "FRAME".into(),
                    name: "Code-first SVG import".into(),
                    x: 0.0,
                    y: 0.0,
                    w: 1440.0,
                    h: 4100.0,
                    children: vec![],
                    props,
                };
                self.nodes.insert(id.clone(), node.clone());
                self.nodes
                    .get_mut(&self.page)
                    .expect("page fixture")
                    .children
                    .push(id);
                self.revision += 1;
                Ok(summary(&node))
            }
            "node.move" => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                let node = self.nodes.get_mut(id).context("not found")?;
                node.x = args["x"].as_f64().context("x")?;
                node.y = args["y"].as_f64().context("y")?;
                self.revision += 1;
                Ok(summary(node))
            }
            "node.resize" => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                let node = self.nodes.get_mut(id).context("not found")?;
                node.w = args["width"].as_f64().context("width")?;
                node.h = args["height"].as_f64().context("height")?;
                self.revision += 1;
                Ok(summary(node))
            }
            "layout.patch" => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                let node = self.nodes.get_mut(id).context("not found")?;
                for key in [
                    "layoutMode",
                    "itemSpacing",
                    "paddingTop",
                    "paddingRight",
                    "paddingBottom",
                    "paddingLeft",
                ] {
                    if let Some(value) = args.get(key) {
                        node.props.insert(key.into(), value.clone());
                    }
                }
                self.revision += 1;
                Ok(json!({
                    "layoutMode": node.props.get("layoutMode").cloned().unwrap_or(json!("NONE")),
                    "itemSpacing": node.props.get("itemSpacing").cloned().unwrap_or(json!(0))
                }))
            }
            "layout.inspect" => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                let node = self.nodes.get(id).context("not found")?;
                Ok(json!({
                    "layoutMode": node.props.get("layoutMode").cloned().unwrap_or(json!("NONE")),
                    "itemSpacing": node.props.get("itemSpacing").cloned().unwrap_or(json!(0))
                }))
            }
            "prototype.reaction.set" => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                if !self.nodes.contains_key(id) {
                    bail!("not found");
                }
                let reactions = args["reactions"].as_array().context("reactions")?.clone();
                self.reactions.insert(id.to_string(), reactions);
                self.revision += 1;
                Ok(json!({"count": self.reactions.get(id).map_or(0, Vec::len)}))
            }
            "prototype.reaction.list" => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                Ok(Value::Array(
                    self.reactions.get(id).cloned().unwrap_or_default(),
                ))
            }
            "motion.styles.list" if self.motion => {
                Ok(json!([{"styleId":"fake-spring","name":"Spring"}]))
            }
            "motion.styles.list" => bail!("motion unavailable"),
            "motion.style.apply" if self.motion => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                let node = self.nodes.get_mut(id).context("not found")?;
                let styles = node
                    .props
                    .entry("animationStyles".into())
                    .or_insert_with(|| json!([]));
                styles
                    .as_array_mut()
                    .context("animationStyles")?
                    .push(json!({
                        "styleId": args["styleId"],
                        "duration": args["duration"],
                        "timelineOffset": args.get("timelineOffset").cloned().unwrap_or(json!(0))
                    }));
                self.revision += 1;
                Ok(json!({"applied":true}))
            }
            "motion.keyframe.apply" if self.motion => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                let node = self.nodes.get_mut(id).context("not found")?;
                let tracks = node
                    .props
                    .entry("manualKeyframeTracks".into())
                    .or_insert_with(|| json!([]));
                tracks
                    .as_array_mut()
                    .context("manualKeyframeTracks")?
                    .push(json!({
                        "field": args["field"],
                        "baseValue": args["track"].get("baseValue").cloned().unwrap_or(Value::Null),
                        "keyframes": args["track"]["keyframes"]
                    }));
                let end = args["track"]["keyframes"]
                    .as_array()
                    .and_then(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.get("timelinePosition").and_then(Value::as_f64))
                            .reduce(f64::max)
                    })
                    .unwrap_or(0.0);
                self.revision += 1;
                Ok(json!({"applied":true,"field":args["field"],"end":end}))
            }
            "motion.timeline.set_duration" if self.motion => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                let node = self.nodes.get_mut(id).context("not found")?;
                node.props.insert(
                    "timelines".into(),
                    json!([{
                        "id": args["timelineId"],
                        "duration": args["duration"]
                    }]),
                );
                self.revision += 1;
                Ok(json!({"duration": args["duration"]}))
            }
            "motion.node.inspect" if self.motion => {
                let id = args["nodeId"].as_str().context("nodeId")?;
                let node = self.nodes.get(id).context("not found")?;
                Ok(json!({
                    "animationStyles": node.props.get("animationStyles").cloned().unwrap_or(json!([])),
                    "manualKeyframeTracks": node.props.get("manualKeyframeTracks").cloned().unwrap_or(json!([])),
                    "animations": [],
                    "timelines": node.props.get("timelines").cloned().unwrap_or(json!([]))
                }))
            }
            "figjam.sticky.create" | "figjam.shape.create" | "figjam.section.create" => {
                let id = format!("1:{}", self.nodes.len() + 1);
                let kind = match op {
                    "figjam.sticky.create" => "STICKY",
                    "figjam.shape.create" => "SHAPE_WITH_TEXT",
                    _ => "SECTION",
                };
                let node = Node {
                    id: id.clone(),
                    kind: kind.into(),
                    name: args["name"].as_str().unwrap_or(kind).into(),
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 100.0,
                    children: vec![],
                    props: BTreeMap::new(),
                };
                self.nodes.insert(id.clone(), node.clone());
                self.revision += 1;
                Ok(summary(&node))
            }
            "figjam.connector.create" => {
                let from = args["from"].as_str().context("from")?;
                let to = args["to"].as_str().context("to")?;
                if !self.nodes.contains_key(from) || !self.nodes.contains_key(to) {
                    bail!("connector endpoint missing");
                }
                let id = format!("1:{}", self.nodes.len() + 1);
                let mut props = BTreeMap::new();
                props.insert("from".into(), json!(from));
                props.insert("to".into(), json!(to));
                let node = Node {
                    id: id.clone(),
                    kind: "CONNECTOR".into(),
                    name: "Connector".into(),
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                    children: vec![],
                    props,
                };
                self.nodes.insert(id.clone(), node.clone());
                self.revision += 1;
                Ok(summary(&node))
            }
            "a11y.vision.analyze" => {
                let root = args
                    .get("rootNodeId")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.page);
                if !self.nodes.contains_key(root) {
                    bail!("not found");
                }
                let modes = args
                    .get("modes")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_else(|| {
                        vec![
                            json!("protanopia"),
                            json!("deuteranopia"),
                            json!("tritanopia"),
                        ]
                    });
                let results = modes
                    .into_iter()
                    .map(|mode| json!({"mode":mode,"pairs":[],"truncated":false}))
                    .collect::<Vec<_>>();
                Ok(json!({
                    "model":"machado-2009-full-severity","metric":"euclidean-srgb","rootNodeId":root,
                    "colorCount":0,"threshold":args.get("threshold").and_then(Value::as_f64).unwrap_or(0.10),
                    "minOriginalDistance":args.get("minOriginalDistance").and_then(Value::as_f64).unwrap_or(0.15),
                    "results":results
                }))
            }
            "a11y.vision.preview" => {
                let source_id = args["nodeId"].as_str().context("nodeId")?;
                let source = self.nodes.get(source_id).context("not found")?.clone();
                let modes = args
                    .get("modes")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_else(|| {
                        vec![
                            json!("protanopia"),
                            json!("deuteranopia"),
                            json!("tritanopia"),
                        ]
                    });
                let gap = args.get("gap").and_then(Value::as_f64).unwrap_or(80.0);
                let mut previews = Vec::new();
                for (index, mode) in modes.into_iter().enumerate() {
                    let id = format!("1:{}", self.nodes.len() + 1);
                    let mut clone = source.clone();
                    clone.id = id.clone();
                    clone.name = format!(
                        "[Semwright vision:{}] {}",
                        mode.as_str().unwrap_or("vision"),
                        source.name
                    );
                    clone.x = source.x + (source.w + gap) * (index as f64 + 1.0);
                    self.nodes.insert(id.clone(), clone.clone());
                    self.nodes
                        .get_mut(&self.page)
                        .expect("page fixture")
                        .children
                        .push(id);
                    previews
                        .push(json!({"mode":mode,"node":summary(&clone),"transformedPaints":0}));
                }
                self.revision += 1;
                Ok(json!({"sourceNodeId":source_id,"previews":previews}))
            }
            "verify.node" => {
                let node_id = args["nodeId"].as_str().context("nodeId")?;
                let node = self.nodes.get(node_id).context("not found")?;
                let bytes = b"\x89PNG\r\n\x1a\nFAKE-VERIFY".to_vec();
                let token = format!("fake-artifact-{}", self.artifacts.len() + 1);
                let name = args
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("figma-verify.png")
                    .to_owned();
                let length = bytes.len();
                let structure = summary(node);
                self.artifacts.insert(token.clone(), bytes);
                Ok(json!({
                    "token":token,"bytes":length,"mediaType":"image/png","name":name,
                    "nodeId":node_id,"scale":args.get("scale").and_then(Value::as_f64).unwrap_or(1.0),
                    "nodeCount":1,"structure":structure
                }))
            }
            "export.node" | "motion.export" => {
                let node_id = args["nodeId"].as_str().context("nodeId")?;
                if !self.nodes.contains_key(node_id) {
                    bail!("not found");
                }
                let format = args
                    .get("format")
                    .and_then(Value::as_str)
                    .unwrap_or(if op == "motion.export" { "MP4" } else { "PNG" })
                    .to_ascii_uppercase();
                let (media_type, extension, bytes): (&str, &str, Vec<u8>) = match format.as_str() {
                    "MP4" => ("video/mp4", "mp4", b"FAKE-MP4-SEMWRIGHT".to_vec()),
                    "GIF" => ("image/gif", "gif", b"GIF89aFAKE-SEMWRIGHT".to_vec()),
                    "WEBM" => ("video/webm", "webm", b"FAKE-WEBM-SEMWRIGHT".to_vec()),
                    "SVG" => (
                        "image/svg+xml",
                        "svg",
                        b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".to_vec(),
                    ),
                    "PDF" => ("application/pdf", "pdf", b"%PDF-1.4\n%fake\n".to_vec()),
                    "JPG" => ("image/jpeg", "jpg", b"FAKE-JPEG-SEMWRIGHT".to_vec()),
                    _ => (
                        "image/png",
                        "png",
                        b"\x89PNG\r\n\x1a\nFAKE-SEMWRIGHT".to_vec(),
                    ),
                };
                let token = format!("fake-artifact-{}", self.artifacts.len() + 1);
                let name = args
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("figma-export.{extension}"));
                let length = bytes.len();
                self.artifacts.insert(token.clone(), bytes);
                Ok(json!({
                    "token":token,
                    "bytes":length,
                    "mediaType":media_type,
                    "name":name
                }))
            }
            "artifact.read" => {
                let token = args["token"].as_str().context("token")?;
                let bytes = self.artifacts.get(token).context("artifact not found")?;
                let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
                let requested = args
                    .get("length")
                    .and_then(Value::as_u64)
                    .unwrap_or(196_608)
                    .min(196_608) as usize;
                if offset > bytes.len() {
                    bail!("artifact offset out of bounds");
                }
                let end = offset.saturating_add(requested).min(bytes.len());
                Ok(json!({
                    "token":token,
                    "offset":offset,
                    "nextOffset":end,
                    "totalBytes":bytes.len(),
                    "eof":end == bytes.len(),
                    "base64":BASE64.encode(&bytes[offset..end])
                }))
            }
            "artifact.release" => {
                let token = args["token"].as_str().context("token")?;
                Ok(json!({"released":self.artifacts.remove(token).is_some()}))
            }
            _ => bail!("unsupported operation"),
        }
    }
}

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn send(ws: &mut Ws, value: Value) -> Result<()> {
    ws.send(Message::Text(value.to_string().into())).await?;
    Ok(())
}

fn proof(secret_hex: &str, nonce: &str, session: &str, generation: u64) -> Result<String> {
    let key = hex::decode(secret_hex).context("pairing secret must be hex")?;
    if key.len() != 32 {
        bail!("pairing secret must be 32 bytes");
    }
    let mut mac = HmacSha256::new_from_slice(&key).expect("HMAC accepts any key length");
    mac.update(nonce.as_bytes());
    mac.update(b"\0");
    mac.update(session.as_bytes());
    mac.update(b"\0");
    mac.update(&generation.to_be_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ws://127.0.0.1:38471".into());
    let secret =
        std::env::var("SEMWRIGHT_FIGMA_PAIRING_SECRET").context("pairing secret env required")?;
    let (mut ws, _) = connect_async(&url).await?;

    let session = Uuid::new_v4().to_string();
    let generation = 1u64;

    send(
        &mut ws,
        json!({
            "type":"hello",
            "protocol":2,
            "plugin_build":"fake-0.2.0",
            "figma_api":"1.138.0",
            "editor_type":"figma",
            "session_id":session,
            "document_id":"fake-doc",
            "generation":generation,
            "capabilities":["design","prototype","motion_beta","figjam"]
        }),
    )
    .await?;

    let challenge_frame = ws.next().await.context("server challenge missing")??;
    if !challenge_frame.is_text() {
        bail!("server challenge must be text");
    }
    let challenge: Value = serde_json::from_str(challenge_frame.to_text()?)?;
    if challenge["type"] != "challenge"
        || challenge["session_id"] != session
        || challenge["generation"] != generation
    {
        bail!("invalid server challenge");
    }
    let nonce = challenge["nonce"].as_str().context("challenge nonce")?;
    let auth_proof = proof(&secret, nonce, &session, generation)?;
    send(
        &mut ws,
        json!({
            "type":"authenticate",
            "session_id":session,
            "generation":generation,
            "proof":auth_proof
        }),
    )
    .await?;

    let mut fake = Fake::new();
    while let Some(frame) = ws.next().await {
        let frame = frame?;
        if !frame.is_text() {
            continue;
        }
        let value: Value = serde_json::from_str(frame.to_text()?)?;
        match value["type"].as_str().unwrap_or("") {
            "ready" => eprintln!("fake Figma paired"),
            "ping" => send(&mut ws, json!({"type":"pong","nonce":value["nonce"]})).await?,
            "request" => {
                let id = value["id"].as_str().context("id")?;
                let op = value["operation"].as_str().context("operation")?;
                if value
                    .get("expected_revision")
                    .and_then(Value::as_u64)
                    .is_some_and(|revision| revision != fake.revision)
                {
                    send(
                        &mut ws,
                        json!({
                            "type":"response",
                            "id":id,
                            "session_id":session,
                            "generation":generation,
                            "revision":fake.revision,
                            "ok":false,
                            "error":{
                                "code":"conflict",
                                "message":"revision changed",
                                "outcome_known":true
                            }
                        }),
                    )
                    .await?;
                } else {
                    match fake.execute(op, &value["args"]) {
                        Ok(result) => {
                            send(
                                &mut ws,
                                json!({
                                    "type":"response",
                                    "id":id,
                                    "session_id":session,
                                    "generation":generation,
                                    "revision":fake.revision,
                                    "ok":true,
                                    "value":result
                                }),
                            )
                            .await?
                        }
                        Err(error) => {
                            send(
                                &mut ws,
                                json!({
                                    "type":"response",
                                    "id":id,
                                    "session_id":session,
                                    "generation":generation,
                                    "revision":fake.revision,
                                    "ok":false,
                                    "error":{
                                        "code":"fake_error",
                                        "message":error.to_string(),
                                        "outcome_known":true
                                    }
                                }),
                            )
                            .await?
                        }
                    }
                }
            }
            "close" => break,
            _ => {}
        }
    }
    Ok(())
}
