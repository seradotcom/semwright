//! Stable, transport-independent domain model. No operating-system side effects.
use regex::RegexBuilder;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, Error>;
pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME: usize = 1_048_576;
pub const MAX_NODES: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ErrorCode {
    Unsupported,
    Unavailable,
    PermissionDenied,
    ConsentRequired,
    PolicyDenied,
    NotFound,
    AmbiguousTarget,
    StaleReference,
    BackendFailed,
    Timeout,
    InvalidArgument,
    PluginProtocolError,
    SandboxDenied,
    Conflict,
    Cancelled,
    Internal,
    ProtocolMismatch,
    ResourceExhausted,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    /// False after a timeout/disconnect during a mutation: never automatically retry.
    #[serde(default)]
    pub outcome_known: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<RecipeProgress>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecipeProgress {
    pub completed_steps: usize,
    pub failed_step: String,
    pub state: String,
}
impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            candidates: vec![],
            outcome_known: true,
            progress: None,
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unavailable, message)
    }
    pub fn uncertain(mut self) -> Self {
        self.outcome_known = false;
        self
    }
    pub fn recipe_progress(mut self, completed: usize, step: &str) -> Self {
        self.progress = Some(RecipeProgress {
            completed_steps: completed,
            failed_step: step.into(),
            state: if completed > 0 || !self.outcome_known {
                "partial_success_or_unknown"
            } else {
                "failed_before_confirmed_step"
            }
            .into(),
        });
        self
    }
    pub fn exit_code(&self) -> i32 {
        match self.code {
            ErrorCode::InvalidArgument | ErrorCode::ProtocolMismatch => 2,
            ErrorCode::PolicyDenied | ErrorCode::PermissionDenied | ErrorCode::ConsentRequired => 3,
            ErrorCode::NotFound | ErrorCode::StaleReference | ErrorCode::AmbiguousTarget => 4,
            ErrorCode::Unavailable | ErrorCode::Unsupported | ErrorCode::SandboxDenied => 5,
            ErrorCode::Timeout | ErrorCode::Cancelled => 6,
            _ => 1,
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        // OS messages may contain user paths or process output. Only disclose the class.
        Self::new(
            ErrorCode::BackendFailed,
            format!("I/O operation failed ({:?})", e.kind()),
        )
    }
}
impl From<serde_json::Error> for Error {
    fn from(_: serde_json::Error) -> Self {
        Self::invalid("Malformed JSON or invalid typed payload")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    ReadOnly,
    MutatingReversible,
    Mutating,
    Destructive,
    SecretAccess,
    CodeExecution,
    PrivilegeSensitive,
}
impl Risk {
    pub fn mutates(self) -> bool {
        !matches!(self, Self::ReadOnly | Self::SecretAccess)
    }
    pub fn sensitive(self) -> bool {
        matches!(
            self,
            Self::Destructive | Self::SecretAccess | Self::CodeExecution | Self::PrivilegeSensitive
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Idempotency {
    ReadOnly,
    Idempotent,
    NonIdempotent,
    Destructive,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandDescriptor {
    pub name: String,
    pub version: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub requires: Vec<String>,
    pub risk: Risk,
    pub idempotency: Idempotency,
    pub timeout_ms: u64,
    pub dry_run: bool,
    pub interactive_consent: bool,
    pub backends: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecuteRequest {
    pub command: String,
    #[serde(default = "empty_object")]
    pub args: Value,
    #[serde(default)]
    pub dry_run: bool,
    /// A preference, never permission to bypass capability checks.
    #[serde(default)]
    pub backend: Option<String>,
}
pub fn empty_object() -> Value {
    serde_json::json!({})
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Execution {
    pub backend: String,
    pub duration_ms: u64,
    pub policy_decision: String,
    pub fallbacks_attempted: Vec<String>,
    pub dry_run: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Envelope {
    pub ok: bool,
    pub request_id: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
    pub execution: Execution,
    pub warnings: Vec<String>,
}
impl Envelope {
    pub fn finish(
        id: String,
        command: String,
        backend: String,
        elapsed: Duration,
        dry_run: bool,
        result: Result<Value>,
    ) -> Self {
        let (ok, data, error) = match result {
            Ok(v) => (true, Some(v), None),
            Err(e) => (false, None, Some(e)),
        };
        let decision = match error.as_ref().map(|e| e.code) {
            Some(ErrorCode::PolicyDenied | ErrorCode::PermissionDenied) => "deny",
            Some(ErrorCode::ConsentRequired) => "require_confirmation",
            None if ok => "allow",
            _ => "not_evaluated",
        };
        Self {
            ok,
            request_id: id,
            command,
            data,
            error,
            warnings: vec![],
            execution: Execution {
                backend,
                duration_ms: elapsed.as_millis().min(u64::MAX as u128) as u64,
                policy_decision: decision.into(),
                fallbacks_attempted: vec![],
                dry_run,
            },
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CapabilityStatus {
    Supported,
    SupportedWithConsent,
    SupportedWithHelper,
    Experimental,
    Unavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Feature {
    pub backend: String,
    pub capability: String,
    pub status: CapabilityStatus,
    pub reason: String,
    pub remediation: String,
}
impl Feature {
    pub fn usable(&self) -> bool {
        self.status != CapabilityStatus::Unavailable
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NativeTarget {
    pub kind: String,
    pub identity: String,
    pub revision: u64,
    pub fingerprint: String,
    pub app: String,
}
pub fn target_marker(target: NativeTarget) -> Value {
    serde_json::json!({"$ref": target})
}
#[derive(Debug, Clone)]
pub struct Reference {
    pub session: String,
    pub backend: String,
    pub target: NativeTarget,
    pub expires: Instant,
}
/// Opaque references never alias a newly-created object. Bounded, monotonic expiry.
pub struct RefStore {
    entries: BTreeMap<String, Reference>,
    ttl: Duration,
    capacity: usize,
}
impl RefStore {
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            ttl,
            capacity,
        }
    }
    pub fn insert(&mut self, session: &str, backend: &str, target: NativeTarget) -> Result<String> {
        let now = Instant::now();
        self.entries.retain(|_, r| r.expires > now);
        if self.entries.len() >= self.capacity {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Reference budget exhausted; wait for expiry",
            ));
        }
        if !matches!(
            target.kind.as_str(),
            "ui" | "win" | "app" | "screen" | "dom" | "tab" | "process"
        ) {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Invalid backend reference kind",
            ));
        }
        let id = format!("{}:{}", target.kind, Uuid::new_v4().simple());
        self.entries.insert(
            id.clone(),
            Reference {
                session: session.into(),
                backend: backend.into(),
                target,
                expires: now + self.ttl,
            },
        );
        Ok(id)
    }
    pub fn get(&self, id: &str, session: &str) -> Result<Reference> {
        self.entries
            .get(id)
            .filter(|r| r.session == session && r.expires > Instant::now())
            .cloned()
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::StaleReference,
                    "Reference expired, belongs to another session, or does not exist",
                )
            })
    }
    pub fn revoke_session(&mut self, session: &str) {
        self.entries.retain(|_, r| r.session != session);
    }
    pub fn invalidate_backend(&mut self, backend: &str) {
        self.entries.retain(|_, r| r.backend != backend);
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NameMatch {
    pub op: NameOp,
    pub value: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NameOp {
    Exact,
    Regex,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    pub app: Option<String>,
    pub role: Option<String>,
    pub name: Option<NameMatch>,
    #[serde(default)]
    pub states: Vec<String>,
    pub action: Option<String>,
    pub ancestor: Option<String>,
    pub nth: Option<usize>,
    /// Discovery only. Never accepted by an invocation command.
    pub query: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UiNode {
    #[serde(rename = "ref")]
    pub reference: String,
    pub role: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub app: String,
    pub parent_ref: Option<String>,
    pub bounds: Option<Bounds>,
    #[serde(default)]
    pub children_count: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub coordinate_space: String,
}
impl Selector {
    pub fn select<'a>(&self, nodes: &'a [UiNode]) -> Result<Vec<&'a UiNode>> {
        if nodes.len() > MAX_NODES {
            return Err(Error::invalid("Node budget exceeded"));
        }
        let regex = match &self.name {
            Some(NameMatch {
                op: NameOp::Regex,
                value,
            }) => {
                if value.len() > 512 {
                    return Err(Error::invalid("Regex exceeds 512 bytes"));
                }
                Some(
                    RegexBuilder::new(value)
                        .size_limit(1_000_000)
                        .build()
                        .map_err(|_| Error::invalid("Invalid or oversized regex"))?,
                )
            }
            _ => None,
        };
        let index: BTreeMap<&str, &UiNode> =
            nodes.iter().map(|n| (n.reference.as_str(), n)).collect();
        let mut found: Vec<&UiNode> = nodes
            .iter()
            .filter(|n| {
                self.app.as_ref().is_none_or(|a| a == &n.app)
                    && self.role.as_ref().is_none_or(|r| r == &n.role)
                    && self.states.iter().all(|s| n.states.contains(s))
                    && self.action.as_ref().is_none_or(|a| n.actions.contains(a))
                    && self.name.as_ref().is_none_or(|m| match m.op {
                        NameOp::Exact => n.name == m.value,
                        NameOp::Regex => regex.as_ref().is_some_and(|r| r.is_match(&n.name)),
                    })
                    && self.ancestor.as_ref().is_none_or(|ancestor| {
                        let mut parent = n.parent_ref.as_deref();
                        let mut seen = BTreeSet::new();
                        while let Some(p) = parent {
                            if !seen.insert(p) {
                                break;
                            }
                            if p == ancestor {
                                return true;
                            }
                            parent = index.get(p).and_then(|v| v.parent_ref.as_deref());
                        }
                        false
                    })
            })
            .collect();
        if let Some(query) = &self.query {
            if query.len() > 256 {
                return Err(Error::invalid("Discovery query is too long"));
            }
            let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
            let score = |n: &&UiNode| -> usize {
                let text = format!("{} {} {}", n.role, n.name, n.description).to_lowercase();
                words.iter().filter(|w| text.contains(w.as_str())).count()
            };
            found.retain(|n| score(&n) > 0);
            found.sort_by_key(|n| std::cmp::Reverse(score(n)));
        }
        if let Some(n) = self.nth {
            return Ok(found.get(n).copied().into_iter().collect());
        }
        Ok(found)
    }
    pub fn unique<'a>(&self, nodes: &'a [UiNode]) -> Result<&'a UiNode> {
        if self.query.is_some() {
            return Err(Error::invalid(
                "Ranked discovery cannot select a mutating target",
            ));
        }
        let matches = self.select(nodes)?;
        match matches.as_slice() {
            [one] => Ok(one),
            [] => Err(Error::new(
                ErrorCode::NotFound,
                "No semantic target matches",
            )),
            _ => {
                let mut e = Error::new(
                    ErrorCode::AmbiguousTarget,
                    "Multiple semantic targets match",
                );
                e.candidates = matches.iter().map(|n| n.reference.clone()).collect();
                Err(e)
            }
        }
    }
}
pub fn arg_str<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(format!("Missing string field {key}")))
}
pub fn native_target(args: &Value) -> Result<NativeTarget> {
    serde_json::from_value(
        args.get("_target")
            .cloned()
            .ok_or_else(|| Error::invalid("Broker-resolved target required"))?,
    )
    .map_err(Into::into)
}
pub fn unique_id() -> String {
    Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    fn node(id: &str, name: &str) -> UiNode {
        UiNode {
            reference: id.into(),
            name: name.into(),
            role: "button".into(),
            description: String::new(),
            states: vec!["enabled".into()],
            actions: vec!["click".into()],
            app: "test".into(),
            parent_ref: None,
            bounds: None,
            children_count: 0,
        }
    }
    #[test]
    fn ambiguity_is_not_an_action() {
        let nodes = [node("ui:1", "Save"), node("ui:2", "Save")];
        let s = Selector {
            name: Some(NameMatch {
                op: NameOp::Exact,
                value: "Save".into(),
            }),
            ..Default::default()
        };
        let e = s.unique(&nodes).unwrap_err();
        assert_eq!(e.code, ErrorCode::AmbiguousTarget);
        assert_eq!(e.candidates.len(), 2);
    }
    #[test]
    fn ranked_is_discovery_only() {
        let nodes = [node("ui:1", "Export")];
        let s = Selector {
            query: Some("export".into()),
            ..Default::default()
        };
        assert_eq!(s.select(&nodes).unwrap().len(), 1);
        assert!(s.unique(&nodes).is_err());
    }
    #[test]
    fn cycles_terminate() {
        let mut a = node("ui:1", "A");
        a.parent_ref = Some("ui:2".into());
        let mut b = node("ui:2", "B");
        b.parent_ref = Some("ui:1".into());
        let s = Selector {
            ancestor: Some("ui:missing".into()),
            ..Default::default()
        };
        assert!(s.select(&[a, b]).unwrap().is_empty());
    }
    #[test]
    fn refs_are_session_bound() {
        let mut store = RefStore::new(Duration::from_secs(60), 2);
        let target = NativeTarget {
            kind: "ui".into(),
            identity: "1".into(),
            revision: 1,
            fingerprint: "f".into(),
            app: "a".into(),
        };
        let id = store.insert("s1", "fake", target).unwrap();
        assert!(store.get(&id, "s2").is_err());
        assert!(store.get(&id, "s1").is_ok());
        store.invalidate_backend("fake");
        assert!(store.get(&id, "s1").is_err());
    }
    #[test]
    fn expired_refs_fail() {
        let mut store = RefStore::new(Duration::ZERO, 1);
        let id = store
            .insert(
                "s",
                "fake",
                NativeTarget {
                    kind: "win".into(),
                    identity: "1".into(),
                    revision: 1,
                    fingerprint: "f".into(),
                    app: "a".into(),
                },
            )
            .unwrap();
        assert!(store.get(&id, "s").is_err());
    }
    proptest! {
        #[test] fn exact_match_never_invents_nodes(name in ".{0,100}") {
            let nodes = [node("ui:1", &name)];
            let s = Selector { name: Some(NameMatch { op: NameOp::Exact, value: name }), ..Default::default() };
            prop_assert_eq!(s.unique(&nodes).unwrap().reference.as_str(), "ui:1");
        }
        #[test] fn serde_error_roundtrip(msg in ".{0,100}") {
            let e = Error::invalid(msg); let bytes = serde_json::to_vec(&e).unwrap();
            let back: Error = serde_json::from_slice(&bytes).unwrap(); prop_assert_eq!(e.message, back.message);
        }
    }
}
