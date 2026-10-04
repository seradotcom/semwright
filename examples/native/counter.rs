//! An owned counter model for the generic domain contract.
//! This file is a new implementation in the 2026-10-03 source cut.
use semwright_native_sdk::{Error, ErrorCode, Model, Operation, Result, Value, json, tokio};

#[derive(Clone, Copy)]
pub struct Counter;
const LIMIT: i64 = 1_000_000;

fn value(input: &Value) -> Result<i64> {
    input
        .as_i64()
        .filter(|v| (-LIMIT..=LIMIT).contains(v))
        .ok_or_else(|| Error::invalid("Counter value must be an integer from -1000000 to 1000000"))
}

impl Model for Counter {
    fn id(&self) -> &'static str {
        "native-counter"
    }
    fn initial(&self) -> Value {
        json!({"counter":{"id":"counter","value":0},"independent":{"id":"independent","value":7}})
    }
    fn validate(&self, state: &Value) -> Result<()> {
        let object = state
            .as_object()
            .ok_or_else(|| Error::invalid("Counter state must be an object"))?;
        if object.len() != 2
            || !object.contains_key("counter")
            || !object.contains_key("independent")
        {
            return Err(Error::invalid("Counter state fields differ"));
        }
        for id in ["counter", "independent"] {
            let row = state[id]
                .as_object()
                .ok_or_else(|| Error::invalid("Counter row must be an object"))?;
            if row.len() != 2 || row.get("id") != Some(&json!(id)) || !row.contains_key("value") {
                return Err(Error::invalid("Counter row identity or fields differ"));
            }
            value(&state[id]["value"])?;
        }
        Ok(())
    }
    // These bounded records have no model dependency relations.
    fn dependencies(&self, _state: &Value) -> Result<Value> {
        Ok(json!({"relations":[],"coverage":"complete_for_model"}))
    }
    fn operations(&self) -> Vec<Operation> {
        vec![Operation {
            name: "set-value",
            description: "Set the counter value",
            input_schema: json!({
                "type":"object","properties":{"value":{"type":"integer","minimum":-1000000,"maximum":1000000}},
                "required":["value"],"additionalProperties":false
            }),
        }]
    }
    fn apply(&self, state: &Value, operation: &str, parameters: &Value) -> Result<Value> {
        if operation != "set-value" {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Counter operation is not allowed",
            ));
        }
        let fields = parameters
            .as_object()
            .ok_or_else(|| Error::invalid("Counter parameters must be an object"))?;
        if fields.len() != 1 || !fields.contains_key("value") {
            return Err(Error::invalid("Counter parameters differ"));
        }
        let next = value(&parameters["value"])?;
        self.validate(state)?;
        let mut candidate = state.clone();
        candidate["counter"]["value"] = json!(next);
        self.validate(&candidate)?;
        Ok(candidate)
    }
}

#[tokio::main(worker_threads = 2)]
async fn main() {
    if let Err(error) = semwright_native_sdk::run(Counter).await {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semwright_native_sdk::{Driver, NativeApp, descriptor_digest, sha256};

    fn args(view: &Value, key: &str, parameters: Value, workspace: Option<&str>) -> Value {
        let mut result = json!({"expected_revision":view["revision"],"expected_generation":view["generation"],"operation_key":key,"parameters":parameters});
        if let Some(workspace) = workspace {
            result["workspace_id"] = json!(workspace);
        }
        result
    }
    async fn call(app: &mut NativeApp<Counter>, operation: &str, args: Value) -> Result<Value> {
        let name = format!("driver.native-counter.{operation}");
        let descriptor = app
            .capabilities_value()
            .into_iter()
            .find(|cap| cap.descriptor.name == name)
            .unwrap()
            .descriptor;
        app.execute(&name, &descriptor_digest(&descriptor)?, args)
            .await
    }
    #[test]
    fn bounded_counter_keeps_the_independent_row() {
        let state = Counter.initial();
        assert_eq!(
            Counter.dependencies(&state).unwrap(),
            json!({"relations":[],"coverage":"complete_for_model"})
        );
        let changed = Counter
            .apply(&state, "set-value", &json!({"value":42}))
            .unwrap();
        assert_eq!(changed["counter"]["value"], 42);
        assert_eq!(changed["independent"], state["independent"]);
        for input in [
            json!({"value":1000001}),
            json!({"value":"42"}),
            json!({"value":42,"extra":true}),
        ] {
            assert_eq!(
                Counter.apply(&state, "set-value", &input).unwrap_err().code,
                ErrorCode::InvalidArgument
            );
        }
        assert_eq!(
            Counter
                .apply(&state, "execute", &json!({"value":42}))
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
    }
    #[tokio::test]
    async fn authored_model_copy_exports_bytes_and_preserves_source() {
        let root = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let mut app = NativeApp::create(root.path(), Counter)
            .unwrap()
            .with_output_root(output.path())
            .unwrap();
        let source = app.inspect().unwrap();
        let source_bytes = std::fs::read(root.path().join("document.json")).unwrap();
        let fork_args = args(&source, "fork", json!({"workspace_id":"owned_job"}), None);
        let fork = call(&mut app, "fork", fork_args.clone()).await.unwrap();
        let copy = call(&mut app, "inspect", json!({"workspace_id":"owned_job"}))
            .await
            .unwrap();
        assert_eq!(copy["resource_id"], fork["resource_id"]);
        assert_ne!(copy["resource_id"], source["resource_id"]);
        call(
            &mut app,
            "set-value",
            args(&copy, "edit", json!({"value":42}), Some("owned_job")),
        )
        .await
        .unwrap();
        let current = call(&mut app, "inspect", json!({"workspace_id":"owned_job"}))
            .await
            .unwrap();
        let exported = call(
            &mut app,
            "export",
            args(
                &current,
                "export",
                json!({"output_namespace":"owned_job","slot":"counter_json"}),
                Some("owned_job"),
            ),
        )
        .await
        .unwrap();
        let bytes = std::fs::read(
            output
                .path()
                .join(exported["artifact"]["path"].as_str().unwrap()),
        )
        .unwrap();
        assert_eq!(sha256(&bytes), exported["artifact"]["sha256"]);
        assert_eq!(
            semwright_native_sdk::serde_json::from_slice::<Value>(&bytes).unwrap()["counter"]["value"],
            42
        );
        assert_eq!(
            source_bytes,
            std::fs::read(root.path().join("document.json")).unwrap()
        );
        let reopened = NativeApp::open(root.path(), Counter).unwrap();
        assert_eq!(
            reopened.inspect().unwrap()["resource_id"],
            source["resource_id"]
        );
        assert_eq!(
            reopened
                .operation_get(
                    &json!({"operation":"fork","operation_key":"fork","request":fork_args})
                )
                .unwrap()["result"],
            fork
        );
    }
}
