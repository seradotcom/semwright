//! Sandboxed first-party Blender driver.
//!
//! Curated operations reuse the existing allowlisted Blender command implementation. Runtime
//! introspection exposes bounded RNA/operator/add-on metadata but never arbitrary Python or generic
//! operator invocation.
use async_trait::async_trait;
use semwright_driver_sdk::{Capability, Driver, descriptor_digest, serve};
use semwright_protocol::{read_frame, write_frame};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk, unique_id};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    net::UnixStream,
    process::{Child, Command},
    time::{sleep, timeout},
};

const DRIVER_ID: &str = "blender";
const DRIVER_SCOPE: &str = "driver:blender";
const WORKSPACE: &str = "/workspace/workspace";
const LEGACY_DESCRIPTORS: &str = include_str!("../../../schemas/commands.json");
const COMMANDS_PY: &str = include_str!("../../../adapters/blender/semwright_blender/commands.py");
const COMMANDS_JSON: &str =
    include_str!("../../../adapters/blender/semwright_blender/commands.json");
const VALIDATION_PY: &str =
    include_str!("../../../adapters/blender/semwright_blender/validation.py");
const BRIDGE_PY: &str = include_str!("bridge.py");

fn blender_binary() -> Result<&'static str> {
    ["/usr/local/bin/blender", "/usr/bin/blender"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
        .ok_or_else(|| Error::unavailable("A supported Blender binary is required"))
}

fn curated_capabilities() -> Result<Vec<Capability>> {
    let rows: Vec<CommandDescriptor> = serde_json::from_str(LEGACY_DESCRIPTORS)?;
    let mut out = Vec::new();
    for mut descriptor in rows
        .into_iter()
        .filter(|descriptor| descriptor.name.starts_with("blender."))
    {
        descriptor.name = format!("driver.{}", descriptor.name);
        descriptor.requires = vec![DRIVER_SCOPE.into()];
        descriptor.backends = vec![DRIVER_SCOPE.into()];
        let object_type = descriptor
            .name
            .split('.')
            .nth(2)
            .filter(|name| {
                matches!(
                    *name,
                    "scene" | "object" | "collection" | "material" | "render" | "file"
                )
            })
            .map(|name| vec![name.into()])
            .unwrap_or_default();
        let mut tags = vec!["blender".into(), "native".into(), "curated".into()];
        match descriptor.name.as_str() {
            "driver.blender.file.save" => {
                tags.push(semwright_driver_sdk::artifact_output_tag("model/3d")?)
            }
            "driver.blender.render" => {
                tags.push(semwright_driver_sdk::artifact_output_tag("image/raster")?)
            }
            _ => {}
        }
        out.push(Capability {
            descriptor,
            aliases: vec![],
            tags,
            object_types: object_type,
        });
    }
    if out.is_empty() {
        return Err(Error::new(
            ErrorCode::Internal,
            "Built-in Blender descriptors are absent",
        ));
    }
    Ok(out)
}

fn descriptor(
    name: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
    object_types: &[&str],
) -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: name.into(),
            version: "1".into(),
            description: description.into(),
            input_schema,
            output_schema,
            requires: vec![DRIVER_SCOPE.into()],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 10_000,
            dry_run: true,
            interactive_consent: false,
            backends: vec![DRIVER_SCOPE.into()],
        },
        aliases: vec![],
        tags: vec!["blender".into(), "rna".into(), "introspection".into()],
        object_types: object_types.iter().map(|value| (*value).into()).collect(),
    }
}
fn query_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "query":{"type":"string","maxLength":256},
            "limit":{"type":"integer","minimum":1,"maximum":256,"default":50}
        },
        "additionalProperties":false
    })
}

fn introspection_capabilities() -> Vec<Capability> {
    vec![
        descriptor(
            "driver.blender.introspect.summary",
            "Inspect Blender version and bounded RNA/operator/add-on counts",
            json!({"type":"object","additionalProperties":false}),
            json!({
                "type":"object",
                "properties":{
                    "version":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"integer","minimum":0,"maximum":99}},
                    "version_string":{"type":"string","maxLength":128},
                    "rna_types":{"type":"integer","minimum":0,"maximum":100000},
                    "operator_categories":{"type":"integer","minimum":0,"maximum":4096},
                    "operators":{"type":"integer","minimum":0,"maximum":100000},
                    "available_addons":{"type":"integer","minimum":0,"maximum":10000},
                    "enabled_addons":{"type":"integer","minimum":0,"maximum":10000},
                    "arbitrary_python":{"const":false},
                    "generic_operator_invoke":{"const":false}
                },
                "required":["version","version_string","rna_types","operator_categories","operators","available_addons","enabled_addons","arbitrary_python","generic_operator_invoke"],
                "additionalProperties":false
            }),
            &["rna", "operator", "addon"],
        ),
        descriptor(
            "driver.blender.introspect.operators",
            "Search Blender operators and report context availability without invoking them",
            query_schema(),
            json!({
                "type":"object",
                "properties":{
                    "items":{"type":"array","maxItems":256,"items":{
                        "type":"object",
                        "properties":{
                            "id":{"type":"string","maxLength":256},
                            "name":{"type":"string","maxLength":512},
                            "description":{"type":"string","maxLength":2048},
                            "available":{"type":"boolean"},
                            "properties":{"type":"integer","minimum":0,"maximum":4096}
                        },
                        "required":["id","name","description","available","properties"],
                        "additionalProperties":false
                    }},
                    "truncated":{"type":"boolean"}
                },
                "required":["items","truncated"],
                "additionalProperties":false
            }),
            &["operator"],
        ),
        descriptor(
            "driver.blender.introspect.operator.describe",
            "Describe one Blender operator RNA schema without invoking it",
            json!({
                "type":"object",
                "properties":{"id":{"type":"string","minLength":3,"maxLength":256,"pattern":"^[a-z0-9_]+\\.[a-z0-9_]+$"}},
                "required":["id"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "id":{"type":"string","maxLength":256},
                    "name":{"type":"string","maxLength":512},
                    "description":{"type":"string","maxLength":2048},
                    "available":{"type":"boolean"},
                    "properties":{"type":"array","maxItems":128,"items":{"type":"object"}}
                },
                "required":["id","name","description","available","properties"],
                "additionalProperties":false
            }),
            &["operator"],
        ),
        descriptor(
            "driver.blender.introspect.types",
            "Search Blender RNA types and their bounded property counts",
            query_schema(),
            json!({
                "type":"object",
                "properties":{
                    "items":{"type":"array","maxItems":256,"items":{
                        "type":"object",
                        "properties":{
                            "identifier":{"type":"string","maxLength":256},
                            "name":{"type":"string","maxLength":512},
                            "description":{"type":"string","maxLength":2048},
                            "base":{"type":["string","null"],"maxLength":256},
                            "properties":{"type":"integer","minimum":0,"maximum":4096}
                        },
                        "required":["identifier","name","description","base","properties"],
                        "additionalProperties":false
                    }},
                    "truncated":{"type":"boolean"}
                },
                "required":["items","truncated"],
                "additionalProperties":false
            }),
            &["rna"],
        ),
        descriptor(
            "driver.blender.introspect.addons",
            "List bounded add-on modules visible to this sandboxed Blender runtime",
            query_schema(),
            json!({
                "type":"object",
                "properties":{
                    "items":{"type":"array","maxItems":256,"items":{
                        "type":"object",
                        "properties":{
                            "module":{"type":"string","maxLength":512},
                            "name":{"type":"string","maxLength":512},
                            "version":{"type":"string","maxLength":128},
                            "enabled":{"type":"boolean"}
                        },
                        "required":["module","name","version","enabled"],
                        "additionalProperties":false
                    }},
                    "truncated":{"type":"boolean"}
                },
                "required":["items","truncated"],
                "additionalProperties":false
            }),
            &["addon"],
        ),
    ]
}

fn capabilities() -> Result<Vec<Capability>> {
    let mut values = curated_capabilities()?;
    values.extend(introspection_capabilities());
    Ok(values)
}

fn capability(command: &str) -> Result<Capability> {
    capabilities()?
        .into_iter()
        .find(|capability| capability.descriptor.name == command)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, "Blender capability is not registered"))
}
fn io_step(step: &str, error: std::io::Error) -> Error {
    Error::new(
        ErrorCode::BackendFailed,
        format!("{step} failed ({:?})", error.kind()),
    )
}

fn private_runtime() -> Result<PathBuf> {
    let path = PathBuf::from(format!("/tmp/semwright-blender-{}", unique_id()));
    fs::create_dir(&path).map_err(|error| io_step("Blender runtime creation", error))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
        .map_err(|error| io_step("Blender runtime permissions", error))?;
    Ok(path)
}

fn stage_runtime(runtime: &Path) -> Result<(PathBuf, PathBuf)> {
    let package = runtime.join("semwright_blender_runtime");
    fs::create_dir(&package).map_err(|error| io_step("Blender package creation", error))?;
    fs::write(package.join("__init__.py"), b"")
        .map_err(|error| io_step("Blender package init", error))?;
    fs::write(package.join("commands.py"), COMMANDS_PY)
        .map_err(|error| io_step("Blender commands staging", error))?;
    fs::write(package.join("commands.json"), COMMANDS_JSON)
        .map_err(|error| io_step("Blender command schemas staging", error))?;
    fs::write(package.join("validation.py"), VALIDATION_PY)
        .map_err(|error| io_step("Blender validation staging", error))?;
    let bridge = runtime.join("bridge.py");
    fs::write(&bridge, BRIDGE_PY).map_err(|error| io_step("Blender bridge staging", error))?;
    Ok((bridge, runtime.join("bridge.sock")))
}

async fn request(socket: &Path, command: &str, args: Value, seconds: u64) -> Result<Value> {
    let mut stream = UnixStream::connect(socket)
        .await
        .map_err(|error| io_step("Blender driver socket connection", error))?;
    write_frame(&mut stream, &json!({"command":command,"args":args})).await?;
    let response: Value = timeout(Duration::from_secs(seconds), read_frame(&mut stream))
        .await
        .map_err(|_| Error::new(ErrorCode::Timeout, "Blender operation timed out"))??;
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        return response
            .get("data")
            .cloned()
            .filter(Value::is_object)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Blender driver result must be an object",
                )
            });
    }
    let code = match response.pointer("/error/code").and_then(Value::as_str) {
        Some("NotFound") => ErrorCode::NotFound,
        Some("Conflict") => ErrorCode::Conflict,
        Some("InvalidArgument") => ErrorCode::InvalidArgument,
        Some("PolicyDenied") => ErrorCode::PolicyDenied,
        Some("Unsupported") => ErrorCode::Unsupported,
        Some("Timeout") => ErrorCode::Timeout,
        _ => ErrorCode::BackendFailed,
    };
    Err(Error::new(code, "Blender rejected the typed operation"))
}
struct BlenderDriver {
    child: Child,
    runtime: PathBuf,
    socket: PathBuf,
    version: String,
}

impl BlenderDriver {
    async fn start() -> Result<Self> {
        let blender = blender_binary()?;
        let workspace = fs::metadata(WORKSPACE)
            .map_err(|_| Error::unavailable("Blender driver requires the workspace mount"))?;
        if !workspace.is_dir() {
            return Err(Error::unavailable(
                "Blender workspace mount is not a directory",
            ));
        }

        let runtime = private_runtime()?;
        let (bridge, socket) = stage_runtime(&runtime)?;

        let version_output = timeout(
            Duration::from_secs(5),
            Command::new(blender).arg("--version").output(),
        )
        .await
        .map_err(|_| Error::new(ErrorCode::Timeout, "Blender version probe timed out"))??;
        if !version_output.status.success() || version_output.stdout.len() > 4096 {
            return Err(Error::unavailable("Blender version probe failed"));
        }
        let version_text = String::from_utf8_lossy(&version_output.stdout);
        let version = version_text
            .lines()
            .next()
            .unwrap_or("Blender unknown")
            .trim()
            .chars()
            .take(128)
            .collect::<String>();

        let mut command = Command::new(blender);
        command
            .args([
                "--background",
                "--factory-startup",
                "--disable-autoexec",
                "--python",
            ])
            .arg(&bridge)
            .arg("--")
            .arg(&socket)
            .arg(WORKSPACE)
            .arg(&runtime)
            .env("PYTHONNOUSERSITE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            // Normal DriverProvider execution discards the driver's stderr at the host boundary.
            // Direct owner/CI probes may capture it, which preserves bounded Blender startup
            // diagnostics without contaminating the framed stdout protocol.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|error| io_step("Blender background launch", error))?;
        if let Some(mut stdout) = child.stdout.take() {
            tokio::spawn(async move {
                let _ = tokio::io::copy(&mut stdout, &mut tokio::io::sink()).await;
            });
        }
        for _ in 0..240 {
            if child
                .try_wait()
                .map_err(|error| io_step("Blender process status", error))?
                .is_some()
            {
                return Err(Error::unavailable(
                    "Sandbox-owned Blender exited during startup",
                ));
            }
            if socket.exists()
                && request(&socket, "driver.blender.introspect.summary", json!({}), 2)
                    .await
                    .is_ok()
            {
                return Ok(Self {
                    child,
                    runtime,
                    socket,
                    version,
                });
            }
            sleep(Duration::from_millis(50)).await;
        }
        let _ = child.kill().await;
        let _ = fs::remove_dir_all(&runtime);
        Err(Error::new(
            ErrorCode::Timeout,
            "Blender background bridge startup timed out",
        ))
    }
}

impl Drop for BlenderDriver {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        let _ = fs::remove_dir_all(&self.runtime);
    }
}

#[async_trait]
impl Driver for BlenderDriver {
    fn id(&self) -> &str {
        DRIVER_ID
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        capabilities()
    }

    async fn execute(&mut self, command: &str, digest: &str, args: Value) -> Result<Value> {
        let capability = capability(command)?;
        if descriptor_digest(&capability.descriptor)? != digest {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Blender capability descriptor changed",
            ));
        }
        request(
            &self.socket,
            command,
            args,
            (capability.descriptor.timeout_ms / 1000).saturating_add(2),
        )
        .await
    }

    async fn health(&mut self) -> Result<Value> {
        let summary = request(
            &self.socket,
            "driver.blender.introspect.summary",
            json!({}),
            5,
        )
        .await?;
        Ok(json!({
            "healthy":true,
            "runtime":self.version,
            "version":summary["version_string"],
            "arbitrary_python":false,
            "generic_operator_invoke":false
        }))
    }
}

#[tokio::main]
async fn main() {
    let result = match BlenderDriver::start().await {
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
    fn curated_commands_are_migrated_without_generic_python_escape() {
        let capabilities = capabilities().unwrap();
        let names = capabilities
            .iter()
            .map(|capability| capability.descriptor.name.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(names.contains("driver.blender.object.create"));
        assert!(names.contains("driver.blender.render"));
        assert!(names.contains("driver.blender.introspect.operator.describe"));
        assert!(!names.iter().any(|name| name.contains("python")));
        assert!(!names.iter().any(|name| name.ends_with("operator.invoke")));
        for capability in capabilities {
            assert_eq!(capability.descriptor.requires, [DRIVER_SCOPE]);
            assert_eq!(capability.descriptor.backends, [DRIVER_SCOPE]);
        }
    }

    #[test]
    fn catalog_declares_generic_artifact_outputs() {
        let capabilities = capabilities().unwrap();
        let saved = capabilities
            .iter()
            .find(|capability| capability.descriptor.name == "driver.blender.file.save")
            .unwrap();
        assert!(saved.tags.iter().any(|tag| tag == "artifact-out:model/3d"));
        let render = capabilities
            .iter()
            .find(|capability| capability.descriptor.name == "driver.blender.render")
            .unwrap();
        assert!(
            render
                .tags
                .iter()
                .any(|tag| tag == "artifact-out:image/raster")
        );
    }

    #[test]
    fn introspection_inputs_are_bounded() {
        for capability in introspection_capabilities() {
            let serialized = serde_json::to_string(&capability.descriptor.input_schema).unwrap();
            assert!(!serialized.contains("additionalProperties\":true"));
            assert!(capability.descriptor.risk == Risk::ReadOnly);
        }
    }

    #[test]
    fn staged_runtime_contains_all_embedded_support_files() {
        let runtime = tempfile::tempdir().unwrap();
        let (bridge, socket) = stage_runtime(runtime.path()).unwrap();
        let package = runtime.path().join("semwright_blender_runtime");

        for file in [
            "__init__.py",
            "commands.py",
            "commands.json",
            "validation.py",
        ] {
            assert!(package.join(file).is_file(), "missing staged file {file}");
        }
        assert_eq!(bridge, runtime.path().join("bridge.py"));
        assert_eq!(socket, runtime.path().join("bridge.sock"));

        let schemas: Value =
            serde_json::from_slice(&std::fs::read(package.join("commands.json")).unwrap()).unwrap();
        assert!(schemas.get("blender.status").is_some());
        assert!(schemas.get("blender.render").is_some());
    }
}
