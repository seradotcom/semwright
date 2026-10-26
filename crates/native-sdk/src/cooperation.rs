//! Optional application-owned cooperation contracts. No persistence or grants live here.
use crate::{CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk, Value, async_trait};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

pub const MAX_VALUE_BYTES: usize = 256 * 1024;
pub const MAX_PAGE_ITEMS: usize = 256;
pub const MAX_OPERATIONS: usize = 128;
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

fn bounded(value: &str, limit: usize) -> Result<()> {
    if value.is_empty() || value.len() > limit || value.chars().any(char::is_control) {
        return Err(Error::invalid("Expected a bounded nonempty identifier"));
    }
    Ok(())
}

/// Full application revision; never truncated to fit NativeTarget.revision.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct RevisionToken(String);
impl RevisionToken {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        bounded(&value, 512)?;
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for RevisionToken {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceVersion {
    pub resource: String,
    pub generation: String,
    pub revision: RevisionToken,
}
impl ResourceVersion {
    pub fn validate(&self) -> Result<()> {
        bounded(&self.resource, 512)?;
        bounded(&self.generation, 256)
    }
}

/// Advisory cancellation policy, not a promise to roll back a committed transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancellationSemantics {
    BeforeEffects,
    Cooperative,
    BestEffort,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitSemantics {
    ReadOnly,
    ApplicationTransaction,
    NonAtomic,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrySemantics {
    Never,
    StateIdempotent,
    DurableRequestKey,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum UndoSemantics {
    None,
    ApplicationOperation { command: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetRequirement {
    None,
    ObservedResource,
}

/// The descriptor is the canonical Broker descriptor, not a second risk vocabulary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationContract {
    pub descriptor: CommandDescriptor,
    pub target: TargetRequirement,
    pub commit: CommitSemantics,
    pub retry: RetrySemantics,
    pub undo: UndoSemantics,
    pub cancellation: CancellationSemantics,
    /// True only when comparison and commit happen in the app's same transaction.
    pub atomic_revision_cas: bool,
}
impl OperationContract {
    pub fn validate(&self, application: &str) -> Result<()> {
        let d = &self.descriptor;
        let prefix = format!("driver.{application}.");
        if !d.name.starts_with(&prefix) || d.name.len() <= prefix.len() || d.name.len() > 192 {
            return Err(Error::invalid(
                "Operation is outside its application namespace",
            ));
        }
        let provider = format!("driver:{application}");
        if d.backends != [provider.clone()] || !d.requires.contains(&provider) {
            return Err(Error::invalid(
                "Operation must retain its canonical provider scope",
            ));
        }
        if d.dry_run {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Native cooperation does not implement Broker dry-run dispatch",
            ));
        }
        if self.target == TargetRequirement::ObservedResource
            && !d
                .input_schema
                .get("required")
                .and_then(Value::as_array)
                .is_some_and(|keys| keys.iter().any(|key| key == "ref"))
        {
            return Err(Error::invalid(
                "Observed operation schema must require its Broker reference",
            ));
        }
        if !d.input_schema.is_object()
            || !d.output_schema.is_object()
            || d.timeout_ms == 0
            || d.timeout_ms > 300_000
        {
            return Err(Error::invalid(
                "Explicit bounded operation schemas and timeout are required",
            ));
        }
        if d.risk == Risk::ReadOnly
            && (self.commit != CommitSemantics::ReadOnly || d.idempotency != Idempotency::ReadOnly)
        {
            return Err(Error::invalid("Read-only operation guarantees disagree"));
        }
        if d.risk.mutates()
            && (self.commit == CommitSemantics::ReadOnly || d.idempotency == Idempotency::ReadOnly)
        {
            return Err(Error::invalid(
                "Mutation cannot declare read-only commit or idempotency",
            ));
        }
        if d.idempotency == Idempotency::Idempotent && self.retry == RetrySemantics::Never {
            return Err(Error::invalid(
                "Idempotent descriptor requires an explicit replay guarantee",
            ));
        }
        if d.risk == Risk::MutatingReversible && matches!(self.undo, UndoSemantics::None) {
            return Err(Error::invalid(
                "Reversible descriptor requires an application undo operation",
            ));
        }
        if let UndoSemantics::ApplicationOperation { command } = &self.undo
            && !command.starts_with(&prefix)
        {
            return Err(Error::invalid("Undo belongs to another application"));
        }
        if self.atomic_revision_cas
            && (self.commit != CommitSemantics::ApplicationTransaction
                || self.target != TargetRequirement::ObservedResource)
        {
            return Err(Error::invalid(
                "Atomic revision CAS requires a transaction and observed resource",
            ));
        }
        validate_value(&d.input_schema)?;
        validate_value(&d.output_schema)
    }
}

/// Cursors are app-owned data. Their revision/scope binding prevents mixed pages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageCursor {
    pub version: ResourceVersion,
    pub scope: String,
    pub token: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub resource: String,
    pub scope: String,
    pub limit: u16,
    pub cursor: Option<PageCursor>,
}
impl Query {
    pub fn validate(&self) -> Result<()> {
        bounded(&self.resource, 512)?;
        bounded(&self.scope, 128)?;
        if self.limit == 0 || usize::from(self.limit) > MAX_PAGE_ITEMS {
            return Err(Error::invalid("Query page limit"));
        }
        if let Some(cursor) = &self.cursor {
            cursor.version.validate()?;
            bounded(&cursor.token, 1024)?;
            if cursor.scope != self.scope || cursor.version.resource != self.resource {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "Cursor scope differs from query",
                ));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationPage {
    pub version: ResourceVersion,
    pub scope: String,
    pub items: Vec<Value>,
    pub next: Option<PageCursor>,
    /// Completeness is a scoped app claim, never canonical evidence admission.
    pub complete: bool,
}
impl ObservationPage {
    pub fn validate_for(&self, query: &Query) -> Result<()> {
        query.validate()?;
        self.version.validate()?;
        if self.version.resource != query.resource
            || self.scope != query.scope
            || self.items.len() > usize::from(query.limit)
        {
            return Err(Error::invalid(
                "Observation differs from requested resource/scope/budget",
            ));
        }
        if let Some(previous) = &query.cursor
            && previous.version != self.version
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Application revision changed during enumeration",
            ));
        }
        if self.complete && self.next.is_some() {
            return Err(Error::invalid("Complete page cannot have a continuation"));
        }
        if let Some(next) = &self.next {
            bounded(&next.token, 1024)?;
            if next.version != self.version
                || next.scope != self.scope
                || query
                    .cursor
                    .as_ref()
                    .is_some_and(|old| old.token == next.token)
            {
                return Err(Error::invalid(
                    "Continuation is unbound or makes no progress",
                ));
            }
        }
        // Incomplete without continuation means partial/unknown, not complete enumeration.
        validate_value(&serde_json::to_value(self)?)
    }
}

/// Expired deduplication epochs must never become permission to execute old keys again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestIdentity {
    pub resource: String,
    pub epoch: u64,
    pub key: String,
    pub request_sha256: String,
}
impl RequestIdentity {
    pub fn validate(&self) -> Result<()> {
        bounded(&self.resource, 512)?;
        bounded(&self.key, 128)?;
        if self.epoch > MAX_SAFE_INTEGER
            || self.request_sha256.len() != 64
            || !self
                .request_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::invalid("Request epoch or digest is invalid"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecoveryRecord {
    Recorded {
        identity: RequestIdentity,
        result: Value,
    },
    Pending {
        identity: RequestIdentity,
    },
    OutcomeUnknown {
        identity: RequestIdentity,
    },
    RetentionExpired {
        identity: RequestIdentity,
        current_epoch: u64,
    },
}
impl RecoveryRecord {
    pub fn identity(&self) -> &RequestIdentity {
        match self {
            Self::Recorded { identity, .. }
            | Self::Pending { identity }
            | Self::OutcomeUnknown { identity }
            | Self::RetentionExpired { identity, .. } => identity,
        }
    }
    pub fn validate_for(&self, request: &RequestIdentity) -> Result<()> {
        request.validate()?;
        if self.identity() != request {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Historical result request binding differs",
            ));
        }
        if let Self::RetentionExpired { current_epoch, .. } = self
            && (*current_epoch <= request.epoch || *current_epoch > MAX_SAFE_INTEGER)
        {
            return Err(Error::invalid("Invalid retention horizon"));
        }
        validate_value(&serde_json::to_value(self)?)
    }
}

/// The app must explicitly distinguish a rejection from an uncertain commit.
#[derive(Debug)]
pub enum Completion {
    Applied(Value),
    NotApplied(Error),
    Uncertain(Error),
}
impl Completion {
    pub fn into_result(self) -> Result<Value> {
        match self {
            Self::Applied(value) => Ok(value),
            Self::NotApplied(mut error) => {
                error.outcome_known = true;
                Err(error)
            }
            Self::Uncertain(error) => Err(error.uncertain()),
        }
    }
}

/// Carries invocation facts, not new grants. Cannot be reconstructed from JSON.
pub struct CallContext {
    request_id: String,
    expected: Option<ResourceVersion>,
    #[cfg(feature = "driver")]
    host: Option<crate::DriverExecutionContext>,
}
impl CallContext {
    /// Application-local invocation: explicitly not a Broker/Driver Host context.
    pub fn application_local(request_id: impl Into<String>) -> Result<Self> {
        let request_id = request_id.into();
        bounded(&request_id, 256)?;
        Ok(Self {
            request_id,
            expected: None,
            #[cfg(feature = "driver")]
            host: None,
        })
    }
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn expected(&self) -> Option<&ResourceVersion> {
        self.expected.as_ref()
    }
    #[cfg(feature = "driver")]
    pub(crate) fn set_expected(&mut self, expected: Option<ResourceVersion>) {
        self.expected = expected;
    }
    #[cfg(feature = "process-bridge")]
    pub fn cancellation(&self) -> tokio_util::sync::CancellationToken {
        self.host
            .as_ref()
            .map(crate::DriverExecutionContext::cancellation)
            .unwrap_or_default()
    }
    pub fn check_cancelled(&self) -> Result<()> {
        #[cfg(feature = "driver")]
        if let Some(host) = &self.host {
            host.check_cancelled()?;
        }
        Ok(())
    }
    #[cfg(feature = "driver")]
    pub fn driver_context(&self) -> Result<&crate::DriverExecutionContext> {
        self.host
            .as_ref()
            .ok_or_else(|| Error::new(ErrorCode::PermissionDenied, "No Driver Host context"))
    }
    #[cfg(feature = "driver")]
    pub(crate) fn from_host(
        host: crate::DriverExecutionContext,
        expected: Option<ResourceVersion>,
    ) -> Self {
        Self {
            request_id: host.request_id().into(),
            expected,
            host: Some(host),
        }
    }
}

#[async_trait]
pub trait ObservationProvider: Send + Sync + 'static {
    async fn observe(&self, query: &Query, context: &CallContext) -> Result<ObservationPage>;
}
#[async_trait]
pub trait OperationHandler: Send + Sync + 'static {
    /// Compare context.expected() inside the same app transaction as the commit.
    /// Never treat a preflight observation alone as an atomic CAS.
    async fn invoke(&self, args: Value, context: &CallContext) -> Completion;
}
#[async_trait]
pub trait RecoveryProvider: Send + Sync + 'static {
    /// Current access must already be checked. The result is historical data only.
    async fn lookup(
        &self,
        request: &RequestIdentity,
        context: &CallContext,
    ) -> Result<RecoveryRecord>;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventPage {
    pub generation: String,
    pub next_cursor: String,
    pub resync_required: bool,
    pub events: Vec<Value>,
}
#[async_trait]
pub trait EventProvider: Send + Sync + 'static {
    /// Events are hints unless the application documents a durable replay contract.
    async fn poll(
        &self,
        cursor: Option<&str>,
        limit: u16,
        context: &CallContext,
    ) -> Result<EventPage>;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub source: ResourceVersion,
    pub snapshot_id: String,
    pub content_sha256: String,
}
#[async_trait]
pub trait SnapshotProvider: Send + Sync + 'static {
    async fn capture(&self, source: &ResourceVersion, context: &CallContext) -> Result<Snapshot>;
}
#[async_trait]
pub trait WorkspaceProvider: Send + Sync + 'static {
    /// A fork creates new app identity; never copy Broker references or operation authority.
    async fn fork(&self, snapshot: &Snapshot, context: &CallContext) -> Completion;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivatePublication {
    pub candidate_sha256: String,
    pub destination: ResourceVersion,
    pub request: RequestIdentity,
}
impl PrivatePublication {
    pub fn validate(&self) -> Result<()> {
        self.destination.validate()?;
        self.request.validate()?;
        if self.request.resource != self.destination.resource
            || self.candidate_sha256.len() != 64
            || !self
                .candidate_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::invalid(
                "Publication requires destination-bound request and lowercase SHA-256 candidate",
            ));
        }
        Ok(())
    }
}
#[async_trait]
pub trait PublicationProvider: Send + Sync + 'static {
    /// The configured destination must be private. Candidate approval and destination
    /// CAS are checked by the owner/app transaction; this value contains no approval.
    async fn publish(&self, candidate: &PrivatePublication, context: &CallContext) -> Completion;
}

pub(crate) struct RegisteredOperation {
    pub contract: OperationContract,
    pub handler: Arc<dyn OperationHandler>,
}
/// Composition of small interfaces. There is no required Model, file or database type.
pub struct Application {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) observer: Option<Arc<dyn ObservationProvider>>,
    pub(crate) recovery: Option<Arc<dyn RecoveryProvider>>,
    pub(crate) operations: BTreeMap<String, RegisteredOperation>,
    pub(crate) host_tools_required: bool,
}
impl Application {
    pub fn new(id: impl Into<String>, version: impl Into<String>) -> Result<Self> {
        let id = id.into();
        let version = version.into();
        bounded(&id, 64)?;
        bounded(&version, 64)?;
        if !id.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
            || !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(Error::invalid(
                "Application ID must be a canonical driver slug",
            ));
        }
        Ok(Self {
            id,
            version,
            observer: None,
            recovery: None,
            operations: BTreeMap::new(),
            host_tools_required: false,
        })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn with_observer(mut self, provider: Arc<dyn ObservationProvider>) -> Self {
        self.observer = Some(provider);
        self
    }
    pub fn with_recovery(mut self, provider: Arc<dyn RecoveryProvider>) -> Self {
        self.recovery = Some(provider);
        self
    }
    /// Declare that at least one application adapter needs Host-sealed tools.
    /// This only negotiates the existing Driver interface; it does not name,
    /// discover, grant, or launch a tool.
    pub fn require_host_tools(mut self) -> Self {
        self.host_tools_required = true;
        self
    }
    pub fn host_tools_required(&self) -> bool {
        self.host_tools_required
    }
    pub fn register(
        mut self,
        contract: OperationContract,
        handler: Arc<dyn OperationHandler>,
    ) -> Result<Self> {
        contract.validate(&self.id)?;
        let name = contract.descriptor.name.clone();
        if self.operations.len() >= MAX_OPERATIONS
            || self.operations.contains_key(&name)
            || [
                format!("driver.{}.observe", self.id),
                format!("driver.{}.operation.get", self.id),
            ]
            .contains(&name)
        {
            return Err(Error::invalid(
                "Duplicate, reserved or excessive application operation",
            ));
        }
        self.operations
            .insert(name, RegisteredOperation { contract, handler });
        Ok(self)
    }
    pub fn contracts(&self) -> Vec<&OperationContract> {
        self.operations.values().map(|op| &op.contract).collect()
    }
    pub fn handler(&self, name: &str) -> Option<&Arc<dyn OperationHandler>> {
        self.operations
            .get(name)
            .map(|operation| &operation.handler)
    }
    pub fn has_observer(&self) -> bool {
        self.observer.is_some()
    }
    pub fn has_recovery(&self) -> bool {
        self.recovery.is_some()
    }
}

/// Shared Rust/TypeScript value boundary. Unsafe JS integers are rejected rather
/// than rounded. Revisions and large application counters should use strings.
pub fn validate_value(value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_VALUE_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Cooperation value byte budget",
        ));
    }
    let mut pending = vec![(value, 0usize)];
    let mut count = 0usize;
    while let Some((value, depth)) = pending.pop() {
        count += 1;
        if count > 16_384 || depth > 32 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Cooperation value structural budget",
            ));
        }
        match value {
            Value::Number(number) => {
                if let Some(n) = number.as_u64() {
                    if n > MAX_SAFE_INTEGER {
                        return Err(Error::invalid("Integer exceeds the shared exact range"));
                    }
                } else if let Some(n) = number.as_i64() {
                    if n.unsigned_abs() > MAX_SAFE_INTEGER {
                        return Err(Error::invalid("Integer exceeds the shared exact range"));
                    }
                } else if number
                    .as_f64()
                    .is_none_or(|n| !n.is_finite() || n.abs() > MAX_SAFE_INTEGER as f64)
                {
                    return Err(Error::invalid("Number exceeds the shared finite range"));
                }
            }
            Value::Array(items) => pending.extend(items.iter().map(|item| (item, depth + 1))),
            Value::Object(items) => pending.extend(items.values().map(|item| (item, depth + 1))),
            _ => {}
        }
    }
    Ok(())
}

/// Optional exact app-level request digest shared with the TypeScript binding.
pub fn exact_request_digest(domain: &str, value: &Value) -> Result<String> {
    bounded(domain, 128)?;
    validate_value(value)?;
    fn encode(value: &Value, out: &mut String) -> Result<()> {
        match value {
            Value::Null => out.push_str("null"),
            Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Value::Number(value) => {
                if let Some(number) = value.as_i64() {
                    if number.unsigned_abs() > MAX_SAFE_INTEGER {
                        return Err(Error::invalid(
                            "Exact request digest integer exceeds shared bound",
                        ));
                    }
                    out.push_str(&number.to_string());
                } else if let Some(number) = value.as_u64() {
                    if number > MAX_SAFE_INTEGER {
                        return Err(Error::invalid(
                            "Exact request digest integer exceeds shared bound",
                        ));
                    }
                    out.push_str(&number.to_string());
                } else if let Some(number) = value.as_f64() {
                    if !number.is_finite()
                        || number.fract() != 0.0
                        || number.abs() > MAX_SAFE_INTEGER as f64
                    {
                        return Err(Error::invalid(
                            "Exact request digest requires exact integer numbers",
                        ));
                    }
                    if number == 0.0 {
                        out.push('0');
                    } else {
                        out.push_str(&format!("{number:.0}"));
                    }
                } else {
                    return Err(Error::invalid("Unsupported request digest number"));
                }
            }
            Value::String(value) => out.push_str(&serde_json::to_string(value)?),
            Value::Array(values) => {
                out.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    encode(value, out)?;
                }
                out.push(']');
            }
            Value::Object(values) => {
                out.push('{');
                let mut keys: Vec<_> = values.keys().collect();
                keys.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
                for (index, key) in keys.into_iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(key)?);
                    out.push(':');
                    encode(values.get(key).expect("key originated from map"), out)?;
                }
                out.push('}');
            }
        }
        Ok(())
    }
    let mut canonical = String::new();
    encode(value, &mut canonical)?;
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    hasher.update(b"\n");
    hasher.update(canonical.as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

pub fn version_fingerprint(version: &ResourceVersion) -> Result<String> {
    version.validate()?;
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(version)?)))
}
