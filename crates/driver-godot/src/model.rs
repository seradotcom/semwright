use serde_json::{Value, json};

pub fn semantic_diff(before: &Value, after: &Value) -> Value {
    let mut changes = Vec::new();
    let mut truncated = false;
    walk(Some(before), Some(after), "", &mut changes, &mut truncated);
    json!({"changes": changes, "truncated": truncated})
}

fn walk(
    a: Option<&Value>,
    b: Option<&Value>,
    path: &str,
    out: &mut Vec<Value>,
    truncated: &mut bool,
) {
    if a == b {
        return;
    }
    if out.len() >= 512 {
        *truncated = true;
        return;
    }
    match (a, b) {
        (Some(Value::Object(am)), Some(Value::Object(bm))) => {
            let keys = am.keys().chain(bm.keys()).collect::<std::collections::BTreeSet<_>>();
            for key in keys {
                let p = format!("{}/{}", path, key.replace('~', "~0").replace('/', "~1"));
                walk(am.get(key), bm.get(key), &p, out, truncated);
                if *truncated { break; }
            }
        }
        (Some(Value::Array(aa)), Some(Value::Array(bb))) => {
            let max = aa.len().max(bb.len());
            for i in 0..max {
                walk(aa.get(i), bb.get(i), &format!("{path}/{i}"), out, truncated);
                if *truncated { break; }
            }
        }
        _ => out.push(json!({"path": if path.is_empty(){"/"}else{path}, "before": a.cloned().unwrap_or(Value::Null), "after": b.cloned().unwrap_or(Value::Null)})),
    }
}
