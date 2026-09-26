pub mod bridge;
pub mod catalog;
pub mod config;
pub mod model;
pub mod runner;

use async_trait::async_trait;
use bridge::Bridge;
use catalog::{Catalog, Route};
pub use config::Config;
use runner::Runner;
use semwright_driver_sdk::{
    Capability, Driver, DriverChildEvent, DriverExecutionContext, DriverInterfaces,
};
use semwright_types::{Error, ErrorCode, NativeTarget, Result, target_marker};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;

pub struct GodotDriver {
    catalog: Catalog,
    pub bridge: Bridge,
    runner: Option<Runner>,
    events: Option<mpsc::UnboundedReceiver<DriverChildEvent>>,
}

impl GodotDriver {
    pub async fn new(config: Config) -> Result<Self> {
        config.validate()?;
        let runner = config
            .runner
            .clone()
            .map(|runner| Runner::new(runner, &config.projects))
            .transpose()?;
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let bridge = Bridge::start(config.port, config.projects, event_tx).await?;
        Ok(Self {
            catalog: Catalog::load()?,
            bridge,
            runner,
            events: Some(event_rx),
        })
    }
}

impl Drop for GodotDriver {
    fn drop(&mut self) {
        self.bridge.shutdown();
    }
}

#[async_trait]
impl Driver for GodotDriver {
    fn id(&self) -> &str {
        "godot"
    }
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            cooperative_cancellation: true,
            events: true,
            progress: true,
            artifacts: true,
            health: true,
            native_refs: true,
            ..DriverInterfaces::default()
        }
    }

    fn take_events(&mut self) -> Option<mpsc::UnboundedReceiver<DriverChildEvent>> {
        self.events.take()
    }

    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        Ok(self.catalog.capabilities_for(self.runner.is_some()))
    }

    async fn health(&mut self) -> Result<Value> {
        Ok(json!({
            "healthy": true,
            "bridge_protocol": 1,
            "connected_sessions": self.bridge.list().await.len()
        }))
    }

    async fn validate_native_ref(&mut self, target: &NativeTarget) -> Result<()> {
        let reference = native_target_to_godot_ref(target)?;
        let session = reference
            .get("session")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("Godot native reference session is missing"))?;
        self.bridge
            .call(
                session,
                "ref.resolve",
                json!({
                    "session": session,
                    "ref": reference,
                    "require_current": true
                }),
                Duration::from_secs(5),
            )
            .await?;
        Ok(())
    }

    async fn execute(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
    ) -> Result<Value> {
        self.execute_command(command, descriptor_sha256, args, None)
            .await
    }

    async fn execute_with_context(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
        context: DriverExecutionContext,
    ) -> Result<Value> {
        context.check_cancelled()?;
        self.execute_command(command, descriptor_sha256, args, Some(&context))
            .await
    }
}

fn godot_ref_native_target(reference: &Value) -> Result<NativeTarget> {
    let object = reference
        .as_object()
        .ok_or_else(|| Error::invalid("Godot reference must be an object"))?;
    if object.get("provider").and_then(Value::as_str) != Some("godot") {
        return Err(Error::invalid("Godot reference provider mismatch"));
    }
    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("Godot reference kind is missing"))?;
    if !matches!(kind, "node" | "resource" | "scene") {
        return Err(Error::invalid("Godot reference kind is unsupported"));
    }
    let revision = object
        .get("revision")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::invalid("Godot reference revision is missing"))?;
    let fingerprint = object
        .get("fingerprint")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("Godot reference fingerprint is missing"))?;
    let identity = serde_json::to_string(reference)?;
    if identity.len() > 8192 || fingerprint.len() != 64 {
        return Err(Error::invalid("Godot native reference exceeds bounds"));
    }
    Ok(NativeTarget {
        kind: "native".into(),
        identity,
        revision,
        fingerprint: fingerprint.into(),
        app: "org.godotengine.Godot".into(),
    })
}

fn native_target_to_godot_ref(target: &NativeTarget) -> Result<Value> {
    if target.kind != "native" || target.app != "org.godotengine.Godot" {
        return Err(Error::new(
            ErrorCode::StaleReference,
            "Native reference does not belong to Godot",
        ));
    }
    let reference: Value = serde_json::from_str(&target.identity).map_err(|_| {
        Error::new(
            ErrorCode::StaleReference,
            "Godot native reference is malformed",
        )
    })?;
    let rebuilt = godot_ref_native_target(&reference)?;
    if rebuilt.revision != target.revision || rebuilt.fingerprint != target.fingerprint {
        return Err(Error::new(
            ErrorCode::StaleReference,
            "Godot native reference identity changed",
        ));
    }
    Ok(reference)
}

fn promote_godot_refs(value: &mut Value) -> Result<()> {
    match value {
        Value::Object(object)
            if object.get("provider").and_then(Value::as_str) == Some("godot")
                && object
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| matches!(kind, "node" | "resource" | "scene")) =>
        {
            let target = godot_ref_native_target(&Value::Object(object.clone()))?;
            *value = target_marker(target);
        }
        Value::Object(object) => {
            for child in object.values_mut() {
                promote_godot_refs(child)?;
            }
        }
        Value::Array(values) => {
            for child in values {
                promote_godot_refs(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

impl GodotDriver {
    async fn execute_command(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        mut args: Value,
        context: Option<&DriverExecutionContext>,
    ) -> Result<Value> {
        if command == "driver.godot.ref.resolve"
            && args.get("ref").and_then(Value::as_str).is_some()
        {
            let target = context
                .and_then(DriverExecutionContext::native_target)
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::StaleReference,
                        "Broker-native Godot reference target is missing",
                    )
                })?;
            args["ref"] = native_target_to_godot_ref(target)?;
        }
        let entry = self.catalog.get(command)?;
        if descriptor_sha256 != entry.digest {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Godot capability descriptor digest mismatch",
            ));
        }
        entry.validate_input(&args)?;
        if let Some(context) = context {
            context.check_cancelled()?;
        }

        let result = match entry.route {
            Route::Local => self.execute_local(command, &args).await,
            Route::Plugin => {
                let session = args
                    .get("session")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::invalid("Godot plugin capability requires session"))?;
                let call = self.bridge.call(
                    session,
                    command.strip_prefix("driver.godot.").unwrap_or(command),
                    args.clone(),
                    Duration::from_millis(entry.capability.descriptor.timeout_ms),
                );
                if let Some(context) = context {
                    let cancellation = context.cancellation();
                    tokio::select! {
                        result = call => result,
                        _ = cancellation.cancelled() => {
                            let error = Error::new(
                                ErrorCode::Cancelled,
                                "Godot plugin request cancelled while in flight",
                            );
                            if entry.mutation {
                                Err(error.uncertain())
                            } else {
                                Err(error)
                            }
                        }
                    }
                } else {
                    call.await
                }
            }
            Route::Runner => {
                let runner = self.runner.as_ref().ok_or_else(|| {
                    Error::new(ErrorCode::Unavailable, "Godot runner is not configured")
                })?;
                if let Some(context) = context {
                    runner.execute_with_context(command, &args, context).await
                } else {
                    runner.execute(command, &args).await
                }
            }
        };

        let value = match result {
            Err(error)
                if entry.mutation
                    && matches!(
                        error.code,
                        ErrorCode::Timeout
                            | ErrorCode::Unavailable
                            | ErrorCode::BackendFailed
                            | ErrorCode::ProtocolMismatch
                    ) =>
            {
                return Err(error.uncertain());
            }
            other => other?,
        };
        entry.validate_output(&value)?;
        let mut promoted = value;
        promote_godot_refs(&mut promoted)?;
        Ok(promoted)
    }

    async fn execute_local(&self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "driver.godot.doctor" => Ok(json!({
                "driver": env!("CARGO_PKG_VERSION"),
                "bridge": 1,
                "connected_sessions": self.bridge.list().await.len(),
                "certification": "integration-under-test"
            })),
            "driver.godot.session.list" => Ok(json!({"sessions": self.bridge.list().await})),
            "driver.godot.snapshot.diff" => {
                Ok(model::semantic_diff(&args["before"], &args["after"]))
            }
            _ => Err(Error::new(
                ErrorCode::Internal,
                "Godot local route invariant violated",
            )),
        }
    }
}
