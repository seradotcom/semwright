use async_trait::async_trait;
use semwright_driver_sdk::{
    Capability, Driver, DriverExecutionContext, DriverInterfaces, RuntimeToolArg,
    artifact_input_tag, artifact_output_tag, serve, workspace_mount,
};
use semwright_mlt_video::{
    app::App,
    catalog,
    fs::PrivateDir,
    jobs::MAX_MEDIA_BYTES,
    runtime::{MediaInfo, ServiceCatalog},
};
use semwright_types::{Error, ErrorCode, Result};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
};

struct MltVideoDriver {
    app: App,
    host_catalog_verified: bool,
}

fn map_error(error: semwright_mlt_video::Error) -> Error {
    let code = match error.code {
        "Unsupported" => ErrorCode::Unsupported,
        "Unavailable" => ErrorCode::Unavailable,
        "PermissionDenied" => ErrorCode::PermissionDenied,
        "ConsentRequired" => ErrorCode::ConsentRequired,
        "PolicyDenied" => ErrorCode::PolicyDenied,
        "NotFound" => ErrorCode::NotFound,
        "AmbiguousTarget" => ErrorCode::AmbiguousTarget,
        "StaleReference" => ErrorCode::StaleReference,
        "Timeout" => ErrorCode::Timeout,
        "InvalidArgument" => ErrorCode::InvalidArgument,
        "SandboxDenied" => ErrorCode::SandboxDenied,
        "Conflict" => ErrorCode::Conflict,
        "Cancelled" => ErrorCode::Cancelled,
        "ProtocolMismatch" => ErrorCode::ProtocolMismatch,
        "ResourceExhausted" => ErrorCode::ResourceExhausted,
        _ => ErrorCode::BackendFailed,
    };
    let mut mapped = Error::new(code, error.message);
    mapped.outcome_known = error.outcome_known;
    mapped
}

fn from_internal(value: semwright_mlt_video::json::Value) -> Result<Value> {
    serde_json::from_str(&value.encode()).map_err(|_| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT driver produced invalid JSON",
        )
    })
}

fn to_internal(value: &Value) -> Result<semwright_mlt_video::json::Value> {
    let bytes = serde_json::to_vec(value)?;
    semwright_mlt_video::json::parse(&bytes).map_err(map_error)
}

fn sdk_capabilities() -> Result<Vec<Capability>> {
    catalog::capabilities()
        .map_err(map_error)?
        .into_iter()
        .map(|capability| {
            let mut capability: Capability =
                serde_json::from_str(&capability.wire).map_err(|_| {
                    Error::new(
                        ErrorCode::PluginProtocolError,
                        "MLT capability catalog is incompatible with the Driver SDK",
                    )
                })?;
            match capability.descriptor.name.as_str() {
                "driver.mlt-video.asset.import" => {
                    capability.tags.push(artifact_input_tag("video/clip")?);
                    capability.tags.push(artifact_input_tag("audio/sample")?);
                    capability.tags.push(artifact_input_tag("image/raster")?);
                }
                "driver.mlt-video.render.result" => {
                    capability.tags.push(artifact_output_tag("video/clip")?);
                }
                _ => {}
            }
            Ok(capability)
        })
        .collect()
}

fn host_runner_failure(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let object = value.as_object()?;
    if object.len() != 3
        || object.get("schema").and_then(Value::as_u64) != Some(1)
        || object.get("operation").and_then(Value::as_str) != Some("error")
    {
        return None;
    }
    object
        .get("error")
        .and_then(Value::as_str)
        .filter(|message| {
            !message.is_empty() && message.len() <= 1024 && !message.chars().any(char::is_control)
        })
        .map(ToOwned::to_owned)
}

fn host_media_info(bytes: &[u8]) -> Result<MediaInfo> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner returned invalid probe JSON",
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner probe result must be an object",
        )
    })?;
    if object.len() != 3
        || object.get("schema").and_then(Value::as_u64) != Some(1)
        || object.get("operation").and_then(Value::as_str) != Some("probe")
    {
        return Err(Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner probe envelope is invalid",
        ));
    }
    let media = object.get("media").ok_or_else(|| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner probe omitted media",
        )
    })?;
    let encoded = serde_json::to_vec(media)?;
    MediaInfo::parse(&encoded).map_err(map_error)
}

fn host_catalog(bytes: &[u8]) -> Result<ServiceCatalog> {
    const GROUPS: [&str; 6] = [
        "producers",
        "filters",
        "transitions",
        "consumers",
        "video_codecs",
        "audio_codecs",
    ];
    let value: Value = serde_json::from_slice(bytes).map_err(|_| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner returned invalid JSON",
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner result must be an object",
        )
    })?;
    if object.len() != 4
        || object.get("schema").and_then(Value::as_u64) != Some(1)
        || object.get("operation").and_then(Value::as_str) != Some("discover")
    {
        return Err(Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner discovery envelope is invalid",
        ));
    }
    let version = object
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| !version.is_empty() && version.len() <= 512)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::PluginProtocolError,
                "MLT runtime runner version is invalid",
            )
        })?
        .to_owned();
    let raw_groups = object
        .get("groups")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::PluginProtocolError,
                "MLT runtime runner groups are invalid",
            )
        })?;
    if raw_groups.len() != GROUPS.len()
        || GROUPS.iter().any(|group| !raw_groups.contains_key(*group))
    {
        return Err(Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner returned an unexpected service group set",
        ));
    }

    let mut groups = BTreeMap::new();
    for group in GROUPS {
        let values = raw_groups[group].as_array().ok_or_else(|| {
            Error::new(
                ErrorCode::PluginProtocolError,
                "MLT runtime runner service group is not an array",
            )
        })?;
        if values.len() > 2048 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "MLT runtime runner service group exceeds its bound",
            ));
        }
        let mut services = BTreeSet::new();
        for value in values {
            let service = value
                .as_str()
                .filter(|service| {
                    !service.is_empty()
                        && service.len() <= 128
                        && service.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'_' | b'-' | b'.' | b':')
                        })
                })
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::PluginProtocolError,
                        "MLT runtime runner service token is invalid",
                    )
                })?;
            if !services.insert(service.to_owned()) {
                return Err(Error::new(
                    ErrorCode::PluginProtocolError,
                    "MLT runtime runner returned a duplicate service",
                ));
            }
        }
        groups.insert(group.to_owned(), services);
    }
    Ok(ServiceCatalog { version, groups })
}

impl MltVideoDriver {
    async fn host_probe_for(
        &self,
        command: &str,
        args: &Value,
        context: &DriverExecutionContext,
    ) -> Result<Option<MediaInfo>> {
        let needs_probe = match command {
            "driver.mlt-video.asset.import" => {
                args.get("kind").and_then(Value::as_str) != Some("color")
            }
            "driver.mlt-video.asset.relink" => true,
            _ => false,
        };
        if !needs_probe {
            return Ok(None);
        }

        let root_name = args
            .get("root")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new(ErrorCode::InvalidArgument, "Media root is required"))?;
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new(ErrorCode::InvalidArgument, "Media path is required"))?;
        if !matches!(root_name, "project" | "media" | "output") {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Media root is not granted for probing",
            ));
        }
        let root = self.app.roots.get(root_name).ok_or_else(|| {
            Error::new(
                ErrorCode::PermissionDenied,
                "Media root is not mounted for probing",
            )
        })?;
        let mut source = root.read_file(path, MAX_MEDIA_BYTES).map_err(map_error)?;

        let scratch_root = workspace_mount("scratch")?;
        if !scratch_root.is_dir() {
            return Err(Error::new(
                ErrorCode::Unavailable,
                "MLT scratch mount is unavailable",
            ));
        }
        let scratch = PrivateDir::new(&scratch_root).map_err(map_error)?;
        let directory = scratch
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty() && value.len() <= 128)
            .ok_or_else(|| {
                Error::new(ErrorCode::Internal, "MLT scratch directory name is invalid")
            })?
            .to_owned();
        let mut destination = scratch.create("probe.bin").map_err(map_error)?;
        let copied = std::io::copy(
            &mut (&mut source).take(MAX_MEDIA_BYTES.saturating_add(1)),
            &mut destination,
        )
        .map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Failed to stage bounded MLT probe input",
            )
        })?;
        if copied > MAX_MEDIA_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "MLT probe input exceeds media byte budget",
            ));
        }
        destination.flush().map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Failed to flush bounded MLT probe input",
            )
        })?;
        destination.sync_all().map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Failed to sync bounded MLT probe input",
            )
        })?;
        drop(destination);
        scratch.seal("probe.bin").map_err(map_error)?;

        let output = context
            .execute_runtime_tool_args(
                "mlt-runner",
                vec![
                    RuntimeToolArg::Literal {
                        value: "probe".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--runtime-root".into(),
                    },
                    RuntimeToolArg::MountPath {
                        mount: "mlt-runtime".into(),
                        relative: String::new(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--ffprobe-sealed".into(),
                    },
                    RuntimeToolArg::ToolPath {
                        tool: "ffprobe".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--scratch-root".into(),
                    },
                    RuntimeToolArg::MountPath {
                        mount: "scratch".into(),
                        relative: String::new(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--directory".into(),
                    },
                    RuntimeToolArg::Literal { value: directory },
                    RuntimeToolArg::Literal {
                        value: "--name".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: "probe.bin".into(),
                    },
                ],
                Vec::new(),
                std::time::Duration::from_secs(30),
                None,
            )
            .await?;
        if output.exit_code != 0 {
            let detail = host_runner_failure(&output.stdout);
            return Err(Error::new(
                ErrorCode::BackendFailed,
                match detail {
                    Some(detail) => format!("Host-mediated MLT probe failed: {detail}"),
                    None => "Host-mediated MLT probe failed".into(),
                },
            ));
        }
        host_media_info(&output.stdout).map(Some)
    }

    async fn ensure_host_catalog(&mut self, context: &DriverExecutionContext) -> Result<()> {
        if self.host_catalog_verified || std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_none() {
            return Ok(());
        }
        let (legacy_version, legacy_groups) = self
            .app
            .runtime
            .as_ref()
            .map(|runtime| {
                (
                    runtime.catalog.version.clone(),
                    runtime.catalog.groups.clone(),
                )
            })
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::Unavailable,
                    "Host-mediated MLT discovery requires the legacy runtime during migration",
                )
            })?;
        let output = context
            .execute_runtime_tool_args(
                "mlt-runner",
                vec![
                    RuntimeToolArg::Literal {
                        value: "discover".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--runtime-root".into(),
                    },
                    RuntimeToolArg::MountPath {
                        mount: "mlt-runtime".into(),
                        relative: String::new(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--melt-sealed".into(),
                    },
                    RuntimeToolArg::ToolPath {
                        tool: "melt".into(),
                    },
                ],
                Vec::new(),
                std::time::Duration::from_secs(30),
                None,
            )
            .await?;
        if output.exit_code != 0 {
            let detail = host_runner_failure(&output.stdout);
            return Err(Error::new(
                ErrorCode::BackendFailed,
                match detail {
                    Some(detail) => format!("Host-mediated MLT discovery runner failed: {detail}"),
                    None => "Host-mediated MLT discovery runner failed".into(),
                },
            ));
        }
        let observed = host_catalog(&output.stdout)?;
        if observed.version != legacy_version || observed.groups != legacy_groups {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Host-mediated and legacy MLT discovery catalogs diverged",
            ));
        }
        self.app.catalog = Some(observed);
        self.app.runtime_reason =
            "Host-mediated MLT catalog verified against the transitional legacy runtime".into();
        self.host_catalog_verified = true;
        Ok(())
    }
}

#[async_trait]
impl Driver for MltVideoDriver {
    fn id(&self) -> &str {
        "mlt-video"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            health: true,
            host_tools: std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_some(),
            ..DriverInterfaces::default()
        }
    }

    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        sdk_capabilities()
    }

    async fn execute(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
    ) -> Result<Value> {
        let value = self
            .app
            .execute(command, descriptor_sha256, to_internal(&args)?)
            .map_err(map_error)?;
        from_internal(value)
    }

    async fn execute_with_context(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
        context: DriverExecutionContext,
    ) -> Result<Value> {
        context.check_cancelled()?;
        let internal = to_internal(&args)?;
        self.app
            .validate_call(command, descriptor_sha256, &internal)
            .map_err(map_error)?;
        self.ensure_host_catalog(&context).await?;
        let probe = self.host_probe_for(command, &args, &context).await?;
        let value = self
            .app
            .execute_with_probe(command, descriptor_sha256, internal, probe)
            .map_err(map_error)?;
        from_internal(value)
    }

    async fn health(&mut self) -> Result<Value> {
        from_internal(self.app.doctor())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let result = App::production()
        .map(|app| MltVideoDriver {
            app,
            host_catalog_verified: false,
        })
        .map_err(map_error);
    let result = match result {
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
    fn host_runner_failure_is_strict_and_bounded() {
        let valid = serde_json::json!({
            "schema": 1,
            "operation": "error",
            "error": "BackendFailed: melt -version discovery failed"
        });
        assert_eq!(
            host_runner_failure(&serde_json::to_vec(&valid).unwrap()).as_deref(),
            Some("BackendFailed: melt -version discovery failed")
        );

        let mut extra = valid.clone();
        extra["extra"] = serde_json::json!(true);
        assert!(host_runner_failure(&serde_json::to_vec(&extra).unwrap()).is_none());

        let control = serde_json::json!({
            "schema": 1,
            "operation": "error",
            "error": "bad\nmessage"
        });
        assert!(host_runner_failure(&serde_json::to_vec(&control).unwrap()).is_none());
    }

    #[test]
    fn host_catalog_parser_is_strict_and_bounded() {
        let valid = serde_json::json!({
            "schema": 1,
            "operation": "discover",
            "version": "melt 7.32.0",
            "groups": {
                "producers": ["avformat"],
                "filters": ["volume"],
                "transitions": ["mix"],
                "consumers": ["avformat"],
                "video_codecs": ["libx264"],
                "audio_codecs": ["aac"]
            }
        });
        let parsed = host_catalog(&serde_json::to_vec(&valid).unwrap()).unwrap();
        assert_eq!(parsed.version, "melt 7.32.0");
        assert!(parsed.has("filters", "volume"));

        let mut missing = valid.clone();
        missing["groups"].as_object_mut().unwrap().remove("filters");
        assert!(host_catalog(&serde_json::to_vec(&missing).unwrap()).is_err());

        let mut duplicate = valid;
        duplicate["groups"]["filters"] = serde_json::json!(["volume", "volume"]);
        assert!(host_catalog(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    }

    #[test]
    fn sdk_catalog_declares_generic_artifact_ports() {
        let caps = sdk_capabilities().unwrap();
        let import = caps
            .iter()
            .find(|cap| cap.descriptor.name == "driver.mlt-video.asset.import")
            .unwrap();
        assert!(
            import
                .tags
                .iter()
                .any(|tag| tag == "artifact-in:video/clip")
        );
        assert!(
            import
                .tags
                .iter()
                .any(|tag| tag == "artifact-in:audio/sample")
        );
        assert!(
            import
                .tags
                .iter()
                .any(|tag| tag == "artifact-in:image/raster")
        );

        let render = caps
            .iter()
            .find(|cap| cap.descriptor.name == "driver.mlt-video.render.result")
            .unwrap();
        assert!(
            render
                .tags
                .iter()
                .any(|tag| tag == "artifact-out:video/clip")
        );
    }
}
