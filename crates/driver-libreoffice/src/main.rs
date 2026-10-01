//! First-party LibreOffice driver using the native UNO object model.
//! The agent receives typed capabilities; no arbitrary Python or macro surface is exposed.
use async_trait::async_trait;
use semwright_driver_sdk::{
    Capability, Driver, DriverExecutionContext, DriverInterfaces, RuntimeToolArg,
    RuntimeToolSession, artifact_output_tag, descriptor_digest, serve,
};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, MAX_FRAME, Result, Risk};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    time::Duration,
};

const DRIVER_ID: &str = "libreoffice";
const DRIVER_SCOPE: &str = "driver:libreoffice";
const WORKSPACE_MOUNT: &str = "workspace";
const RUNTIME_MOUNT: &str = "libreoffice-runtime";
const SESSION_RUNNER_TOOL: &str = "libreoffice-session-runner";
const MAX_DRIVER_SESSIONS: usize = 2;

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

fn capabilities() -> Result<Vec<Capability>> {
    let mut capabilities = vec![
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
    ];
    let pdf = capabilities
        .iter_mut()
        .find(|capability| capability.descriptor.name == "driver.libreoffice.export.pdf")
        .expect("LibreOffice PDF export capability is part of the static catalog");
    pdf.tags.push(artifact_output_tag("document/pdf")?);
    Ok(capabilities)
}

fn capability(command: &str) -> Result<Capability> {
    capabilities()?
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
    let workspace = semwright_driver_sdk::workspace_mount(WORKSPACE_MOUNT)?;
    Ok((raw.into(), workspace.join(path)))
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

fn session_arguments() -> Vec<RuntimeToolArg> {
    vec![
        RuntimeToolArg::Literal {
            value: "--workspace".into(),
        },
        RuntimeToolArg::MountPath {
            mount: WORKSPACE_MOUNT.into(),
            relative: String::new(),
        },
        RuntimeToolArg::Literal {
            value: "--runtime".into(),
        },
        RuntimeToolArg::MountPath {
            mount: RUNTIME_MOUNT.into(),
            relative: String::new(),
        },
        RuntimeToolArg::Literal {
            value: "--soffice-sealed".into(),
        },
        RuntimeToolArg::ToolPath {
            tool: "soffice-bin".into(),
        },
        RuntimeToolArg::Literal {
            value: "--python".into(),
        },
        RuntimeToolArg::ToolPath {
            tool: "python3".into(),
        },
    ]
}

fn decode_uno_response(bytes: &[u8]) -> Result<Value> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(Error::new(
            ErrorCode::PluginProtocolError,
            "LibreOffice session response exceeds its bounded contract",
        ));
    }
    let response: Value = serde_json::from_slice(bytes)?;
    let object = response.as_object().ok_or_else(|| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "LibreOffice session response must be an object",
        )
    })?;
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        if object.len() != 2 || !object.contains_key("data") {
            return Err(Error::new(
                ErrorCode::PluginProtocolError,
                "LibreOffice success response exceeds its fixed envelope",
            ));
        }
        return response
            .get("data")
            .cloned()
            .filter(Value::is_object)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PluginProtocolError,
                    "LibreOffice UNO result must be an object",
                )
            });
    }
    if response.get("ok").and_then(Value::as_bool) != Some(false) {
        return Err(Error::new(
            ErrorCode::PluginProtocolError,
            "LibreOffice session response has an invalid envelope",
        ));
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

struct LibreOffice {
    sessions: BTreeMap<String, RuntimeToolSession>,
    versions: BTreeMap<String, String>,
}

impl LibreOffice {
    async fn start() -> Result<Self> {
        let workspace_root = semwright_driver_sdk::workspace_mount(WORKSPACE_MOUNT)?;
        let workspace = std::fs::metadata(&workspace_root)
            .map_err(|_| Error::unavailable("LibreOffice driver requires the workspace mount"))?;
        if !workspace.is_dir() {
            return Err(Error::unavailable(
                "LibreOffice workspace mount is not a directory",
            ));
        }
        Ok(Self {
            sessions: BTreeMap::new(),
            versions: BTreeMap::new(),
        })
    }

    fn validate_call(command: &str, pinned_digest: &str) -> Result<Capability> {
        let capability = capability(command)?;
        if descriptor_digest(&capability.descriptor)? != pinned_digest {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "LibreOffice capability descriptor changed",
            ));
        }
        Ok(capability)
    }

    async fn exchange(
        context: &DriverExecutionContext,
        session: &RuntimeToolSession,
        operation: &str,
        args: Value,
        timeout_ms: u64,
    ) -> Result<Value> {
        let payload = serde_json::to_vec(&json!({"operation":operation,"args":args}))?;
        if payload.is_empty() || payload.len() > MAX_FRAME {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "LibreOffice session request exceeds its bounded contract",
            ));
        }
        let response = context
            .runtime_tool_session_request(
                session,
                payload,
                Duration::from_millis(timeout_ms.max(1)),
            )
            .await?;
        decode_uno_response(&response)
    }

    async fn ensure_session(
        &mut self,
        context: &DriverExecutionContext,
    ) -> Result<RuntimeToolSession> {
        if let Some(session) = self.sessions.get(context.session()) {
            return Ok(session.clone());
        }
        if self.sessions.len() >= MAX_DRIVER_SESSIONS {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "LibreOffice DriverProvider session capacity is exhausted",
            ));
        }
        let session = context
            .start_runtime_tool_session_args(
                SESSION_RUNNER_TOOL,
                session_arguments(),
                Duration::from_secs(3_600),
                None,
            )
            .await?;

        let status = match Self::exchange(context, &session, "status", json!({}), 20_000).await {
            Ok(status) => status,
            Err(error) => {
                let _ = context.close_runtime_tool_session(&session).await;
                return Err(error);
            }
        };
        let version = status
            .get("version")
            .and_then(Value::as_str)
            .filter(|value| {
                !value.is_empty() && value.len() <= 64 && !value.chars().any(char::is_control)
            })
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PluginProtocolError,
                    "LibreOffice status omitted its bounded product version",
                )
            })?
            .to_owned();
        self.sessions
            .insert(context.session().to_owned(), session.clone());
        self.versions.insert(context.session().to_owned(), version);
        Ok(session)
    }

    async fn uno(
        context: &DriverExecutionContext,
        session: &RuntimeToolSession,
        operation: &str,
        args: Value,
        timeout_ms: u64,
    ) -> Result<Value> {
        Self::exchange(context, session, operation, args, timeout_ms).await
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

    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            cooperative_cancellation: true,
            health: true,
            host_tools: std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_some(),
            ..DriverInterfaces::default()
        }
    }

    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        capabilities()
    }

    async fn health(&mut self) -> Result<Value> {
        let host_tools = std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_some();
        Ok(json!({
            "healthy":host_tools,
            "product":"LibreOffice",
            "runtime":"host-managed-v8",
            "sessions":self.sessions.len(),
            "version":self.versions.values().next().cloned()
        }))
    }

    async fn execute(&mut self, command: &str, pinned_digest: &str, _args: Value) -> Result<Value> {
        let _ = Self::validate_call(command, pinned_digest)?;
        Err(Error::new(
            ErrorCode::Unsupported,
            "LibreOffice execution requires a protocol-v8 Host-owned runtime-tool session",
        ))
    }

    async fn execute_with_context(
        &mut self,
        command: &str,
        pinned_digest: &str,
        args: Value,
        context: DriverExecutionContext,
    ) -> Result<Value> {
        context.check_cancelled()?;
        let capability = Self::validate_call(command, pinned_digest)?;
        let session = self.ensure_session(&context).await?;
        let result = match command {
            "driver.libreoffice.status" => {
                Self::uno(
                    &context,
                    &session,
                    "status",
                    json!({}),
                    capability.descriptor.timeout_ms,
                )
                .await
            }
            "driver.libreoffice.writer.create" => {
                let (relative, absolute) = relative_path(&args, "path", &["odt"])?;
                ensure_new_target(&absolute)?;
                let text = text_arg(&args, "text", 65_536)?;
                Self::uno(
                    &context,
                    &session,
                    "writer_create",
                    json!({"path":relative,"text":text}),
                    capability.descriptor.timeout_ms,
                )
                .await?;
                Ok(json!({"created":true,"path":relative}))
            }
            "driver.libreoffice.writer.read" => {
                let (relative, absolute) = relative_path(&args, "path", &["odt"])?;
                ensure_source(&absolute)?;
                let data = Self::uno(
                    &context,
                    &session,
                    "writer_read",
                    json!({"path":relative}),
                    capability.descriptor.timeout_ms,
                )
                .await?;
                Ok(json!({"path":relative,"text":data["text"]}))
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
                let data = Self::uno(
                    &context,
                    &session,
                    "calc_create",
                    json!({"path":relative,"cells":cells}),
                    capability.descriptor.timeout_ms,
                )
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
                let data = Self::uno(
                    &context,
                    &session,
                    "calc_get",
                    json!({"path":relative,"cell":cell}),
                    capability.descriptor.timeout_ms,
                )
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
                Self::uno(
                    &context,
                    &session,
                    "calc_set",
                    json!({"path":relative,"cell":cell,"value":value}),
                    capability.descriptor.timeout_ms,
                )
                .await?;
                Ok(json!({"updated":true,"path":relative,"cell":cell}))
            }
            "driver.libreoffice.export.pdf" => {
                let (source_relative, source) = relative_path(&args, "path", &["odt", "ods"])?;
                ensure_source(&source)?;
                let (output_relative, output) = relative_path(&args, "output", &["pdf"])?;
                ensure_new_target(&output)?;
                Self::uno(
                    &context,
                    &session,
                    "export_pdf",
                    json!({"path":source_relative,"output":output_relative}),
                    capability.descriptor.timeout_ms,
                )
                .await?;
                Ok(json!({"exported":true,"path":output_relative}))
            }
            _ => Err(Error::new(
                ErrorCode::NotFound,
                "LibreOffice capability is not registered",
            )),
        };

        match result {
            Ok(value) => Ok(value),
            Err(error)
                if matches!(
                    error.code,
                    ErrorCode::Unavailable
                        | ErrorCode::ProtocolMismatch
                        | ErrorCode::PluginProtocolError
                        | ErrorCode::Timeout
                        | ErrorCode::Cancelled
                        | ErrorCode::NotFound
                ) =>
            {
                self.sessions.remove(context.session());
                self.versions.remove(context.session());
                let _ = context.close_runtime_tool_session(&session).await;
                Err(error)
            }
            Err(error) => Err(error),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_export_advertises_generic_artifact_output() {
        let capabilities = capabilities().unwrap();
        let pdf = capabilities
            .iter()
            .find(|capability| capability.descriptor.name == "driver.libreoffice.export.pdf")
            .unwrap();
        assert!(
            pdf.tags
                .iter()
                .any(|tag| tag == "artifact-out:document/pdf")
        );
    }
}
