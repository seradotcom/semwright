use crate::{Fault, FaultKind, Result, bounds};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub enum Message {
    Hello(Value),
    Identified(u64),
    Event(Value),
    Response(Response),
    Batch { id: String, results: Vec<Value> },
}
#[derive(Debug, Clone)]
pub struct Response {
    pub id: String,
    pub request_type: String,
    pub result: Result<Value>,
}

fn short(value: &Value, key: &str, max: usize) -> Result<String> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_graphic()))
        .map(String::from)
        .ok_or_else(|| Fault::new(FaultKind::Protocol))
}

pub fn status(data: &Value) -> Result<Value> {
    let status = data
        .get("requestStatus")
        .filter(|s| s.is_object())
        .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
    let ok = status["result"]
        .as_bool()
        .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
    let code = status["code"]
        .as_u64()
        .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
    if ok {
        if code != 100 {
            return Err(Fault::new(FaultKind::Protocol));
        }
        let body = data
            .get("responseData")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !body.is_object() {
            return Err(Fault::new(FaultKind::Protocol));
        }
        Ok(body)
    } else {
        let kind = match code {
            205 => FaultKind::Application,
            204 | 206 => FaultKind::Unsupported,
            207 => FaultKind::NotReady,
            300..=499 => FaultKind::Configuration,
            500..=607 => FaultKind::Precondition,
            700..=703 => FaultKind::Application,
            _ => FaultKind::Protocol,
        };
        if code == 100 {
            return Err(Fault::new(FaultKind::Protocol));
        }
        Err(Fault::new(kind))
    }
}

pub fn parse(bytes: &[u8]) -> Result<Message> {
    let value = bounds::parse(bytes, bounds::MAX_FRAME)?;
    let op = value["op"]
        .as_u64()
        .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
    let data = value
        .get("d")
        .filter(|v| v.is_object())
        .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
    match op {
        0 => {
            let rpc = data["rpcVersion"]
                .as_u64()
                .filter(|v| *v >= 1)
                .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
            if rpc > 1024 {
                return Err(Fault::new(FaultKind::Protocol));
            }
            short(data, "obsWebSocketVersion", 80)?;
            Ok(Message::Hello(data.clone()))
        }
        2 => Ok(Message::Identified(
            data["negotiatedRpcVersion"]
                .as_u64()
                .ok_or_else(|| Fault::new(FaultKind::Protocol))?,
        )),
        5 => Ok(Message::Event(data.clone())),
        7 => Ok(Message::Response(Response {
            id: short(data, "requestId", 96)?,
            request_type: short(data, "requestType", 128)?,
            result: status(data),
        })),
        9 => {
            let id = short(data, "requestId", 96)?;
            let results = data["results"]
                .as_array()
                .filter(|a| a.len() <= 32)
                .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
            for result in results {
                short(result, "requestType", 128)?;
                if !result["requestStatus"].is_object() {
                    return Err(Fault::new(FaultKind::Protocol));
                }
            }
            Ok(Message::Batch {
                id,
                results: results.clone(),
            })
        }
        _ => Err(Fault::new(FaultKind::Protocol)),
    }
}

pub fn request(id: &str, kind: &str, data: Value) -> Result<Value> {
    if id.len() > 96 || kind.len() > 128 || !data.is_object() {
        return Err(Fault::new(FaultKind::Configuration));
    }
    bounds::check(&data, 65536)?;
    Ok(json!({"op":6,"d":{"requestType":kind,"requestId":id,"requestData":data}}))
}

pub fn batch(id: &str, requests: &[(String, Value)], halt: bool) -> Result<Value> {
    if requests.is_empty() || requests.len() > 32 {
        return Err(Fault::new(FaultKind::ResourceLimit));
    }
    let rows: Vec<_> = requests
        .iter()
        .enumerate()
        .map(|(index, (kind, data))| {
            json!({"requestType":kind,"requestId":format!("{id}:{index}"),"requestData":data})
        })
        .collect();
    let value =
        json!({"op":8,"d":{"requestId":id,"haltOnFailure":halt,"executionType":0,"requests":rows}});
    bounds::check(&value, bounds::MAX_FRAME)?;
    Ok(value)
}

pub fn close(code: u16) -> Fault {
    Fault::new(match code {
        4009 => FaultKind::Authentication,
        4010 | 4002..=4008 | 4012 => FaultKind::Protocol,
        4011 => FaultKind::Closed,
        _ => FaultKind::Transport,
    })
}
