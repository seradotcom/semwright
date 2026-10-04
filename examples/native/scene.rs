//! Own reference app: a persistent bounded parametric scene, not a 3D engine.
use semwright_native_sdk::{Error, ErrorCode, Model, Operation, Result, Value, json};
#[derive(Clone, Copy)]
pub struct Scene;
impl Model for Scene {
    fn id(&self) -> &'static str {
        "native-scene"
    }
    fn initial(&self) -> Value {
        json!({"objects":{"cube":{"position":[0.0,0.0,0.0],"scale":1.0,"color":"#8080ff"},"independent":{"position":[5.0,0.0,0.0],"scale":1.0,"color":"#ffffff"}}})
    }
    fn validate(&self, state: &Value) -> Result<()> {
        let objects = state
            .as_object()
            .filter(|o| o.len() == 1)
            .and_then(|o| o.get("objects"))
            .and_then(Value::as_object)
            .ok_or_else(|| Error::invalid("Invalid scene model"))?;
        if objects.is_empty() || objects.len() > 128 {
            return Err(Error::invalid("Scene object bound"));
        }
        for (id, v) in objects {
            if id.is_empty()
                || id.len() > 64
                || !id
                    .bytes()
                    .all(|x| x.is_ascii_alphanumeric() || b"_-".contains(&x))
            {
                return Err(Error::invalid("Invalid object id"));
            }
            let o = v
                .as_object()
                .filter(|o| o.len() == 3)
                .ok_or_else(|| Error::invalid("Invalid scene object"))?;
            let pos = o
                .get("position")
                .and_then(Value::as_array)
                .filter(|a| a.len() == 3)
                .ok_or_else(|| Error::invalid("Position requires three coordinates"))?;
            if pos.iter().any(|x| {
                x.as_f64()
                    .is_none_or(|f| !f.is_finite() || f.abs() > 10000.)
            }) || o
                .get("scale")
                .and_then(Value::as_f64)
                .is_none_or(|f| !f.is_finite() || !(0.01..=1000.).contains(&f))
            {
                return Err(Error::invalid("Scene numeric bounds exceeded"));
            }
            let color = o
                .get("color")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("Color required"))?;
            if color.len() != 7
                || !color.starts_with('#')
                || !color[1..].bytes().all(|c| c.is_ascii_hexdigit())
            {
                return Err(Error::invalid("Color must be #RRGGBB"));
            }
        }
        Ok(())
    }
    // These bounded records have no model dependency relations.
    fn dependencies(&self, _state: &Value) -> Result<Value> {
        Ok(json!({"relations":[],"coverage":"complete_for_model"}))
    }
    fn operations(&self) -> Vec<Operation> {
        vec![Operation {
            name: "set-object",
            description: "Update one existing scene object with exact document CAS",
            input_schema: json!({"type":"object","properties":{"object_id":{"type":"string","maxLength":64},"position":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","minimum":-10000,"maximum":10000}},"scale":{"type":"number","minimum":0.01,"maximum":1000},"color":{"type":"string","pattern":"^#[a-fA-F0-9]{6}$"}},"required":["object_id"],"minProperties":2,"additionalProperties":false}),
        }]
    }
    fn apply(&self, state: &Value, op: &str, p: &Value) -> Result<Value> {
        if op != "set-object" {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Only bounded scene edits are permitted",
            ));
        }
        let map = p
            .as_object()
            .ok_or_else(|| Error::invalid("Object parameters required"))?;
        if map.len() < 2
            || map
                .keys()
                .any(|k| !["object_id", "position", "scale", "color"].contains(&k.as_str()))
        {
            return Err(Error::invalid("Unsupported scene edit"));
        }
        let id = p["object_id"]
            .as_str()
            .ok_or_else(|| Error::invalid("Object id required"))?;
        let mut result = state.clone();
        let target = result["objects"]
            .get_mut(id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Object does not exist"))?;
        for key in ["position", "scale", "color"] {
            if let Some(v) = p.get(key) {
                target[key] = v.clone();
            }
        }
        self.validate(&result)?;
        Ok(result)
    }
}
#[tokio::main(worker_threads = 2)]
async fn main() {
    if let Err(e) = semwright_native_sdk::run(Scene).await {
        eprintln!("{e}");
        std::process::exit(e.exit_code());
    }
}
use semwright_native_sdk::tokio;
