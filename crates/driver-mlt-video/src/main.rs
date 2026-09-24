use async_trait::async_trait;
use semwright_driver_sdk::{Capability, Driver, artifact_input_tag, artifact_output_tag, serve};
use semwright_mlt_video::{app::App, catalog};
use semwright_types::{Error, ErrorCode, Result};
use serde_json::Value;

struct MltVideoDriver {
    app: App,
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

#[async_trait]
impl Driver for MltVideoDriver {
    fn id(&self) -> &str {
        "mlt-video"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
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

    async fn health(&mut self) -> Result<Value> {
        from_internal(self.app.doctor())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let result = App::production()
        .map(|app| MltVideoDriver { app })
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
