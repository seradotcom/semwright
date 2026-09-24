//! Stable, transport-independent domain model. No operating-system side effects.
pub mod event;
pub mod job;
pub mod provider;
pub use event::{EventEnvelope, semantic_ui_event};
pub use job::{JobArtifact, JobProgress, JobSnapshot, JobState};
pub use provider::{InvocationProvenance, ProviderIdentity, SourceKind};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<InvocationProvenance>,
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
                provenance: None,
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RelationMatch {
    pub kind: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiRelation {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiTextRange {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiTextFacet {
    pub character_count: Option<usize>,
    pub caret_offset: Option<i64>,
    pub selection_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selections: Vec<UiTextRange>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub caret_attributes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caret_attribute_range: Option<UiTextRange>,
    #[serde(default)]
    pub editable: bool,
    #[serde(default)]
    pub password: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiValueFacet {
    pub current: Option<f64>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub increment: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiSelectionFacet {
    pub selected: Option<bool>,
    pub selected_count: Option<usize>,
    pub child_count: Option<usize>,
    pub multi_select: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiTableFacet {
    pub rows: Option<usize>,
    pub columns: Option<usize>,
    pub row: Option<usize>,
    pub column: Option<usize>,
    pub row_span: Option<usize>,
    pub column_span: Option<usize>,
    pub selected_rows: Option<usize>,
    pub selected_columns: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_headers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub column_headers: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiDocumentFacet {
    pub locale: Option<String>,
    pub page_index: Option<i64>,
    pub page_count: Option<i64>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiHypertextFacet {
    pub link_count: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiImageFacet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiScrollFacet {
    pub horizontal_percent: Option<f64>,
    pub vertical_percent: Option<f64>,
    pub horizontal_view_size: Option<f64>,
    pub vertical_view_size: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiWindowFacet {
    pub modal: Option<bool>,
    pub minimized: Option<bool>,
    pub maximized: Option<bool>,
    pub can_minimize: Option<bool>,
    pub can_maximize: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiTransformFacet {
    pub can_move: Option<bool>,
    pub can_resize: Option<bool>,
    pub can_rotate: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiFacets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<UiTextFacet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<UiValueFacet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<UiSelectionFacet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<UiTableFacet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<UiDocumentFacet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hypertext: Option<UiHypertextFacet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<UiImageFacet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll: Option<UiScrollFacet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<UiWindowFacet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<UiTransformFacet>,
}

impl UiFacets {
    pub fn is_empty(&self) -> bool {
        self.text.is_none()
            && self.value.is_none()
            && self.selection.is_none()
            && self.table.is_none()
            && self.document.is_none()
            && self.hypertext.is_none()
            && self.image.is_none()
            && self.scroll.is_none()
            && self.window.is_none()
            && self.transform.is_none()
    }

    pub fn has(&self, name: &str) -> bool {
        match name {
            "text" => self.text.is_some(),
            "value" => self.value.is_some(),
            "selection" => self.selection.is_some(),
            "table" => self.table.is_some(),
            "document" => self.document.is_some(),
            "hypertext" => self.hypertext.is_some(),
            "image" => self.image.is_some(),
            "scroll" => self.scroll.is_some(),
            "window" => self.window.is_some(),
            "transform" => self.transform.is_some(),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextFacetMatch {
    pub editable: Option<bool>,
    pub password: Option<bool>,
    pub has_selection: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValueFacetMatch {
    pub current_minimum: Option<f64>,
    pub current_maximum: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionFacetMatch {
    pub selected: Option<bool>,
    pub multi_select: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableFacetMatch {
    pub row: Option<usize>,
    pub column: Option<usize>,
    pub min_rows: Option<usize>,
    pub min_columns: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    pub app: Option<String>,
    pub role: Option<String>,
    pub name: Option<NameMatch>,
    pub help: Option<NameMatch>,
    pub framework: Option<String>,
    #[serde(default)]
    pub states: Vec<String>,
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    pub relation: Option<RelationMatch>,
    pub facet: Option<String>,
    pub text: Option<TextFacetMatch>,
    pub value: Option<ValueFacetMatch>,
    pub selection: Option<SelectionFacetMatch>,
    pub table: Option<TableFacetMatch>,
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub help: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub accessibility_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub framework: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<UiRelation>,
    #[serde(default, skip_serializing_if = "UiFacets::is_empty")]
    pub facets: UiFacets,
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
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub coordinate_space: String,
}
impl Selector {
    pub fn select<'a>(&self, nodes: &'a [UiNode]) -> Result<Vec<&'a UiNode>> {
        if nodes.len() > MAX_NODES {
            return Err(Error::invalid("Node budget exceeded"));
        }
        let compile_regex = |matcher: &Option<NameMatch>| -> Result<_> {
            match matcher {
                Some(NameMatch {
                    op: NameOp::Regex,
                    value,
                }) => {
                    if value.len() > 512 {
                        return Err(Error::invalid("Regex exceeds 512 bytes"));
                    }
                    Ok(Some(
                        RegexBuilder::new(value)
                            .size_limit(1_000_000)
                            .build()
                            .map_err(|_| Error::invalid("Invalid or oversized regex"))?,
                    ))
                }
                _ => Ok(None),
            }
        };
        let name_regex = compile_regex(&self.name)?;
        let help_regex = compile_regex(&self.help)?;
        if let Some(value) = &self.value {
            if value.current_minimum.is_some_and(|v| !v.is_finite())
                || value.current_maximum.is_some_and(|v| !v.is_finite())
                || matches!((value.current_minimum, value.current_maximum), (Some(min), Some(max)) if min > max)
            {
                return Err(Error::invalid("Value selector range is invalid"));
            }
        }
        let index: BTreeMap<&str, &UiNode> =
            nodes.iter().map(|n| (n.reference.as_str(), n)).collect();
        let mut found: Vec<&UiNode> = nodes
            .iter()
            .filter(|n| {
                self.app.as_ref().is_none_or(|a| a == &n.app)
                    && self.role.as_ref().is_none_or(|r| r == &n.role)
                    && self.framework.as_ref().is_none_or(|f| f == &n.framework)
                    && self.states.iter().all(|s| n.states.contains(s))
                    && self.action.as_ref().is_none_or(|a| n.actions.contains(a))
                    && self
                        .attributes
                        .iter()
                        .all(|(k, v)| n.attributes.get(k) == Some(v))
                    && self.facet.as_ref().is_none_or(|f| n.facets.has(f))
                    && self.text.as_ref().is_none_or(|wanted| {
                        n.facets.text.as_ref().is_some_and(|text| {
                            wanted.editable.is_none_or(|v| text.editable == v)
                                && wanted.password.is_none_or(|v| text.password == v)
                                && wanted.has_selection.is_none_or(|v| {
                                    let has_selection = !text.selections.is_empty()
                                        || text.selection_count.is_some_and(|count| count > 0);
                                    has_selection == v
                                })
                        })
                    })
                    && self.value.as_ref().is_none_or(|wanted| {
                        n.facets.value.as_ref().is_some_and(|value| {
                            value.current.is_some_and(|current| {
                                wanted.current_minimum.is_none_or(|min| current >= min)
                                    && wanted.current_maximum.is_none_or(|max| current <= max)
                            })
                        })
                    })
                    && self.selection.as_ref().is_none_or(|wanted| {
                        n.facets.selection.as_ref().is_some_and(|selection| {
                            wanted
                                .selected
                                .is_none_or(|v| selection.selected == Some(v))
                                && wanted
                                    .multi_select
                                    .is_none_or(|v| selection.multi_select == Some(v))
                        })
                    })
                    && self.table.as_ref().is_none_or(|wanted| {
                        n.facets.table.as_ref().is_some_and(|table| {
                            wanted.row.is_none_or(|row| table.row == Some(row))
                                && wanted
                                    .column
                                    .is_none_or(|column| table.column == Some(column))
                                && wanted
                                    .min_rows
                                    .is_none_or(|rows| table.rows.is_some_and(|v| v >= rows))
                                && wanted.min_columns.is_none_or(|columns| {
                                    table.columns.is_some_and(|v| v >= columns)
                                })
                        })
                    })
                    && self.relation.as_ref().is_none_or(|wanted| {
                        n.relations.iter().any(|relation| {
                            relation.kind == wanted.kind
                                && wanted
                                    .target
                                    .as_ref()
                                    .is_none_or(|target| relation.targets.contains(target))
                        })
                    })
                    && self.name.as_ref().is_none_or(|m| match m.op {
                        NameOp::Exact => n.name == m.value,
                        NameOp::Regex => name_regex.as_ref().is_some_and(|r| r.is_match(&n.name)),
                    })
                    && self.help.as_ref().is_none_or(|m| match m.op {
                        NameOp::Exact => n.help == m.value,
                        NameOp::Regex => help_regex.as_ref().is_some_and(|r| r.is_match(&n.help)),
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
                let attribute_text = n
                    .attributes
                    .iter()
                    .map(|(key, value)| format!("{key} {value}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let text = format!(
                    "{} {} {} {} {} {}",
                    n.role, n.name, n.description, n.help, n.framework, attribute_text
                )
                .to_lowercase();
                words.iter().filter(|w| text.contains(w.as_str())).count()
            };
            found.retain(|n| score(n) > 0);
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
            help: String::new(),
            accessibility_id: String::new(),
            framework: String::new(),
            attributes: BTreeMap::new(),
            relations: vec![],
            facets: UiFacets::default(),
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
    fn rich_selector_matches_facets_attributes_relations_and_help() {
        let mut n = node("ui:1", "");
        n.help = "Adjust opacity".into();
        n.framework = "gtk4".into();
        n.attributes
            .insert("semantic-name".into(), "Opacity".into());
        n.facets.value = Some(UiValueFacet {
            current: Some(75.0),
            minimum: Some(0.0),
            maximum: Some(100.0),
            increment: Some(1.0),
            text: None,
        });
        n.relations.push(UiRelation {
            kind: "labelled_by".into(),
            targets: vec!["ui:label".into()],
        });
        let selector = Selector {
            help: Some(NameMatch {
                op: NameOp::Regex,
                value: "opacity".into(),
            }),
            framework: Some("gtk4".into()),
            attributes: BTreeMap::from([("semantic-name".into(), "Opacity".into())]),
            relation: Some(RelationMatch {
                kind: "labelled_by".into(),
                target: Some("ui:label".into()),
            }),
            facet: Some("value".into()),
            ..Default::default()
        };
        assert_eq!(selector.unique(&[n]).unwrap().reference, "ui:1");
    }

    #[test]
    fn fractional_bounds_roundtrip_for_hidpi_platforms() {
        let bounds = Bounds {
            x: -10.25,
            y: 20.5,
            width: 640.5,
            height: 360.25,
            coordinate_space: "screen".into(),
        };
        let value = serde_json::to_value(&bounds).unwrap();
        let back: Bounds = serde_json::from_value(value).unwrap();
        assert_eq!(back.x, -10.25);
        assert_eq!(back.width, 640.5);
    }

    #[test]
    fn legacy_ui_node_json_deserializes_with_empty_v2_fields() {
        let n: UiNode = serde_json::from_value(serde_json::json!({
            "ref":"ui:1",
            "role":"button",
            "name":"Save",
            "states":[],
            "actions":["click"],
            "app":"fixture",
            "parent_ref":null,
            "bounds":null,
            "children_count":0
        }))
        .unwrap();
        assert!(n.help.is_empty());
        assert!(n.attributes.is_empty());
        assert!(n.relations.is_empty());
        assert!(!n.facets.has("text"));
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
