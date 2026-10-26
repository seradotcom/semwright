//! Observed package versions via the existing sealed Motion Node Host tool.
//! This is package diagnosis, not proof of module loading, rendering or admission.
use crate::{
    Error, ErrorCode, Result,
    model::{MOTION_CANVAS_VERSION, NODE_VERSION},
};
use schemars::JsonSchema;
use semwright_driver_sdk::{DriverExecutionContext, RuntimeToolArg, RuntimeToolCwd};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const PROGRAM: &str = include_str!("runtime_probe.mjs");
const MAX_STDOUT: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProbeState {
    Observed,
    PackageNotFound,
    InvalidPackage,
    ScopeViolation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawObservation {
    state: ProbeState,
    node_version: String,
    core_version: Option<String>,
    two_d_version: Option<String>,
    vite_plugin_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeVersionProbe {
    schema_version: u32,
    operation: String,
    requested: bool,
    source: String,
    state: ProbeState,
    expected_motion_canvas_version: String,
    expected_node_version: String,
    observed_node_version: String,
    observed_core_version: Option<String>,
    observed_two_d_version: Option<String>,
    observed_vite_plugin_version: Option<String>,
    compatible: Option<bool>,
    reason_code: String,
    render_performed: bool,
    scene_mutation_performed: bool,
    browser_probe_performed: bool,
}

pub(super) async fn inspect(context: &DriverExecutionContext) -> Result<RuntimeVersionProbe> {
    context.check_cancelled()?;
    let args = [
        "--disable-wasm-trap-handler",
        "--max-old-space-size=512",
        "--input-type=module",
        "-",
    ]
    .into_iter()
    .map(|value| RuntimeToolArg::Literal {
        value: value.into(),
    })
    .collect();
    let output = context
        .execute_runtime_tool_args(
            "motion-node",
            args,
            PROGRAM.as_bytes().to_vec(),
            Duration::from_secs(30),
            Some(RuntimeToolCwd {
                mount: "runtime".into(),
                relative: String::new(),
            }),
        )
        .await?;
    if output.exit_code != 0 {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Motion Node package probe failed",
        ));
    }
    decode(&output.stdout)
}

fn protocol(message: &str) -> Error {
    Error::new(ErrorCode::ProtocolMismatch, message)
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'+' | b'_' | b'-'))
        })
}

fn decode(bytes: &[u8]) -> Result<RuntimeVersionProbe> {
    if bytes.len() > MAX_STDOUT {
        return Err(protocol(
            "Motion Node package observation exceeds the bound",
        ));
    }
    let shape: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| protocol("Motion Node package observation is malformed"))?;
    let expected = [
        "state",
        "node_version",
        "core_version",
        "two_d_version",
        "vite_plugin_version",
    ];
    let object = shape
        .as_object()
        .ok_or_else(|| protocol("Motion Node package observation must be an object"))?;
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(protocol(
            "Motion Node package observation has an incomplete or foreign shape",
        ));
    }
    let raw: RawObservation = serde_json::from_slice(bytes)
        .map_err(|_| protocol("Motion Node package observation is malformed"))?;
    if !valid_version(&raw.node_version) {
        return Err(protocol(
            "Motion Node version is not a bounded version string",
        ));
    }
    let packages = [
        raw.core_version.as_deref(),
        raw.two_d_version.as_deref(),
        raw.vite_plugin_version.as_deref(),
    ];
    let compatible = if raw.state == ProbeState::Observed {
        if packages
            .iter()
            .any(|value| !value.is_some_and(valid_version))
        {
            return Err(protocol(
                "Observed Motion Canvas packages require three valid versions",
            ));
        }
        Some(
            raw.node_version == NODE_VERSION
                && packages
                    .iter()
                    .all(|value| *value == Some(MOTION_CANVAS_VERSION)),
        )
    } else {
        if packages.iter().any(Option::is_some) {
            return Err(protocol(
                "Failed package probe cannot claim observed package versions",
            ));
        }
        None
    };
    let reason_code = match (raw.state, compatible) {
        (ProbeState::Observed, Some(true)) => "pinned_versions_observed",
        (ProbeState::Observed, Some(false)) => "observed_version_mismatch",
        (ProbeState::PackageNotFound, None) => "runtime_package_not_found",
        (ProbeState::InvalidPackage, None) => "runtime_package_invalid",
        (ProbeState::ScopeViolation, None) => "runtime_package_outside_mount",
        _ => return Err(protocol("Inconsistent runtime package observation")),
    };
    Ok(RuntimeVersionProbe {
        schema_version: 1,
        operation: "runtime_version_probe".into(),
        requested: true,
        source: "host_native_package_probe".into(),
        state: raw.state,
        expected_motion_canvas_version: MOTION_CANVAS_VERSION.into(),
        expected_node_version: NODE_VERSION.into(),
        observed_node_version: raw.node_version,
        observed_core_version: raw.core_version,
        observed_two_d_version: raw.two_d_version,
        observed_vite_plugin_version: raw.vite_plugin_version,
        compatible,
        reason_code: reason_code.into(),
        render_performed: false,
        scene_mutation_performed: false,
        browser_probe_performed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn observed() -> Value {
        json!({
            "state":"observed", "node_version":NODE_VERSION,
            "core_version":MOTION_CANVAS_VERSION,
            "two_d_version":MOTION_CANVAS_VERSION,
            "vite_plugin_version":MOTION_CANVAS_VERSION
        })
    }

    fn decode_value(value: &Value) -> Result<RuntimeVersionProbe> {
        decode(&serde_json::to_vec(value).unwrap())
    }

    #[test]
    fn actual_pinned_versions_have_separate_declared_and_observed_fields() {
        let report = decode_value(&observed()).unwrap();
        assert_eq!(report.compatible, Some(true));
        assert_eq!(
            report.observed_core_version.as_deref(),
            Some(MOTION_CANVAS_VERSION)
        );
        assert!(!report.render_performed);
        assert!(!report.scene_mutation_performed);
        assert!(!report.browser_probe_performed);
    }

    #[test]
    fn package_mismatch_is_observed_and_never_replaced_with_the_pin() {
        let mut value = observed();
        value["core_version"] = json!("3.18.0");
        let report = decode_value(&value).unwrap();
        assert_eq!(report.compatible, Some(false));
        assert_eq!(report.observed_core_version.as_deref(), Some("3.18.0"));
        assert_eq!(report.reason_code, "observed_version_mismatch");
    }

    #[test]
    fn actual_node_mismatch_disables_compatibility() {
        let mut value = observed();
        value["node_version"] = json!("24.1.0");
        let report = decode_value(&value).unwrap();
        assert_eq!(report.compatible, Some(false));
        assert_eq!(report.observed_node_version, "24.1.0");
    }

    #[test]
    fn each_failure_state_is_explicit_without_package_health() {
        for state in ["package_not_found", "invalid_package", "scope_violation"] {
            let value = json!({
                "state":state, "node_version":NODE_VERSION,
                "core_version":null, "two_d_version":null, "vite_plugin_version":null
            });
            let report = decode_value(&value).unwrap();
            assert_eq!(report.compatible, None);
            assert_eq!(report.observed_core_version, None);
        }
    }

    #[test]
    fn partial_and_forged_failed_observations_are_rejected() {
        let mut missing = observed();
        missing["two_d_version"] = Value::Null;
        assert!(decode_value(&missing).is_err());
        let mut omitted = observed();
        omitted.as_object_mut().unwrap().remove("two_d_version");
        assert!(decode_value(&omitted).is_err());
        let mut forged = observed();
        forged["state"] = json!("package_not_found");
        assert!(decode_value(&forged).is_err());
    }

    #[test]
    fn malformed_unknown_fields_and_duplicate_fields_are_rejected() {
        assert!(decode(b"not-json").is_err());
        let mut unknown = observed();
        unknown["render_performed"] = json!(true);
        assert!(decode_value(&unknown).is_err());
        assert!(decode(br#"{"state":"observed","node_version":"22.22.0","node_version":"24.0.0","core_version":"3.17.2","two_d_version":"3.17.2","vite_plugin_version":"3.17.2"}"#).is_err());
    }

    #[test]
    fn unbounded_control_unicode_and_invalid_version_strings_are_rejected() {
        for value in ["", "3.17.2\n", "3 17", "\u{00e9}", ".3"] {
            let mut raw = observed();
            raw["core_version"] = json!(value);
            assert!(decode_value(&raw).is_err());
        }
        let mut raw = observed();
        raw["node_version"] = json!("1".repeat(81));
        assert!(decode_value(&raw).is_err());
        assert!(decode(&vec![b' '; MAX_STDOUT + 1]).is_err());
    }
}
