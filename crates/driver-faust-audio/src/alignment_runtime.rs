//! Proposed ACC007 producer. Installing/admitting its profile is a separate owner action.
use crate::{
    alignment_candidate::{self, ValidatedRequest},
    analysis_driver::DRIVER_SCOPE,
    analysis_runtime::AnalysisRuntime,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use semwright_driver_sdk::{Capability, DriverExecutionContext, RuntimeToolArg};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

const TOOL_NAME: &str = "audio-alignment-launcher";
const TOOL_SECONDS: u64 = 180;
const MAX_WIRE_STDERR: usize = 1_024;
const MAX_CARRIER_JSON: usize = 350_000;

fn literal(value: impl Into<String>) -> RuntimeToolArg {
    RuntimeToolArg::Literal {
        value: value.into(),
    }
}
fn mount(name: &str) -> RuntimeToolArg {
    RuntimeToolArg::MountPath {
        mount: name.into(),
        relative: String::new(),
    }
}
fn job_args(request: &ValidatedRequest) -> Vec<RuntimeToolArg> {
    vec![
        literal("-I"),
        literal("-B"),
        mount("alignment-profile"),
        literal("--input-root"),
        mount("analysis-input"),
        literal("--work-root"),
        mount("analysis-work"),
        literal("--checkpoint-root"),
        mount("alignment-checkpoint"),
        literal("--file-name"),
        literal(&request.file_name),
        literal("--request-id"),
        literal(request.id()),
    ]
}
fn retained(mut error: Error, request_id: &str) -> Error {
    error.message.push_str(&format!(
        "; alignment attempt {request_id} must be retained and inspected; do not resubmit"
    ));
    error
}

pub(crate) async fn align(
    runtime: &AnalysisRuntime,
    args: &Value,
    context: &DriverExecutionContext,
) -> Result<Value> {
    let request = ValidatedRequest::parse(args)?;
    context.check_cancelled()?;
    let (temporary, staged, observed_sha, observed_bytes) =
        runtime.snapshot_alignment(&request.file_name)?;
    if observed_sha != request.source_sha() || observed_bytes != request.source_bytes() {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Alignment preflight snapshot differs from the bound source",
        ));
    }
    let observation = alignment_candidate::observe_pcm(&staged, &request)?;
    drop(temporary);
    context.check_cancelled()?;
    let job = context
        .start_runtime_tool_job_args(
            TOOL_NAME,
            job_args(&request),
            request.bytes.clone(),
            Duration::from_secs(TOOL_SECONDS),
            None,
        )
        .await
        .map_err(|error| retained(error.uncertain(), request.id()))?;
    let output = context
        .wait_runtime_tool_job(&job)
        .await
        .map_err(|error| retained(error, request.id()))?;
    if output.stdout.len() > alignment_candidate::MAX_STDOUT
        || output.stderr.len() > MAX_WIRE_STDERR
    {
        return Err(retained(
            Error::new(
                ErrorCode::ResourceExhausted,
                "Alignment tool exceeded its bounded stdout/stderr contract",
            ),
            request.id(),
        ));
    }
    if output.exit_code != 0 {
        return Err(retained(
            Error::new(
                ErrorCode::BackendFailed,
                "Alignment candidate tool exited unsuccessfully",
            ),
            request.id(),
        ));
    }
    let envelope = alignment_candidate::validate_envelope(&output.stdout, &request, &observation)
        .map_err(|error| retained(error, request.id()))?;
    let carrier = json!({
        "runtime_output":{
            "encoding":"base64",
            "data":STANDARD.encode(&output.stdout),
            "sha256":hex::encode(Sha256::digest(&output.stdout)),
            "size_bytes":output.stdout.len()
        },
        "runtime_job":{
            "id":job.id,
            "status":"succeeded",
            "tool_name":TOOL_NAME,
            "request_id":request.id(),
            "request_sha256":request.sha256,
            "candidate_sha256":envelope["candidate_file"]["sha256"],
            "execution_sha256":envelope["execution_file"]["sha256"]
        }
    });
    if serde_json::to_vec(&carrier)?.len() > MAX_CARRIER_JSON {
        return Err(retained(
            Error::new(
                ErrorCode::ResourceExhausted,
                "alignment carrier exceeded its Driver response budget",
            ),
            request.id(),
        ));
    }
    Ok(carrier)
}

fn uuid_schema() -> Value {
    json!({"type":"string","pattern":"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"})
}
fn digest_schema() -> Value {
    json!({"type":"string","pattern":"^[0-9a-f]{64}$"})
}
fn input_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "file_name":{"type":"string","minLength":1,"maxLength":200,
                "pattern":"^[A-Za-z0-9_][A-Za-z0-9._-]*[.]wav$"},
            "request":{
                "type":"object",
                "properties":{
                    "schema":{"const":"motionwright.alignment-request/1"},
                    "request_id":uuid_schema(),
                    "project_id":uuid_schema(),
                    "project_generation":uuid_schema(),
                    "project_revision":{"type":"integer","minimum":0,"maximum":9_007_199_254_740_991u64},
                    "voice_track_id":uuid_schema(),
                    "asset_id":uuid_schema(),
                    "source_commit":{"type":"string","pattern":"^[0-9a-f]{40}$"},
                    "language":{"const":"en"},
                    "transcript":{
                        "type":"object",
                        "properties":{
                            "version":{"type":"integer","minimum":1,"maximum":9_007_199_254_740_991u64},
                            "text":{"type":"string","minLength":1,"maxLength":256,
                                "pattern":"^[A-Za-z]+(?:'[A-Za-z]+)*(?: [A-Za-z]+(?:'[A-Za-z]+)*)*$"},
                            "sha256":digest_schema()
                        },
                        "required":["version","text","sha256"],
                        "additionalProperties":false
                    },
                    "source":{
                        "type":"object",
                        "properties":{
                            "sha256":digest_schema(),
                            "size_bytes":{"type":"integer","minimum":1,"maximum":alignment_candidate::MAX_WAV},
                            "sample_rate_hz":{"const":16000},
                            "channels":{"const":1},
                            "sample_frames":{"type":"integer","minimum":400,"maximum":128000}
                        },
                        "required":["sha256","size_bytes","sample_rate_hz","channels","sample_frames"],
                        "additionalProperties":false
                    },
                    "runtime":{"const":alignment_candidate::runtime_pins()}
                },
                "required":["schema","request_id","project_id","project_generation",
                    "project_revision","voice_track_id","asset_id","source_commit",
                    "language","transcript","source","runtime"],
                "additionalProperties":false
            }
        },
        "required":["file_name","request"],
        "additionalProperties":false
    })
}
fn output_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "runtime_output":{
                "type":"object",
                "properties":{
                    "encoding":{"const":"base64"},
                    "data":{"type":"string","minLength":4,"maxLength":327680},
                    "sha256":digest_schema(),
                    "size_bytes":{"type":"integer","minimum":1,"maximum":alignment_candidate::MAX_STDOUT}
                },
                "required":["encoding","data","sha256","size_bytes"],
                "additionalProperties":false
            },
            "runtime_job":{
                "type":"object",
                "properties":{
                    "id":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9-]+$"},
                    "status":{"const":"succeeded"},
                    "tool_name":{"const":TOOL_NAME},
                    "request_id":uuid_schema(),
                    "request_sha256":digest_schema(),
                    "candidate_sha256":digest_schema(),
                    "execution_sha256":digest_schema()
                },
                "required":["id","status","tool_name","request_id","request_sha256",
                    "candidate_sha256","execution_sha256"],
                "additionalProperties":false
            }
        },
        "required":["runtime_output","runtime_job"],
        "additionalProperties":false
    })
}

pub(crate) fn capability() -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: alignment_candidate::COMMAND.into(),
            version: "1".into(),
            description: "Produce a bounded offline English CTC alignment candidate with original-byte evidence; qualification and project mutation are separate".into(),
            input_schema: input_schema(),
            output_schema: output_schema(),
            requires: vec![DRIVER_SCOPE.into()],
            risk: Risk::MutatingReversible,
            idempotency: Idempotency::NonIdempotent,
            timeout_ms: 185_000,
            dry_run: false,
            interactive_consent: false,
            backends: vec![DRIVER_SCOPE.into()],
        },
        aliases: vec![],
        tags: vec!["audio".into(), "analysis".into(), "artifact-in:audio/wav".into()],
        object_types: vec!["audio-artifact".into(), "alignment-candidate".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semwright_driver_sdk::{
        Request, Response, RuntimeToolJob, RuntimeToolJobStatus, ToolExecutionOutput,
    };

    #[test]
    fn alignment_descriptor_is_closed_non_retryable_and_separate_from_meter() {
        let capability = capability();
        assert_eq!(capability.descriptor.name, alignment_candidate::COMMAND);
        assert_eq!(capability.descriptor.risk, Risk::MutatingReversible);
        assert_eq!(
            capability.descriptor.idempotency,
            Idempotency::NonIdempotent
        );
        assert_eq!(capability.descriptor.timeout_ms, 185_000);
        assert!(!capability.descriptor.dry_run);
        assert_eq!(
            capability.descriptor.input_schema["additionalProperties"],
            false
        );
        assert_eq!(
            capability.descriptor.output_schema["additionalProperties"],
            false
        );
        let digest = semwright_driver_sdk::descriptor_digest(&capability.descriptor)
            .expect("descriptor digest");
        assert_eq!(digest.len(), 64);
        println!("ACC007_DESCRIPTOR_SHA256={digest}");
    }

    #[test]
    fn maximum_original_stdout_carrier_fits_actual_driver_response_frame() {
        let original = vec![255; alignment_candidate::MAX_STDOUT];
        let value = json!({
            "runtime_output":{
                "encoding":"base64","data":STANDARD.encode(&original),
                "sha256":hex::encode(Sha256::digest(&original)),
                "size_bytes":original.len()
            },
            "runtime_job":{
                "id":"j".repeat(128),"status":"succeeded","tool_name":TOOL_NAME,
                "request_id":"12345678-1234-1234-1234-123456789abc",
                "request_sha256":"a".repeat(64),
                "candidate_sha256":"b".repeat(64),
                "execution_sha256":"c".repeat(64)
            }
        });
        assert!(serde_json::to_vec(&value).unwrap().len() <= MAX_CARRIER_JSON);
        let response = Response::Result {
            id: "r".repeat(128),
            value: value.clone(),
        };
        let frame = semwright_protocol::encode(&response).expect("bounded real Response frame");
        assert!(frame.len() <= semwright_types::MAX_FRAME + 4);
        let decoded: Response = semwright_protocol::decode(&frame).expect("real frame decode");
        match decoded {
            Response::Result {
                value: returned, ..
            } => assert_eq!(returned, value),
            _ => panic!("wrong decoded response variant"),
        }
    }

    #[test]
    fn maximum_tool_vectors_fit_actual_job_state_only_with_bounded_wire_stderr() {
        let output = ToolExecutionOutput {
            exit_code: 0,
            stdout: vec![255; alignment_candidate::MAX_STDOUT],
            stderr: vec![255; MAX_WIRE_STDERR],
        };
        output.validate().expect("per-stream tool limits");
        let request = Request::ToolJobState {
            id: "s".repeat(128),
            job: RuntimeToolJob {
                id: "j".repeat(128),
            },
            status: RuntimeToolJobStatus::Succeeded {
                output: output.clone(),
            },
        };
        let frame =
            semwright_protocol::encode(&request).expect("bounded real Host job-state frame");
        assert!(frame.len() <= semwright_types::MAX_FRAME + 4);
        let decoded: Request = semwright_protocol::decode(&frame).expect("real frame decode");
        match decoded {
            Request::ToolJobState {
                status: RuntimeToolJobStatus::Succeeded { output: returned },
                ..
            } => {
                assert_eq!(returned, output);
            }
            _ => panic!("wrong decoded job-state variant"),
        }
    }

    #[test]
    fn per_stream_sdk_maximum_stderr_would_overflow_the_actual_job_frame() {
        let output = ToolExecutionOutput {
            exit_code: 0,
            stdout: vec![255; alignment_candidate::MAX_STDOUT],
            stderr: vec![255; 256 * 1024],
        };
        output
            .validate()
            .expect("each stream independently passes the SDK bound");
        let request = Request::ToolJobState {
            id: "s".repeat(128),
            job: RuntimeToolJob {
                id: "j".repeat(128),
            },
            status: RuntimeToolJobStatus::Succeeded { output },
        };
        assert!(semwright_protocol::encode(&request).is_err());
    }
}
