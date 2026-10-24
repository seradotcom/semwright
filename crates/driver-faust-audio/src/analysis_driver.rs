use crate::analysis_runtime::AnalysisRuntime;
use async_trait::async_trait;
use semwright_driver_sdk::{
    Capability, Driver, DriverExecutionContext, DriverInterfaces, descriptor_digest,
};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk};
use serde_json::{Value, json};

pub const DRIVER_ID: &str = "audio-analysis";
pub const DRIVER_SCOPE: &str = "driver:audio-analysis";

pub struct AudioAnalysisDriver {
    runtime: Option<AnalysisRuntime>,
    capabilities: Vec<Capability>,
    alignment_candidate: bool,
}
impl AudioAnalysisDriver {
    pub fn production() -> Result<Self> {
        Ok(Self {
            runtime: AnalysisRuntime::load()?,
            capabilities: capability_catalog(),
            alignment_candidate: false,
        })
    }
    /// Candidate producer for an explicitly selected owner profile.
    /// This constructor neither installs nor admits that profile.
    pub fn production_alignment_candidate() -> Result<Self> {
        let mut driver = Self::production()?;
        driver.alignment_candidate = true;
        if let Some(runtime) = driver.runtime.as_mut() {
            runtime.enable_host_snapshot_meter();
        }
        driver
            .capabilities
            .push(crate::alignment_runtime::capability());
        Ok(driver)
    }

    fn verify_digest(&self, command: &str, digest: &str) -> Result<()> {
        let capability = self
            .capabilities
            .iter()
            .find(|capability| capability.descriptor.name == command)
            .ok_or_else(|| {
                Error::new(ErrorCode::NotFound, "Audio analysis capability is absent")
            })?;
        if descriptor_digest(&capability.descriptor)? != digest {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Pinned audio analysis descriptor digest mismatch",
            ));
        }
        Ok(())
    }
    async fn execute_inner(
        &self,
        command: &str,
        args: &Value,
        context: Option<&DriverExecutionContext>,
    ) -> Result<Value> {
        match command {
            crate::alignment_candidate::COMMAND if self.alignment_candidate => {
                let context = context.ok_or_else(|| {
                    Error::new(
                        ErrorCode::Unsupported,
                        "Alignment requires an authenticated Driver execution context",
                    )
                })?;
                let runtime = self.runtime.as_ref().ok_or_else(|| {
                    Error::new(
                        ErrorCode::Unavailable,
                        "Pinned audio alignment candidate runtime is unavailable",
                    )
                })?;
                crate::alignment_runtime::align(runtime, args, context).await
            }
            "driver.audio-analysis.doctor" => Ok(json!({
                "runtime_available": self.runtime.is_some(),
                "meter": "libebur128",
                "meter_version": "1.2.6",
                "network": false,
                "physical_audio_io": false
            })),
            "driver.audio-analysis.artifact.measure" => {
                let runtime = self.runtime.as_ref().ok_or_else(|| {
                    Error::new(
                        ErrorCode::Unavailable,
                        "Pinned audio analysis runtime is unavailable",
                    )
                })?;
                let file_name = text(args, "file_name", 200)?;
                let expected_sha256 = text(args, "expected_sha256", 64)?;
                let layout = text(args, "layout", 16)?;
                let receipt = runtime
                    .measure(context, file_name, expected_sha256, layout)
                    .await?;
                Ok(json!({
                    "artifact": {
                        "file_name": receipt.file_name,
                        "sha256": receipt.sha256,
                        "bytes": receipt.bytes,
                        "format": receipt.format
                    },
                    "loudness": receipt.loudness,
                    "pcm_statistics": receipt.statistics,
                    "exhaustive": true
                }))
            }
            _ => Err(Error::new(
                ErrorCode::NotFound,
                "Audio analysis capability is absent",
            )),
        }
    }
}
#[async_trait]
impl Driver for AudioAnalysisDriver {
    fn id(&self) -> &str {
        DRIVER_ID
    }
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }
    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            cooperative_cancellation: true,
            host_tools: std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_some(),
            health: true,
            ..Default::default()
        }
    }
    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        Ok(self.capabilities.clone())
    }
    async fn execute(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
    ) -> Result<Value> {
        self.verify_digest(command, descriptor_sha256)?;
        self.execute_inner(command, &args, None).await
    }
    async fn execute_with_context(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
        context: DriverExecutionContext,
    ) -> Result<Value> {
        self.verify_digest(command, descriptor_sha256)?;
        context.check_cancelled()?;
        self.execute_inner(command, &args, Some(&context)).await
    }
    async fn health(&mut self) -> Result<Value> {
        Ok(json!({
            "healthy": self.runtime.is_some(),
            "runtime_available": self.runtime.is_some()
        }))
    }
}

pub fn capability_catalog() -> Vec<Capability> {
    vec![
        capability(
            "driver.audio-analysis.doctor",
            "Inspect the fixed offline audio meter runtime",
            json!({"type":"object","properties":{},"additionalProperties":false}),
            json!({
                "type":"object",
                "properties":{
                    "runtime_available":{"type":"boolean"},
                    "meter":{"const":"libebur128"},
                    "meter_version":{"const":"1.2.6"},
                    "network":{"const":false},
                    "physical_audio_io":{"const":false}
                },
                "required":["runtime_available","meter","meter_version","network","physical_audio_io"],
                "additionalProperties":false
            }),
            &["audio-analysis"],
        ),
        capability(
            "driver.audio-analysis.artifact.measure",
            "Measure a digest-pinned WAV or FLAC snapshot with libebur128; WAV also receives an independent bounded PCM decode",
            json!({
                "type":"object",
                "properties":{
                    "file_name":{"type":"string","minLength":1,"maxLength":200,"pattern":"^[A-Za-z0-9][A-Za-z0-9._-]*\\.(wav|flac)$"},
                    "expected_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                    "layout":{"type":"string","enum":["mono","stereo"]}
                },
                "required":["file_name","expected_sha256","layout"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "artifact":{
                        "type":"object",
                        "properties":{
                            "file_name":{"type":"string"},
                            "sha256":{"type":"string"},
                            "bytes":{"type":"integer","minimum":1},
                            "format":{"enum":["wav","flac"]}
                        },
                        "required":["file_name","sha256","bytes","format"],
                        "additionalProperties":false
                    },
                    "loudness":{"type":"object"},
                    "pcm_statistics":{"type":["object","null"]},
                    "exhaustive":{"const":true}
                },
                "required":["artifact","loudness","pcm_statistics","exhaustive"],
                "additionalProperties":false
            }),
            &["audio-artifact", "audio-analysis"],
        ),
    ]
}
fn capability(
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
            timeout_ms: 35_000,
            dry_run: true,
            interactive_consent: false,
            backends: vec![DRIVER_SCOPE.into()],
        },
        aliases: vec![],
        tags: vec![
            "audio".into(),
            "analysis".into(),
            "artifact-in:audio/wav".into(),
            "artifact-in:audio/flac".into(),
        ],
        object_types: object_types.iter().map(|value| (*value).into()).collect(),
    }
}
fn text<'a>(args: &'a Value, key: &str, max: usize) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
        })
        .ok_or_else(|| Error::invalid(format!("Invalid audio analysis {key}")))
}

#[cfg(test)]
mod alignment_route_tests {
    use super::*;

    #[test]
    fn ordinary_analysis_catalog_does_not_advertise_alignment() {
        let capabilities = capability_catalog();
        let names: Vec<_> = capabilities
            .iter()
            .map(|capability| capability.descriptor.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "driver.audio-analysis.doctor",
                "driver.audio-analysis.artifact.measure",
            ]
        );
    }

    #[tokio::test]
    async fn candidate_route_requires_context_and_is_absent_from_the_ordinary_driver() {
        let mut capabilities = capability_catalog();
        capabilities.push(crate::alignment_runtime::capability());
        let driver = AudioAnalysisDriver {
            runtime: None,
            capabilities,
            alignment_candidate: true,
        };
        let error = driver
            .execute_inner(crate::alignment_candidate::COMMAND, &json!({}), None)
            .await
            .expect_err("context is required before runtime or request decoding");
        assert_eq!(error.code, ErrorCode::Unsupported);
        let ordinary = AudioAnalysisDriver {
            runtime: None,
            capabilities: capability_catalog(),
            alignment_candidate: false,
        };
        let error = ordinary
            .execute_inner(crate::alignment_candidate::COMMAND, &json!({}), None)
            .await
            .expect_err("ordinary driver does not expose this route");
        assert_eq!(error.code, ErrorCode::NotFound);
    }
}
