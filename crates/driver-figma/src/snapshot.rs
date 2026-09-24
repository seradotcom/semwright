use crate::model::{Finding, NodeSummary};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotMode {
    Identity,
    Portable,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub schema_version: u32,
    pub editor: String,
    pub api_version: String,
    pub document: Value,
}
fn rounded(n: f64) -> f64 {
    (n * 10000.0).round() / 10000.0
}
pub fn canonicalize(value: &Value, mode: SnapshotMode) -> Value {
    match value {
        Value::Object(m) => {
            let mut out = BTreeMap::new();
            for (k, v) in m {
                if matches!(
                    k.as_str(),
                    "timestamp" | "lastModified" | "user" | "session_nonce"
                ) {
                    continue;
                }
                if mode == SnapshotMode::Portable
                    && matches!(k.as_str(), "id" | "node_id" | "document_id" | "session_id")
                {
                    continue;
                }
                out.insert(k.clone(), canonicalize(v, mode));
            }
            Value::Object(out.into_iter().collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(|v| canonicalize(v, mode)).collect()),
        Value::Number(n) => n
            .as_f64()
            .and_then(|f| Number::from_f64(rounded(f)))
            .map(Value::Number)
            .unwrap_or_else(|| value.clone()),
        _ => value.clone(),
    }
}
pub fn digest(value: &Value, mode: SnapshotMode) -> String {
    let bytes = serde_json::to_vec(&canonicalize(value, mode)).unwrap_or_default();
    hex::encode(Sha256::digest(bytes))
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffEntry {
    pub path: String,
    pub kind: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}
pub fn diff(a: &Value, b: &Value, limit: usize) -> Vec<DiffEntry> {
    fn walk(path: &str, a: &Value, b: &Value, out: &mut Vec<DiffEntry>, limit: usize) {
        if out.len() >= limit || a == b {
            return;
        }
        match (a, b) {
            (Value::Object(x), Value::Object(y)) => {
                let keys: std::collections::BTreeSet<_> = x.keys().chain(y.keys()).collect();
                for k in keys {
                    if out.len() >= limit {
                        return;
                    }
                    match (x.get(k), y.get(k)) {
                        (Some(av), Some(bv)) => walk(&format!("{path}/{k}"), av, bv, out, limit),
                        (Some(av), None) => out.push(DiffEntry {
                            path: format!("{path}/{k}"),
                            kind: "removed".into(),
                            before: Some(av.clone()),
                            after: None,
                        }),
                        (None, Some(bv)) => out.push(DiffEntry {
                            path: format!("{path}/{k}"),
                            kind: "added".into(),
                            before: None,
                            after: Some(bv.clone()),
                        }),
                        _ => {}
                    }
                }
            }
            (Value::Array(x), Value::Array(y)) => {
                for i in 0..x.len().max(y.len()) {
                    if out.len() >= limit {
                        return;
                    }
                    match (x.get(i), y.get(i)) {
                        (Some(av), Some(bv)) => walk(&format!("{path}/{i}"), av, bv, out, limit),
                        (Some(av), None) => out.push(DiffEntry {
                            path: format!("{path}/{i}"),
                            kind: "removed".into(),
                            before: Some(av.clone()),
                            after: None,
                        }),
                        (None, Some(bv)) => out.push(DiffEntry {
                            path: format!("{path}/{i}"),
                            kind: "added".into(),
                            before: None,
                            after: Some(bv.clone()),
                        }),
                        _ => {}
                    }
                }
            }
            _ => out.push(DiffEntry {
                path: path.into(),
                kind: "changed".into(),
                before: Some(a.clone()),
                after: Some(b.clone()),
            }),
        }
    }
    let mut out = vec![];
    walk("", a, b, &mut out, limit);
    out
}
pub fn contrast_ratio(a: [f64; 3], b: [f64; 3]) -> f64 {
    fn lum(c: [f64; 3]) -> f64 {
        fn l(x: f64) -> f64 {
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * l(c[0]) + 0.7152 * l(c[1]) + 0.0722 * l(c[2])
    }
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}
pub fn validate_touch_target(node: &NodeSummary, min: f64) -> Option<Finding> {
    let g = node.geometry.as_ref()?;
    if g.width < min || g.height < min {
        Some(Finding {
            rule: "a11y.touch_target".into(),
            severity: "warning".into(),
            message: format!("target is {:.1}x{:.1}, below {:.1}", g.width, g.height, min),
            node: Some(node.reference.clone()),
        })
    } else {
        None
    }
}
