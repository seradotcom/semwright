//! First-party LibreOffice driver using the native UNO object model.
//! The agent receives typed capabilities; no arbitrary Python or macro surface is exposed.
use async_trait::async_trait;
use semwright_driver_sdk::{Capability, Driver, descriptor_digest, serve};
use semwright_types::{
    CommandDescriptor, Error, ErrorCode, Idempotency, MAX_FRAME, Result, Risk, unique_id,
};
use serde_json::{Value, json};
use std::{
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    process::{Child, Command},
    time::{sleep, timeout},
};

const DRIVER_ID: &str = "libreoffice";
const DRIVER_SCOPE: &str = "driver:libreoffice";
const WORKSPACE: &str = "/workspace/workspace";
const PYTHON: &str = "/usr/bin/python3";
const SOFFICE: &str = "/usr/bin/soffice";
const SYSTEM_SHELL: &str = "/usr/bin/sh";
const UNO_SCRIPT: &str = include_str!("uno_bridge.py");

fn descriptor(
    name: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
    risk: Risk,
    idempotency: Idempotency,
    timeout_ms: u64,
) -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: name.into(),
            version: "1".into(),
            description: description.into(),
            input_schema,
            output_schema,
            requires: vec![DRIVER_SCOPE.into()],
            risk,
            idempotency,
            timeout_ms,
            dry_run: risk == Risk::ReadOnly,
            interactive_consent: false,
            backends: vec![DRIVER_SCOPE.into()],
        },
        aliases: vec![],
        tags: vec!["libreoffice".into(), "uno".into()],
        object_types: vec![],
    }
}

fn path_schema() -> Value {
    json!({"type":"string","minLength":1,"maxLength":240})
}

fn capabilities() -> Vec<Capability> {
    vec![
        descriptor(
            "driver.libreoffice.status",
            "Inspect the sandbox-owned LibreOffice UNO runtime",
            json!({"type":"object","additionalProperties":false}),
            json!({
                "type":"object",
                "properties":{
                    "connected":{"const":true},
                    "product":{"const":"LibreOffice"},
                    "version":{"type":"string","minLength":1,"maxLength":64}
                },
                "required":["connected","product","version"],
                "additionalProperties":false
            }),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            5_000,
        ),
        descriptor(
            "driver.libreoffice.writer.create",
            "Create a new ODT document with bounded plain text",
            json!({
                "type":"object",
                "properties":{
                    "path":path_schema(),
                    "text":{"type":"string","maxLength":65536}
                },
                "required":["path","text"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "created":{"const":true},
                    "path":path_schema()
                },
                "required":["created","path"],
                "additionalProperties":false
            }),
            Risk::Mutating,
            Idempotency::NonIdempotent,
            15_000,
        ),
        descriptor(
            "driver.libreoffice.writer.read",
            "Read bounded plain text from an ODT document",
            json!({
                "type":"object",
                "properties":{"path":path_schema()},
                "required":["path"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "path":path_schema(),
                    "text":{"type":"string","maxLength":65536}
                },
                "required":["path","text"],
                "additionalProperties":false
            }),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            10_000,
        ),
        descriptor(
            "driver.libreoffice.calc.create",
            "Create an ODS spreadsheet from a bounded map of A1-style cells",
            json!({
                "type":"object",
                "properties":{
                    "path":path_schema(),
                    "cells":{
                        "type":"object",
                        "maxProperties":256,
                        "propertyNames":{"pattern":"^[A-Z]{1,3}[1-9][0-9]{0,5}$"},
                        "additionalProperties":{
                            "oneOf":[
                                {"type":"string","maxLength":32768},
                                {"type":"number"}
                            ]
                        }
                    }
                },
                "required":["path","cells"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "created":{"const":true},
                    "path":path_schema(),
                    "cells_written":{"type":"integer","minimum":0,"maximum":256}
                },
                "required":["created","path","cells_written"],
                "additionalProperties":false
            }),
            Risk::Mutating,
            Idempotency::NonIdempotent,
            15_000,
        ),
        descriptor(
            "driver.libreoffice.calc.get",
            "Read one cell from the first sheet of an ODS spreadsheet",
            json!({
                "type":"object",
                "properties":{
                    "path":path_schema(),
                    "cell":{"type":"string","pattern":"^[A-Z]{1,3}[1-9][0-9]{0,5}$"}
                },
                "required":["path","cell"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "path":path_schema(),
                    "cell":{"type":"string"},
                    "kind":{"enum":["empty","number","text","formula"]},
                    "value":{"oneOf":[
                        {"type":"null"},
                        {"type":"number"},
                        {"type":"string","maxLength":65536}
                    ]}
                },
                "required":["path","cell","kind","value"],
                "additionalProperties":false
            }),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            10_000,
        ),
        descriptor(
            "driver.libreoffice.calc.set",
            "Set one cell in the first sheet of an existing ODS spreadsheet",
            json!({
                "type":"object",
                "properties":{
                    "path":path_schema(),
                    "cell":{"type":"string","pattern":"^[A-Z]{1,3}[1-9][0-9]{0,5}$"},
                    "value":{"oneOf":[
                        {"type":"string","maxLength":32768},
                        {"type":"number"}
                    ]}
                },
                "required":["path","cell","value"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "updated":{"const":true},
                    "path":path_schema(),
                    "cell":{"type":"string"}
                },
                "required":["updated","path","cell"],
                "additionalProperties":false
            }),
            Risk::Mutating,
            Idempotency::Idempotent,
            15_000,
        ),
        descriptor(
            "driver.libreoffice.export.pdf",
            "Export an ODT or ODS document to a new PDF",
            json!({
                "type":"object",
                "properties":{
                    "path":path_schema(),
                    "output":path_schema()
                },
                "required":["path","output"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "exported":{"const":true},
                    "path":path_schema()
                },
                "required":["exported","path"],
                "additionalProperties":false
            }),
            Risk::Mutating,
            Idempotency::NonIdempotent,
            20_000,
        ),
    ]
}

fn capability(command: &str) -> Result<Capability> {
    capabilities()
        .into_iter()
        .find(|capability| capability.descriptor.name == command)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::NotFound,
                "LibreOffice capability is not registered",
            )
        })
}

fn text_arg<'a>(args: &'a Value, key: &str, max: usize) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| value.len() <= max && !value.contains(char::from(0)))
        .ok_or_else(|| Error::invalid("LibreOffice string argument is invalid"))
}

fn relative_path(args: &Value, key: &str, extensions: &[&str]) -> Result<(String, PathBuf)> {
    let raw = text_arg(args, key, 240)?;
    let path = Path::new(raw);
    if path.is_absolute()
        || path.components().count() == 0
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "LibreOffice paths must be simple relative workspace paths",
        ));
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !extensions.iter().any(|allowed| *allowed == extension) {
        return Err(Error::invalid(
            "LibreOffice document extension is not allowed",
        ));
    }
    Ok((raw.into(), Path::new(WORKSPACE).join(path)))
}

fn ensure_source(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| {
        Error::new(
            ErrorCode::NotFound,
            "LibreOffice source document was not found",
        )
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "LibreOffice source must be a regular non-symlink file",
        ));
    }
    Ok(())
}

fn ensure_new_target(path: &Path) -> Result<()> {
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(Error::new(
            ErrorCode::Conflict,
            "LibreOffice output already exists",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("LibreOffice output has no parent"))?;
    let metadata = std::fs::metadata(parent)
        .map_err(|_| Error::new(ErrorCode::NotFound, "LibreOffice output parent is absent"))?;
    if !metadata.is_dir() {
        return Err(Error::invalid(
            "LibreOffice output parent is not a directory",
        ));
    }
    Ok(())
}
fn valid_cell(cell: &str) -> bool {
    if cell.len() < 2 || cell.len() > 9 || !cell.is_ascii() {
        return false;
    }
    let split = cell
        .bytes()
        .position(|byte| byte.is_ascii_digit())
        .unwrap_or(cell.len());
    let (letters, digits) = cell.split_at(split);
    !letters.is_empty()
        && letters.len() <= 3
        && letters.bytes().all(|byte| byte.is_ascii_uppercase())
        && !digits.is_empty()
        && digits.len() <= 6
        && !digits.starts_with('0')
        && digits.bytes().all(|byte| byte.is_ascii_digit())
}

async fn run_bridge(pipe: &str, operation: &str, args: Value, deadline: Duration) -> Result<Value> {
    let payload = serde_json::to_string(&args)?;
    let mut command = Command::new(PYTHON);
    command
        .arg("-c")
        .arg(UNO_SCRIPT)
        .arg(pipe)
        .arg(operation)
        .arg(payload)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn()?;
    let output = timeout(deadline, child.wait_with_output())
        .await
        .map_err(|_| Error::new(ErrorCode::Timeout, "LibreOffice UNO helper timed out"))??;
    if !output.status.success() || output.stdout.len() > MAX_FRAME {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "LibreOffice UNO helper failed",
        ));
    }
    let response: Value = serde_json::from_slice(&output.stdout)?;
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        return response
            .get("data")
            .cloned()
            .filter(Value::is_object)
            .ok_or_else(|| {
                Error::new(ErrorCode::ProtocolMismatch, "UNO result must be an object")
            });
    }
    let code = match response.get("code").and_then(Value::as_str) {
        Some("NotFound") => ErrorCode::NotFound,
        Some("Conflict") => ErrorCode::Conflict,
        Some("InvalidArgument") => ErrorCode::InvalidArgument,
        Some("Unsupported") => ErrorCode::Unsupported,
        _ => ErrorCode::BackendFailed,
    };
    Err(Error::new(
        code,
        "LibreOffice rejected the typed UNO operation",
    ))
}

fn io_step(step: &str, error: std::io::Error) -> Error {
    Error::new(
        ErrorCode::BackendFailed,
        format!("{step} failed ({:?})", error.kind()),
    )
}

struct LibreOffice {
    office: Child,
    pipe: String,
    profile: PathBuf,
    version: String,
}

impl LibreOffice {
    async fn start() -> Result<Self> {
        if !Path::new(SOFFICE).is_file()
            || !Path::new(SYSTEM_SHELL).is_file()
            || !Path::new(PYTHON).is_file()
        {
            return Err(Error::unavailable(
                "LibreOffice and the system Python UNO bridge are required",
            ));
        }
        let workspace = std::fs::metadata(WORKSPACE)
            .map_err(|_| Error::unavailable("LibreOffice driver requires the workspace mount"))?;
        if !workspace.is_dir() {
            return Err(Error::unavailable(
                "LibreOffice workspace mount is not a directory",
            ));
        }
        let token = unique_id().replace('-', "_");
        let pipe = format!("semwright_lo_{token}");
        let profile = PathBuf::from(format!("/tmp/semwright-libreoffice-{token}"));
        std::fs::create_dir(&profile)
            .map_err(|error| io_step("LibreOffice profile creation", error))?;
        let profile_arg = format!("-env:UserInstallation=file://{}", profile.display());
        let accept = format!("--accept=pipe,name={pipe};urp;StarOffice.ComponentContext");
        let mut command = Command::new(SYSTEM_SHELL);
        command
            .arg(SOFFICE)
            .args([
                "--headless",
                "--nologo",
                "--nodefault",
                "--nofirststartwizard",
                "--norestore",
                &profile_arg,
                &accept,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut office = command
            .spawn()
            .map_err(|error| io_step("LibreOffice process launch", error))?;
        drop(office.stdin.take());
        if let Some(mut stdout) = office.stdout.take() {
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut stdout, &mut tokio::io::sink()).await;
            });
        }
        if let Some(mut stderr) = office.stderr.take() {
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
            });
        }
        for _ in 0..120 {
            if office
                .try_wait()
                .map_err(|error| io_step("LibreOffice process status", error))?
                .is_some()
            {
                return Err(Error::unavailable(
                    "Sandbox-owned LibreOffice exited during startup",
                ));
            }
            if let Ok(status) = run_bridge(&pipe, "status", json!({}), Duration::from_secs(2)).await
                && let Some(version) = status.get("version").and_then(Value::as_str)
            {
                return Ok(Self {
                    office,
                    pipe,
                    profile,
                    version: version.into(),
                });
            }
            sleep(Duration::from_millis(50)).await;
        }
        let _ = office.kill().await;
        Err(Error::new(
            ErrorCode::Timeout,
            "LibreOffice UNO startup timed out",
        ))
    }

    async fn uno(&self, operation: &str, args: Value, seconds: u64) -> Result<Value> {
        run_bridge(&self.pipe, operation, args, Duration::from_secs(seconds)).await
    }
}

impl Drop for LibreOffice {
    fn drop(&mut self) {
        let _ = self.office.start_kill();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

#[async_trait]
impl Driver for LibreOffice {
    fn id(&self) -> &str {
        DRIVER_ID
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        Ok(capabilities())
    }

    async fn health(&mut self) -> Result<Value> {
        Ok(json!({
            "healthy":true,
            "product":"LibreOffice",
            "version":self.version
        }))
    }

    async fn execute(&mut self, command: &str, pinned_digest: &str, args: Value) -> Result<Value> {
        let capability = capability(command)?;
        if descriptor_digest(&capability.descriptor)? != pinned_digest {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "LibreOffice capability descriptor changed",
            ));
        }
        match command {
            "driver.libreoffice.status" => Ok(json!({
                "connected":true,
                "product":"LibreOffice",
                "version":self.version
            })),
            "driver.libreoffice.writer.create" => {
                let (relative, absolute) = relative_path(&args, "path", &["odt"])?;
                ensure_new_target(&absolute)?;
                let text = text_arg(&args, "text", 65_536)?;
                self.uno("writer_create", json!({"path":absolute,"text":text}), 12)
                    .await?;
                Ok(json!({"created":true,"path":relative}))
            }
            "driver.libreoffice.writer.read" => {
                let (relative, absolute) = relative_path(&args, "path", &["odt"])?;
                ensure_source(&absolute)?;
                let data = self.uno("writer_read", json!({"path":absolute}), 8).await?;
                Ok(json!({
                    "path":relative,
                    "text":data["text"]
                }))
            }
            "driver.libreoffice.calc.create" => {
                let (relative, absolute) = relative_path(&args, "path", &["ods"])?;
                ensure_new_target(&absolute)?;
                let cells = args
                    .get("cells")
                    .and_then(Value::as_object)
                    .filter(|cells| cells.len() <= 256)
                    .ok_or_else(|| Error::invalid("LibreOffice cells map is invalid"))?;
                if !cells.keys().all(|cell| valid_cell(cell)) {
                    return Err(Error::invalid("LibreOffice cell reference is invalid"));
                }
                let data = self
                    .uno("calc_create", json!({"path":absolute,"cells":cells}), 12)
                    .await?;
                Ok(json!({
                    "created":true,
                    "path":relative,
                    "cells_written":data["cells_written"]
                }))
            }
            "driver.libreoffice.calc.get" => {
                let (relative, absolute) = relative_path(&args, "path", &["ods"])?;
                ensure_source(&absolute)?;
                let cell = text_arg(&args, "cell", 9)?;
                if !valid_cell(cell) {
                    return Err(Error::invalid("LibreOffice cell reference is invalid"));
                }
                let data = self
                    .uno("calc_get", json!({"path":absolute,"cell":cell}), 8)
                    .await?;
                Ok(json!({
                    "path":relative,
                    "cell":cell,
                    "kind":data["kind"],
                    "value":data["value"]
                }))
            }
            "driver.libreoffice.calc.set" => {
                let (relative, absolute) = relative_path(&args, "path", &["ods"])?;
                ensure_source(&absolute)?;
                let cell = text_arg(&args, "cell", 9)?;
                if !valid_cell(cell) {
                    return Err(Error::invalid("LibreOffice cell reference is invalid"));
                }
                let value = args
                    .get("value")
                    .cloned()
                    .filter(|value| value.is_string() || value.is_number())
                    .ok_or_else(|| Error::invalid("LibreOffice cell value is invalid"))?;
                self.uno(
                    "calc_set",
                    json!({"path":absolute,"cell":cell,"value":value}),
                    12,
                )
                .await?;
                Ok(json!({
                    "updated":true,
                    "path":relative,
                    "cell":cell
                }))
            }
            "driver.libreoffice.export.pdf" => {
                let (_source_relative, source) = relative_path(&args, "path", &["odt", "ods"])?;
                ensure_source(&source)?;
                let (output_relative, output) = relative_path(&args, "output", &["pdf"])?;
                ensure_new_target(&output)?;
                self.uno("export_pdf", json!({"path":source,"output":output}), 18)
                    .await?;
                Ok(json!({
                    "exported":true,
                    "path":output_relative
                }))
            }
            _ => Err(Error::new(
                ErrorCode::NotFound,
                "LibreOffice capability is not registered",
            )),
        }
    }
}

#[tokio::main]
async fn main() {
    let result = match LibreOffice::start().await {
        Ok(driver) => serve(driver).await,
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}
