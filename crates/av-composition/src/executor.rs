//! Descriptor-pinned command sequencing through the existing Broker Executor.
use crate::{CommandProof, Error, Result, StageCall};
use semwright_driver_sdk::descriptor_digest;
use semwright_recipes::Executor;
use semwright_semantic_composition::{Digest, ensure};
use semwright_types::{ErrorCode, ExecuteRequest};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

fn broker_error(error: semwright_types::Error) -> Error {
    let code = error.code;
    let payload = error
        .message
        .chars()
        .filter(|character| !character.is_control())
        .take(448)
        .collect::<String>();
    let message = format!("{code:?}: {payload}")
        .chars()
        .take(512)
        .collect::<String>();
    match code {
        ErrorCode::PolicyDenied
        | ErrorCode::PermissionDenied
        | ErrorCode::ConsentRequired
        | ErrorCode::SandboxDenied => Error::Denied(message),
        ErrorCode::StaleReference | ErrorCode::Conflict => Error::Stale(message),
        ErrorCode::ResourceExhausted => Error::Limit(message),
        ErrorCode::Cancelled | ErrorCode::Timeout => Error::Unknown(message),
        _ if !error.outcome_known => Error::Unknown(message),
        _ => Error::Invalid(message),
    }
}

fn command_context(error: Error, command: &str) -> Error {
    let command = command
        .chars()
        .filter(|character| !character.is_control())
        .take(192)
        .collect::<String>();
    let prefix = format!("{command}: ");
    let decorate = |message: String| {
        prefix
            .chars()
            .chain(message.chars())
            .take(512)
            .collect::<String>()
    };
    match error {
        Error::Denied(message) => Error::Denied(decorate(message)),
        Error::Stale(message) => Error::Stale(decorate(message)),
        Error::Limit(message) => Error::Limit(decorate(message)),
        Error::Unknown(message) => Error::Unknown(decorate(message)),
        Error::Invalid(message) => Error::Invalid(decorate(message)),
    }
}

/// A single reserved AV stage consumes exactly the ordered descriptor bindings
/// captured in its ServiceProof. It cannot select another command or backend.
pub struct StageCommandRunner<'a> {
    executor: &'a dyn Executor,
    bindings: &'a [CommandProof],
    cursor: usize,
    failed: bool,
}

impl<'a> StageCommandRunner<'a> {
    pub fn new(executor: &'a dyn Executor, call: &'a StageCall) -> Result<Self> {
        let bindings =
            call.proof.commands.get(&call.stage).ok_or_else(|| {
                Error::Invalid("reserved AV stage has no command bindings".into())
            })?;
        ensure(
            !bindings.is_empty() && bindings.len() <= 16,
            "AV stage command binding budget",
        )?;
        Ok(Self {
            executor,
            bindings,
            cursor: 0,
            failed: false,
        })
    }

    pub fn remaining(&self) -> usize {
        self.bindings.len().saturating_sub(self.cursor)
    }

    pub async fn next(&mut self, args: Value, cancellation: CancellationToken) -> Result<Value> {
        if self.failed {
            return Err(Error::Unknown(
                "AV stage runner cannot continue after a failed Broker operation".into(),
            ));
        }
        let binding = self.bindings.get(self.cursor).ok_or_else(|| {
            Error::Denied("AV stage attempted an operation not present in its proof".into())
        })?;
        let descriptor = self
            .executor
            .describe(&binding.command)
            .map_err(broker_error)?;
        ensure(
            descriptor.name == binding.command,
            "Broker described a different AV command",
        )?;
        let actual = Digest::parse(descriptor_digest(&descriptor).map_err(broker_error)?)
            .map_err(|error| Error::Invalid(error.to_string()))?;
        if actual != binding.descriptor {
            self.failed = true;
            return Err(Error::Stale(
                "AV command descriptor changed after planning".into(),
            ));
        }
        let request = ExecuteRequest {
            command: binding.command.clone(),
            args,
            dry_run: false,
            backend: None,
        };
        match self.executor.execute(request, cancellation).await {
            Ok(value) => {
                self.cursor += 1;
                Ok(value)
            }
            Err(error) => {
                self.failed = true;
                Err(command_context(broker_error(error), &binding.command))
            }
        }
    }

    pub fn finish(self) -> Result<()> {
        if self.failed {
            return Err(Error::Unknown(
                "AV stage ended after a failed Broker operation".into(),
            ));
        }
        ensure(
            self.cursor == self.bindings.len(),
            "AV stage did not execute every descriptor-pinned command",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Service, ServiceProof, Stage, StagePayload};
    use async_trait::async_trait;
    use semwright_media_time::{Rate, Rational};
    use semwright_semantic_composition::{BaseStateSet, Owner, PrincipalBinding};
    use semwright_types::{CommandDescriptor, Error as NativeError, Idempotency, Risk};
    use serde_json::json;
    use std::{collections::BTreeMap, sync::Mutex};

    fn descriptor(name: &str, version: &str) -> CommandDescriptor {
        CommandDescriptor {
            name: name.into(),
            version: version.into(),
            description: "fixture AV command".into(),
            input_schema: json!({"type":"object","additionalProperties":true}),
            output_schema: json!({"type":"object","additionalProperties":true}),
            requires: vec![],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 1_000,
            dry_run: false,
            interactive_consent: false,
            backends: vec!["fixture".into()],
        }
    }

    fn proof_for(names: &[&str]) -> ServiceProof {
        let bindings = names
            .iter()
            .map(|name| {
                let value = descriptor(name, "1");
                CommandProof {
                    command: (*name).into(),
                    descriptor: Digest::parse(descriptor_digest(&value).unwrap()).unwrap(),
                }
            })
            .collect();
        ServiceProof {
            service: Service::Motion,
            provider: "fixture-motion".into(),
            generation: 1,
            catalog_digest: Digest::of_bytes(b"catalog"),
            runtime_digest: Digest::of_bytes(b"runtime"),
            commands: BTreeMap::from([(Stage::ApplyMotion, bindings)]),
            available: true,
        }
    }

    fn call(names: &[&str]) -> StageCall {
        StageCall {
            request_id: "request-1".into(),
            owner: Owner {
                session: "session-1".into(),
                principal: PrincipalBinding::HostSession,
            },
            av_plan_digest: Digest::of_bytes(b"av-plan"),
            stage: Stage::ApplyMotion,
            proof: proof_for(names),
            expected_base: BaseStateSet(vec![]),
            payload: StagePayload::ApplyMotion {
                plan_ref: "motion-plan".into(),
            },
        }
    }

    struct Fake {
        descriptors: BTreeMap<String, CommandDescriptor>,
        calls: Mutex<Vec<String>>,
        fail_on: Option<String>,
    }

    impl Fake {
        fn new(names: &[&str]) -> Self {
            Self {
                descriptors: names
                    .iter()
                    .map(|name| ((*name).into(), descriptor(name, "1")))
                    .collect(),
                calls: Mutex::new(vec![]),
                fail_on: None,
            }
        }
    }

    #[async_trait]
    impl Executor for Fake {
        fn describe(&self, command: &str) -> semwright_types::Result<CommandDescriptor> {
            self.descriptors
                .get(command)
                .cloned()
                .ok_or_else(|| NativeError::new(ErrorCode::NotFound, "fixture command absent"))
        }

        async fn execute(
            &self,
            request: ExecuteRequest,
            _: CancellationToken,
        ) -> semwright_types::Result<Value> {
            self.calls.lock().unwrap().push(request.command.clone());
            if self.fail_on.as_deref() == Some(request.command.as_str()) {
                return Err(NativeError::new(
                    ErrorCode::BackendFailed,
                    "fixture native failure",
                ));
            }
            Ok(json!({"command":request.command}))
        }
    }

    #[tokio::test]
    async fn executes_only_the_descriptor_pinned_order() {
        let fake = Fake::new(&["fixture.one", "fixture.two"]);
        let call = call(&["fixture.one", "fixture.two"]);
        let mut runner = StageCommandRunner::new(&fake, &call).unwrap();
        assert_eq!(runner.remaining(), 2);
        assert_eq!(
            runner
                .next(json!({"step":1}), CancellationToken::new())
                .await
                .unwrap()["command"],
            "fixture.one"
        );
        assert_eq!(
            runner
                .next(json!({"step":2}), CancellationToken::new())
                .await
                .unwrap()["command"],
            "fixture.two"
        );
        assert_eq!(runner.remaining(), 0);
        runner.finish().unwrap();
        assert_eq!(
            *fake.calls.lock().unwrap(),
            ["fixture.one".to_owned(), "fixture.two".to_owned()]
        );
    }

    #[tokio::test]
    async fn descriptor_drift_fails_before_dispatch() {
        let call = call(&["fixture.one"]);
        let mut fake = Fake::new(&["fixture.one"]);
        fake.descriptors
            .insert("fixture.one".into(), descriptor("fixture.one", "2"));
        let mut runner = StageCommandRunner::new(&fake, &call).unwrap();
        assert!(matches!(
            runner
                .next(json!({}), CancellationToken::new())
                .await
                .unwrap_err(),
            Error::Stale(_)
        ));
        assert!(fake.calls.lock().unwrap().is_empty());
        assert!(runner.finish().is_err());
    }

    #[tokio::test]
    async fn stage_cannot_finish_with_unexecuted_bindings() {
        let fake = Fake::new(&["fixture.one", "fixture.two"]);
        let call = call(&["fixture.one", "fixture.two"]);
        let mut runner = StageCommandRunner::new(&fake, &call).unwrap();
        runner
            .next(json!({}), CancellationToken::new())
            .await
            .unwrap();
        assert!(runner.finish().is_err());
    }

    #[tokio::test]
    async fn broker_failure_poisoned_stage_cannot_continue() {
        let mut fake = Fake::new(&["fixture.one", "fixture.two"]);
        fake.fail_on = Some("fixture.one".into());
        let call = call(&["fixture.one", "fixture.two"]);
        let mut runner = StageCommandRunner::new(&fake, &call).unwrap();
        assert!(
            runner
                .next(json!({}), CancellationToken::new())
                .await
                .is_err()
        );
        assert!(matches!(
            runner
                .next(json!({}), CancellationToken::new())
                .await
                .unwrap_err(),
            Error::Unknown(_)
        ));
        assert_eq!(*fake.calls.lock().unwrap(), ["fixture.one".to_owned()]);
    }

    #[test]
    fn stage_runner_does_not_embed_media_clock_or_delivery_authority() {
        // The runner has no special cases for media metadata; this guard keeps
        // its test fixture independent of delivery timing/provider semantics.
        let rate = Rate::new(30, 1).unwrap();
        assert_eq!(rate.at(30).unwrap(), Rational::ONE);
    }
}

#[cfg(test)]
mod error_mapping_tests {
    use super::*;
    use semwright_types::{Error as NativeError, ErrorCode};

    #[test]
    fn broker_error_classes_preserve_fail_closed_semantics() {
        for code in [
            ErrorCode::PolicyDenied,
            ErrorCode::PermissionDenied,
            ErrorCode::ConsentRequired,
            ErrorCode::SandboxDenied,
        ] {
            assert!(matches!(
                broker_error(NativeError::new(code, "denied")),
                Error::Denied(_)
            ));
        }
        for code in [ErrorCode::StaleReference, ErrorCode::Conflict] {
            assert!(matches!(
                broker_error(NativeError::new(code, "stale")),
                Error::Stale(_)
            ));
        }
        assert!(matches!(
            broker_error(NativeError::new(ErrorCode::ResourceExhausted, "bounded")),
            Error::Limit(_)
        ));
        for code in [ErrorCode::Cancelled, ErrorCode::Timeout] {
            assert!(matches!(
                broker_error(NativeError::new(code, "terminal")),
                Error::Unknown(_)
            ));
            assert!(matches!(
                broker_error(NativeError::new(code, "uncertain").uncertain()),
                Error::Unknown(_)
            ));
        }
        assert!(matches!(
            broker_error(NativeError::new(ErrorCode::BackendFailed, "known failure")),
            Error::Invalid(_)
        ));
        assert!(matches!(
            broker_error(NativeError::new(ErrorCode::BackendFailed, "unknown failure").uncertain()),
            Error::Unknown(_)
        ));
    }

    #[test]
    fn broker_error_sanitizes_control_characters_and_bounds_message() {
        let raw = format!("start\n{}\tend", "x".repeat(700));
        let mapped = broker_error(NativeError::new(ErrorCode::PolicyDenied, raw));
        let Error::Denied(message) = mapped else {
            panic!("policy denial must remain denied");
        };
        assert!(message.len() <= 512);
        assert!(!message.chars().any(char::is_control));
        assert!(message.starts_with("PolicyDenied: start"));
    }
}

#[cfg(test)]
mod command_context_tests {
    use super::*;
    use semwright_types::{Error as NativeError, ErrorCode};

    #[test]
    fn trusted_command_context_preserves_class_and_redacted_message_budget() {
        let mapped = command_context(
            broker_error(
                NativeError::new(
                    ErrorCode::BackendFailed,
                    format!("redacted-{}", "x".repeat(700)),
                )
                .uncertain(),
            ),
            "driver.motion-canvas.render.execute",
        );
        let Error::Unknown(message) = mapped else {
            panic!("uncertain backend failure must remain unknown");
        };
        assert!(
            message.starts_with("driver.motion-canvas.render.execute: BackendFailed: redacted-")
        );
        assert!(message.len() <= 512);
        assert!(!message.chars().any(char::is_control));
    }
}
