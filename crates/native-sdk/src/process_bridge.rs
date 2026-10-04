//! Optional Host-mediated process binding for owner-pinned TypeScript apps.
//!
//! The app implements storage/transactions. This bridge only transports bounded
//! cooperation frames through an already isolated canonical Driver Host profile.
//! The secondary runtime is selected, pinned and mount-scoped by Driver Host;
//! no caller can choose an executable, source code, module, path or shell command.
use crate::cooperation::{
    CallContext, Completion, ObservationPage, ObservationProvider, OperationHandler,
    PrivatePublication, PublicationProvider, Query, RecoveryProvider, RecoveryRecord,
    RequestIdentity, ResourceVersion, validate_value,
};
use crate::{Error, ErrorCode, Result, Value, async_trait, driver_sdk};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path, sync::Arc, time::Duration};

#[path = "process_bridge_reply_wire.rs"]
mod reply_wire;

pub const BRIDGE_SCHEMA: &str = "semwright-native-app-bridge/1";
const MAX_BUNDLE: usize = 48 * 1024;
const MAX_INPUT: usize = 64 * 1024;
const MAX_REPLY: usize = 256 * 1024;

/// Installation-time configuration. Never accept this type as operation input.
#[derive(Debug, Clone)]
pub struct NodeBridgeConfig {
    pub tool: String,
    pub bundle_mount: String,
    pub bundle_file: String,
    pub bundle_sha256: String,
    pub data_mount: String,
    pub output_mount: Option<String>,
    pub timeout: Duration,
}
impl NodeBridgeConfig {
    pub fn validate(&self) -> Result<()> {
        for name in [&self.tool, &self.bundle_mount, &self.data_mount] {
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                || name.starts_with("semwright-internal-")
            {
                return Err(Error::invalid(
                    "Bridge requires canonical owner-grant names",
                ));
            }
        }
        if let Some(output) = &self.output_mount
            && (output.is_empty()
                || output.len() > 64
                || !output
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                || output.starts_with("semwright-internal-"))
        {
            return Err(Error::invalid("Bridge output grant is invalid"));
        }
        if self.bundle_file.is_empty()
            || self.bundle_file.len() > 128
            || !self
                .bundle_file
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || self.bundle_file.starts_with('.')
        {
            return Err(Error::invalid(
                "Bundle must be a single installation filename",
            ));
        }
        if self.bundle_sha256.len() != 64
            || !self
                .bundle_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::invalid("Bundle requires a pinned lowercase SHA-256"));
        }
        if self.timeout.is_zero() || self.timeout > Duration::from_secs(30) {
            return Err(Error::invalid(
                "Bridge timeout must be bounded to 30 seconds",
            ));
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct BridgeFrame<'a> {
    schema_version: &'static str,
    id: &'a str,
    method: &'a str,
    operation: Option<&'a str>,
    args: Value,
    expected: Option<&'a ResourceVersion>,
    runtime: RuntimePaths,
}
#[derive(Serialize)]
struct RuntimePaths {
    data_root: String,
    output_root: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgeReply {
    schema_version: String,
    id: String,
    ok: bool,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    error: Option<Error>,
}

/// Bridge configuration is immutable after driver construction. The pinned bundle
/// is re-read/hashed per invocation; only those exact bytes are executed on stdin.
pub struct NodeBridge {
    config: NodeBridgeConfig,
}
impl NodeBridge {
    pub fn new(config: NodeBridgeConfig) -> Result<Arc<Self>> {
        config.validate()?;
        Ok(Arc::new(Self { config }))
    }
    pub fn operation(self: &Arc<Self>, command: impl Into<String>) -> Arc<dyn OperationHandler> {
        Arc::new(BridgeOperation {
            bridge: Arc::clone(self),
            command: command.into(),
        })
    }
    /// Adapt the canonical private-publication interface to an explicit
    /// OperationContract. The descriptor remains the negotiated capability;
    /// this adapter only constructs the destination-bound provider request.
    pub fn publication_operation(self: &Arc<Self>) -> Arc<dyn OperationHandler> {
        Arc::new(BridgePublicationOperation {
            bridge: Arc::clone(self),
        })
    }
    pub fn bundle_sha256(&self) -> &str {
        &self.config.bundle_sha256
    }

    async fn call(
        &self,
        method: &str,
        operation: Option<&str>,
        args: Value,
        context: &CallContext,
    ) -> Result<Value> {
        context.check_cancelled()?;
        validate_value(&args)?;
        self.config.validate()?;
        let mutating = matches!(method, "invoke" | "publish");
        let host = context.driver_context()?;
        if host.runtime_tool_mode(&self.config.tool)? != driver_sdk::RuntimeToolMode::HostMediated {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "This bridge requires a Host-mediated sealed runtime tool profile",
            ));
        }
        let root = driver_sdk::workspace_mount(&self.config.bundle_mount)?;
        let bundle = read_pinned(
            &root.join(&self.config.bundle_file),
            &self.config.bundle_sha256,
        )?;
        let paths = RuntimePaths {
            data_root: utf8_path(&driver_sdk::workspace_mount(&self.config.data_mount)?)?,
            output_root: self
                .config
                .output_mount
                .as_ref()
                .map(|mount| driver_sdk::workspace_mount(mount).and_then(|path| utf8_path(&path)))
                .transpose()?,
        };
        let frame = BridgeFrame {
            schema_version: BRIDGE_SCHEMA,
            id: context.request_id(),
            method,
            operation,
            args,
            expected: context.expected(),
            runtime: paths,
        };
        let encoded = serde_json::to_string(&frame)?;
        // JSON.parse of a quoted JSON string avoids source interpolation and JS
        // object-literal special keys. The only executable bytes are owner-pinned.
        let argument = serde_json::to_string(&encoded)?;
        let mut stdin = bundle;
        stdin.extend_from_slice(
            format!("\nvoid module.exports.semwrightNativeBridgeMain(JSON.parse({argument}));\n")
                .as_bytes(),
        );
        if stdin.len() > MAX_INPUT {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Bundle and invocation exceed the canonical runtime stdin budget",
            ));
        }
        let output = host
            .execute_runtime_tool(
                &self.config.tool,
                vec![
                    "--disable-wasm-trap-handler".into(),
                    "--max-old-space-size=256".into(),
                    "--input-type=commonjs".into(),
                    "-".into(),
                ],
                stdin,
                self.config.timeout,
            )
            .await
            .map_err(|error| if mutating { error.uncertain() } else { error })?;
        if output.exit_code != 0 {
            let error = Error::new(
                ErrorCode::BackendFailed,
                "Application runtime exited unsuccessfully before a valid reply",
            );
            return Err(if mutating { error.uncertain() } else { error });
        }
        if output.stdout.is_empty() {
            let error = Error::new(
                ErrorCode::PluginProtocolError,
                "Application runtime exited successfully without a protocol reply",
            );
            return Err(if mutating { error.uncertain() } else { error });
        }
        if output.stdout.len() > MAX_REPLY {
            let error = Error::new(
                ErrorCode::ResourceExhausted,
                "Application runtime reply exceeded the canonical byte budget",
            );
            return Err(if mutating { error.uncertain() } else { error });
        }
        let reply: BridgeReply = reply_wire::parse(&output.stdout).map_err(|_| {
            Error::new(
                ErrorCode::PluginProtocolError,
                "Application returned malformed bridge JSON",
            )
            .uncertain()
        })?;
        if reply.schema_version != BRIDGE_SCHEMA
            || reply.id != context.request_id()
            || reply.ok != reply.data.is_some()
            || reply.ok == reply.error.is_some()
        {
            return Err(Error::new(
                ErrorCode::PluginProtocolError,
                "Application reply is not bound to its invocation",
            )
            .uncertain());
        }
        if let Some(error) = reply.error {
            return Err(error);
        }
        let data = reply.data.ok_or_else(|| {
            Error::new(ErrorCode::PluginProtocolError, "Application result missing").uncertain()
        })?;
        validate_value(&data).map_err(|error| if mutating { error.uncertain() } else { error })?;
        Ok(data)
    }
}
#[async_trait]
impl ObservationProvider for NodeBridge {
    async fn observe(&self, query: &Query, context: &CallContext) -> Result<ObservationPage> {
        query.validate()?;
        let page: ObservationPage = serde_json::from_value(
            self.call("observe", None, serde_json::to_value(query)?, context)
                .await?,
        )?;
        page.validate_for(query)?;
        Ok(page)
    }
}
#[async_trait]
impl PublicationProvider for NodeBridge {
    async fn publish(&self, candidate: &PrivatePublication, context: &CallContext) -> Completion {
        if let Err(error) = candidate.validate() {
            return Completion::NotApplied(error);
        }
        match self
            .call(
                "publish",
                None,
                match serde_json::to_value(candidate) {
                    Ok(value) => value,
                    Err(error) => return Completion::NotApplied(error.into()),
                },
                context,
            )
            .await
        {
            Ok(value) => Completion::Applied(value),
            Err(error) if error.outcome_known => Completion::NotApplied(error),
            Err(error) => Completion::Uncertain(error),
        }
    }
}
#[async_trait]
impl RecoveryProvider for NodeBridge {
    async fn lookup(
        &self,
        request: &RequestIdentity,
        context: &CallContext,
    ) -> Result<RecoveryRecord> {
        request.validate()?;
        let record: RecoveryRecord = serde_json::from_value(
            self.call("lookup", None, serde_json::to_value(request)?, context)
                .await?,
        )?;
        record.validate_for(request)?;
        Ok(record)
    }
}
struct BridgePublicationOperation {
    bridge: Arc<NodeBridge>,
}
#[async_trait]
impl OperationHandler for BridgePublicationOperation {
    async fn invoke(&self, args: Value, context: &CallContext) -> Completion {
        let build = || -> Result<PrivatePublication> {
            let expected = context.expected().cloned().ok_or_else(|| {
                Error::new(
                    ErrorCode::StaleReference,
                    "Private publication requires an observed destination",
                )
            })?;
            let request: RequestIdentity = serde_json::from_value(
                args.get("request")
                    .cloned()
                    .ok_or_else(|| Error::invalid("Publication request identity is required"))?,
            )?;
            let candidate_sha256 = args
                .pointer("/parameters/candidate_sha256")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("Publication candidate SHA-256 is required"))?
                .to_owned();
            let snapshot_id = args
                .pointer("/parameters/snapshot_id")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("Publication snapshot identity is required"))?;
            if snapshot_id != candidate_sha256 {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Publication candidate differs from reviewed snapshot identity",
                ));
            }
            let candidate = PrivatePublication {
                candidate_sha256,
                destination: expected,
                request,
            };
            candidate.validate()?;
            Ok(candidate)
        };
        match build() {
            Ok(candidate) => self.bridge.publish(&candidate, context).await,
            Err(error) => Completion::NotApplied(error),
        }
    }
}
struct BridgeOperation {
    bridge: Arc<NodeBridge>,
    command: String,
}
#[async_trait]
impl OperationHandler for BridgeOperation {
    async fn invoke(&self, args: Value, context: &CallContext) -> Completion {
        match self
            .bridge
            .call("invoke", Some(&self.command), args, context)
            .await
        {
            Ok(value) => Completion::Applied(value),
            Err(error) if error.outcome_known => Completion::NotApplied(error),
            Err(error) => Completion::Uncertain(error),
        }
    }
}

fn utf8_path(path: &Path) -> Result<String> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        Error::new(
            ErrorCode::Unsupported,
            "This JSON process binding requires UTF-8 owner paths",
        )
    })
}
fn read_pinned(path: &Path, expected: &str) -> Result<Vec<u8>> {
    #[cfg(target_os = "linux")]
    {
        let fd = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NOFOLLOW,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| {
            Error::new(
                ErrorCode::PermissionDenied,
                "Pinned application bundle is unavailable",
            )
        })?;
        let file = std::fs::File::from(fd);
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_BUNDLE as u64 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Pinned bundle type or byte budget",
            ));
        }
        let mut bytes = Vec::new();
        file.take((MAX_BUNDLE + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_BUNDLE
            || std::str::from_utf8(&bytes).is_err()
            || hex::encode(Sha256::digest(&bytes)) != expected
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Application bundle differs from its owner-pinned bytes",
            ));
        }
        Ok(bytes)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, expected);
        Err(Error::new(
            ErrorCode::Unsupported,
            "Pinned materialized bridge is currently a Linux Host profile",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;
    fn config() -> NodeBridgeConfig {
        NodeBridgeConfig {
            tool: "node".into(),
            bundle_mount: "native-runtime".into(),
            bundle_file: "inventory.cjs".into(),
            bundle_sha256: "a".repeat(64),
            data_mount: "inventory-data".into(),
            output_mount: None,
            timeout: Duration::from_secs(3),
        }
    }
    #[test]
    fn installation_configuration_is_bounded_and_not_an_operation() {
        config().validate().unwrap();
        let mut bad = config();
        bad.bundle_file = "../outside.cjs".into();
        assert!(bad.validate().is_err());
        let mut bad = config();
        bad.bundle_sha256 = "A".repeat(64);
        assert!(bad.validate().is_err());
        let mut bad = config();
        bad.timeout = Duration::from_secs(31);
        assert!(bad.validate().is_err());
    }
    #[test]
    fn bridge_protocol_rejects_unknown_authority_fields() {
        assert!(
            serde_json::from_value::<BridgeReply>(
                json!({"schema_version":BRIDGE_SCHEMA,"id":"r","ok":true,"data":{},"approved":true})
            )
            .is_err()
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn pinned_bundle_reads_exact_opened_bytes_and_refuses_links() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("app.cjs");
        std::fs::write(&file, b"module.exports = {};\n").unwrap();
        let bytes = std::fs::read(&file).unwrap();
        let hash = hex::encode(Sha256::digest(&bytes));
        assert_eq!(read_pinned(&file, &hash).unwrap(), bytes);
        assert!(read_pinned(&file, &"0".repeat(64)).is_err());
        let link = dir.path().join("link.cjs");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(read_pinned(&link, &hash).is_err());
    }
}
