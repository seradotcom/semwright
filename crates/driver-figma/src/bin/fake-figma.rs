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

    fn execute(&mut self, op: &str, args: &Value) -> Result<Value> {
        match op {
            "document.status" => Ok(json!({
                "documentId":"fake-doc",
                "editorType":"figma",
                "revision":self.revision,
                "currentPage":self.page
            })),
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
                let node = Node {
                    id: id.clone(),
                    kind,
                    name: args["name"].as_str().unwrap_or("Untitled").into(),
                    x: args.get("x").and_then(Value::as_f64).unwrap_or(0.0),
                    y: args.get("y").and_then(Value::as_f64).unwrap_or(0.0),
                    w: args.get("width").and_then(Value::as_f64).unwrap_or(100.0),
                    h: args.get("height").and_then(Value::as_f64).unwrap_or(100.0),
                    children: vec![],
                    props: BTreeMap::new(),
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
