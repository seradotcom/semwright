//! Linux subprocess fixture for the genuine Core/Host/NativeSDK permit path.
//! Its fixed marker is not Motionwright storage, Graph CAS or installed acceptance.
#[cfg(target_os = "linux")]
mod linux {
    use semwright_native_sdk::cooperation::{
        Application, CallContext, CancellationSemantics, CommitSemantics, Completion,
        ObservationPage, ObservationProvider, OperationContract, OperationHandler, Query,
        ResourceVersion, RetrySemantics, RevisionToken, TargetRequirement, UndoSemantics,
    };
    use semwright_native_sdk::{
        CommandDescriptor, Error, ErrorCode, Idempotency, NativeDriver, Result, Risk, Value,
        async_trait, driver_sdk, json, serve,
    };
    use std::{
        fs::{File, OpenOptions},
        io::{Read, Write},
        os::unix::fs::OpenOptionsExt,
        path::{Path, PathBuf},
        sync::Arc,
        time::{Duration, Instant},
    };
    const RESOURCE: &str = "fixture:owner-operation";
    const MOUNT: &str = "motionwright-data";
    const CONTROL: &str = "permit-fixture-control";
    fn version() -> Result<ResourceVersion> {
        Ok(ResourceVersion {
            resource: RESOURCE.into(),
            generation: "fixture-generation-1".into(),
            revision: RevisionToken::new("1")?,
        })
    }
    fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }
    fn publish_control(control: &Path, marker: &str) -> Result<()> {
        let (staging_name, bytes): (&str, &[u8]) = match marker {
            "entered" => ("entered.pending", b"entered\n"),
            "close-seen" => ("close-seen.pending", b"closed\n"),
            "release" => ("release.pending", b"go\n"),
            _ => return Err(Error::invalid("Unknown fixture control marker")),
        };
        let staging = control.join(staging_name);
        let destination = control.join(marker);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staging)?;
        // Preserve staging on any failure; do not hide partial custody or retry it.
        file.write_all(bytes)
            .map_err(|error| Error::from(error).uncertain())?;
        file.sync_all()
            .map_err(|error| Error::from(error).uncertain())?;
        drop(file);
        std::fs::hard_link(&staging, &destination)
            .map_err(|error| Error::from(error).uncertain())?;
        // Only remove our staging alias after publication. Never roll back destination.
        std::fs::remove_file(&staging).map_err(|error| Error::from(error).uncertain())?;
        Ok(())
    }
    fn released(control: &Path) -> Result<bool> {
        let file = match File::open(control.join("release")) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(16).read_to_end(&mut bytes)?;
        if bytes != b"go\n" {
            return Err(Error::invalid("Invalid fixture release"));
        }
        Ok(true)
    }
    fn wait_release(control: &Path, deadline: Instant) -> Result<()> {
        while Instant::now() < deadline {
            if released(control)? {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Err(Error::new(
            ErrorCode::Timeout,
            "Fixture release deadline expired",
        ))
    }
    struct Fixture {
        data: PathBuf,
        control: PathBuf,
    }
    #[async_trait]
    impl ObservationProvider for Fixture {
        async fn observe(&self, query: &Query, context: &CallContext) -> Result<ObservationPage> {
            context.driver_context()?;
            context.check_cancelled()?;
            if query.resource != RESOURCE || query.scope != "fixture" || query.limit != 1 {
                return Err(Error::invalid("Unexpected fixture observation"));
            }
            Ok(ObservationPage {
                version: version()?,
                scope: query.scope.clone(),
                items: vec![json!({"kind":"owner-operation-fixture"})],
                next: None,
                complete: true,
            })
        }
    }
    impl Fixture {
        async fn execute(&self, args: Value, context: &CallContext) -> Result<Value> {
            if context.expected() != Some(&version()?) {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "Fixture resource differs",
                ));
            }
            let closing = match args.get("mode").and_then(Value::as_str) {
                Some("commit") => false,
                Some("closing") => true,
                _ => return Err(Error::invalid("Unknown fixture mode")),
            };
            let permit = context.acquire_operation_permit().await?;
            if permit.logical_mount() != MOUNT {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Fixture mount differs",
                ));
            }
            let guard = context.enter_operation_permit(&permit)?;
            guard.check_current()?;
            publish_control(&self.control, "entered")?;
            let deadline = Instant::now() + Duration::from_secs(10);
            if closing {
                let original = loop {
                    if let Err(error) = guard.check_current() {
                        break error;
                    }
                    if released(&self.control)? {
                        return Err(Error::invalid("Release arrived before owner Close"));
                    }
                    if Instant::now() >= deadline {
                        return Err(Error::new(ErrorCode::Timeout, "Owner Close did not arrive"));
                    }
                    std::thread::sleep(Duration::from_millis(2));
                };
                context.check_cancelled()?;
                if original.code != ErrorCode::Cancelled {
                    return Err(Error::new(
                        ErrorCode::Internal,
                        "Unexpected permit Close result",
                    ));
                }
                publish_control(&self.control, "close-seen")?;
                wait_release(&self.control, deadline)?;
                if guard.check_current().is_ok() {
                    return Err(Error::new(
                        ErrorCode::Internal,
                        "Closed permit became current",
                    ));
                }
                drop(guard);
                drop(permit);
                return Err(original);
            }
            wait_release(&self.control, deadline)?;
            guard.check_current()?;
            context.check_cancelled()?;
            write_new(&self.data.join("effect"), b"effect-once\n").map_err(Error::uncertain)?;
            drop(guard);
            drop(permit);
            Ok(json!({"committed":true}))
        }
    }
    #[async_trait]
    impl OperationHandler for Fixture {
        async fn invoke(&self, args: Value, context: &CallContext) -> Completion {
            match self.execute(args, context).await {
                Ok(value) => Completion::Applied(value),
                Err(error) if error.outcome_known => Completion::NotApplied(error),
                Err(error) => Completion::Uncertain(error),
            }
        }
    }
    pub async fn run() -> Result<()> {
        let fixture = Arc::new(Fixture {
            data: driver_sdk::workspace_mount(MOUNT)?,
            control: driver_sdk::workspace_mount(CONTROL)?,
        });
        let app = Application::new("owner-permit-fixture", env!("CARGO_PKG_VERSION"))?
            .with_observer(fixture.clone())
            .register(
                OperationContract {
                    descriptor: CommandDescriptor {
                        name: "driver.owner-permit-fixture.apply".into(),
                        version: "1".into(),
                        description: "Exercise one genuine owner operation permit".into(),
                        input_schema: json!({"type":"object","properties":{
                        "ref":{"type":"string"},"mode":{"enum":["commit","closing"]}},
                        "required":["ref","mode"],"additionalProperties":false}),
                        output_schema: json!({"type":"object","properties":{
                        "committed":{"const":true}},"required":["committed"],
                        "additionalProperties":false}),
                        requires: vec!["driver:owner-permit-fixture".into()],
                        risk: Risk::Mutating,
                        idempotency: Idempotency::NonIdempotent,
                        timeout_ms: 15_000,
                        dry_run: false,
                        interactive_consent: false,
                        backends: vec!["driver:owner-permit-fixture".into()],
                    },
                    target: TargetRequirement::ObservedResource,
                    commit: CommitSemantics::NonAtomic,
                    retry: RetrySemantics::Never,
                    undo: UndoSemantics::None,
                    cancellation: CancellationSemantics::Cooperative,
                    atomic_revision_cas: false,
                },
                fixture,
            )?;
        serve(NativeDriver::new(app)?).await
    }
}
#[cfg(target_os = "linux")]
#[tokio::main(worker_threads = 2)]
async fn main() {
    if let Err(error) = linux::run().await {
        eprintln!("owner operation fixture failed: {error}");
        std::process::exit(error.exit_code());
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("owner operation fixture requires Linux");
    std::process::exit(2);
}
