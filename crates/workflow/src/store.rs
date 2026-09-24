use crate::{
    ActiveTrace, CANDIDATE_VERSION, Candidate, MAX_PROMOTIONS, MAX_TRACE_STEPS, MAX_TRACES,
    Promotion, TRACE_VERSION, TraceStep, WorkflowTrace,
};
use semwright_types::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Persisted {
    version: u32,
    traces: Vec<WorkflowTrace>,
    candidates: Vec<Candidate>,
    promotions: Vec<Promotion>,
}

#[derive(Debug, Default)]
pub struct WorkflowManager {
    active: BTreeMap<String, ActiveTrace>,
    traces: BTreeMap<String, WorkflowTrace>,
    candidates: BTreeMap<String, Candidate>,
    promotions: BTreeMap<String, Promotion>,
    persistence: Option<PathBuf>,
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn canonical_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.as_bytes()[s.len() - 1].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl WorkflowManager {
    pub fn configure_persistence(&mut self, directory: &Path) -> Result<()> {
        fs::create_dir_all(directory)?;
        let directory_metadata = fs::symlink_metadata(directory)?;
        if !directory_metadata.is_dir() || directory_metadata.file_type().is_symlink() {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Workflow persistence directory must be a real directory",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        }
        let file = directory.join("workflows.json");
        self.persistence = Some(file.clone());
        if !file.exists() {
            return Ok(());
        }
        let metadata = fs::symlink_metadata(&file)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Workflow store must be a regular non-symlink file",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Workflow store permissions are broader than 0600",
                ));
            }
        }
        let bytes = fs::read(&file)?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Workflow store exceeds 8 MiB",
            ));
        }
        let state: Persisted = serde_json::from_slice(&bytes)
            .map_err(|_| Error::new(ErrorCode::Conflict, "Workflow store is corrupt"))?;
        let trace_ids = state
            .traces
            .iter()
            .map(|value| value.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let candidate_ids = state
            .candidates
            .iter()
            .map(|value| value.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let promotion_slugs = state
            .promotions
            .iter()
            .map(|value| value.slug.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let invalid_trace = state.traces.iter().any(|trace| {
            trace.version != TRACE_VERSION
                || !trace.id.starts_with("trace-")
                || !valid_name(&trace.name)
                || trace.steps.is_empty()
                || trace.steps.len() > MAX_TRACE_STEPS
        });
        let invalid_candidate = state.candidates.iter().any(|candidate| {
            candidate.version != CANDIDATE_VERSION
                || !candidate.id.starts_with("candidate-")
                || candidate.recipe.steps.is_empty()
                || candidate.recipe.steps.len() > MAX_TRACE_STEPS
        });
        let invalid_promotion = state.promotions.iter().any(|promotion| {
            promotion.version != 1
                || !canonical_slug(&promotion.slug)
                || promotion.capability != format!("recipe.{}.run", promotion.slug)
                || promotion.candidate.version != CANDIDATE_VERSION
        });
        if state.version != 1
            || state.traces.len() > MAX_TRACES
            || state.candidates.len() > MAX_TRACES
            || state.promotions.len() > MAX_PROMOTIONS
            || trace_ids.len() != state.traces.len()
            || candidate_ids.len() != state.candidates.len()
            || promotion_slugs.len() != state.promotions.len()
            || invalid_trace
            || invalid_candidate
            || invalid_promotion
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Workflow store version, identity, or budget is invalid",
            ));
        }
        self.traces = state
            .traces
            .into_iter()
            .map(|v| (v.id.clone(), v))
            .collect();
        self.candidates = state
            .candidates
            .into_iter()
            .map(|v| (v.id.clone(), v))
            .collect();
        self.promotions = state
            .promotions
            .into_iter()
            .map(|v| (v.slug.clone(), v))
            .collect();
        Ok(())
    }
    fn persist(&self) -> Result<()> {
        let Some(path) = &self.persistence else {
            return Ok(());
        };
        let state = Persisted {
            version: 1,
            traces: self.traces.values().cloned().collect(),
            candidates: self.candidates.values().cloned().collect(),
            promotions: self.promotions.values().cloned().collect(),
        };
        let bytes = serde_json::to_vec_pretty(&state)?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Workflow store exceeds 8 MiB",
            ));
        }
        let tmp = path.with_extension(format!("tmp.{}", Uuid::new_v4().simple()));
        {
            use std::io::Write;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        if let Err(error) = fs::rename(&tmp, path) {
            let _ = fs::remove_file(&tmp);
            return Err(error.into());
        }
        #[cfg(unix)]
        if let Some(parent) = path.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
    pub fn start(
        &mut self,
        session: &str,
        name: &str,
        intent: &str,
        capture_values: bool,
    ) -> Result<String> {
        if self.active.contains_key(session) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "This session is already recording a workflow",
            ));
        }
        if self.active.len() >= 32 || self.traces.len() >= MAX_TRACES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Workflow recording budget exhausted",
            ));
        }
        if !valid_name(name) || intent.len() > 4096 || intent.chars().any(char::is_control) {
            return Err(Error::invalid("Workflow name or intent is invalid"));
        }
        let id = format!("trace-{}", Uuid::new_v4().simple());
        self.active.insert(
            session.into(),
            ActiveTrace {
                id: id.clone(),
                name: name.into(),
                intent: intent.into(),
                started_unix_ms: now_ms(),
                capture_values,
                steps: vec![],
                invalid_reason: None,
            },
        );
        Ok(id)
    }
    pub fn active_capture_values(&self, session: &str) -> Option<bool> {
        self.active.get(session).map(|v| v.capture_values)
    }

    pub fn invalidate(&mut self, session: &str, reason: &str) {
        if let Some(active) = self.active.get_mut(session) {
            active.invalid_reason = Some(reason.chars().take(512).collect());
        }
    }

    pub fn record(&mut self, session: &str, mut step: TraceStep) -> Result<()> {
        let Some(active) = self.active.get_mut(session) else {
            return Ok(());
        };
        if active.steps.len() >= MAX_TRACE_STEPS {
            active.invalid_reason = Some("Workflow trace exceeded 64 steps".into());
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Workflow trace exceeded 64 steps",
            ));
        }
        if active.invalid_reason.is_some() {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Workflow recording was invalidated",
            ));
        }
        step.index = active.steps.len();
        active.steps.push(step);
        Ok(())
    }

    pub fn stop(&mut self, session: &str, successful: bool) -> Result<WorkflowTrace> {
        let active = self
            .active
            .remove(session)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "No workflow recording is active"))?;
        if let Some(reason) = active.invalid_reason {
            return Err(Error::new(ErrorCode::Conflict, reason));
        }
        if active.steps.is_empty() {
            return Err(Error::invalid(
                "Workflow trace contains no operational steps",
            ));
        }
        let trace = WorkflowTrace {
            version: TRACE_VERSION,
            id: active.id,
            name: active.name,
            intent: active.intent,
            started_unix_ms: active.started_unix_ms,
            ended_unix_ms: now_ms(),
            capture_values: active.capture_values,
            successful,
            steps: active.steps,
        };
        self.traces.insert(trace.id.clone(), trace.clone());
        if let Err(error) = self.persist() {
            self.traces.remove(&trace.id);
            self.active.insert(
                session.into(),
                ActiveTrace {
                    id: trace.id.clone(),
                    name: trace.name.clone(),
                    intent: trace.intent.clone(),
                    started_unix_ms: trace.started_unix_ms,
                    capture_values: trace.capture_values,
                    steps: trace.steps.clone(),
                    invalid_reason: None,
                },
            );
            return Err(error);
        }
        Ok(trace)
    }

    pub fn list_traces(&self) -> Vec<Value> {
        self.traces
            .values()
            .map(|t| {
                json!({
                    "id":t.id,
                    "name":t.name,
                    "steps":t.steps.len(),
                    "successful":t.successful,
                    "capture_values":t.capture_values,
                    "started_unix_ms":t.started_unix_ms,
                    "ended_unix_ms":t.ended_unix_ms
                })
            })
            .collect()
    }

    pub fn trace(&self, id: &str) -> Result<WorkflowTrace> {
        self.traces
            .get(id)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Workflow trace not found"))
    }

    pub fn delete_trace(&mut self, id: &str) -> Result<WorkflowTrace> {
        if self.candidates.values().any(|candidate| {
            candidate
                .source_trace_ids
                .iter()
                .any(|trace_id| trace_id == id)
        }) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Workflow trace is still referenced by a compiled candidate",
            ));
        }
        let trace = self
            .traces
            .remove(id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Workflow trace not found"))?;
        if let Err(error) = self.persist() {
            self.traces.insert(id.into(), trace.clone());
            return Err(error);
        }
        Ok(trace)
    }

    pub fn store_candidate(&mut self, candidate: Candidate) -> Result<()> {
        if !self.candidates.contains_key(&candidate.id) && self.candidates.len() >= MAX_TRACES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Workflow candidate budget exhausted",
            ));
        }
        let key = candidate.id.clone();
        let previous = self.candidates.insert(key.clone(), candidate);
        if let Err(error) = self.persist() {
            match previous {
                Some(previous) => {
                    self.candidates.insert(key, previous);
                }
                None => {
                    self.candidates.remove(&key);
                }
            }
            return Err(error);
        }
        Ok(())
    }
    pub fn candidate(&self, id: &str) -> Result<Candidate> {
        self.candidates
            .get(id)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Workflow candidate not found"))
    }

    pub fn delete_candidate(&mut self, id: &str) -> Result<Candidate> {
        if self
            .promotions
            .values()
            .any(|promotion| promotion.candidate.id == id)
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Workflow candidate is still promoted; demote it first",
            ));
        }
        let candidate = self
            .candidates
            .remove(id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Workflow candidate not found"))?;
        if let Err(error) = self.persist() {
            self.candidates.insert(id.into(), candidate.clone());
            return Err(error);
        }
        Ok(candidate)
    }

    pub fn mark_static_verified(&mut self, id: &str) -> Result<Candidate> {
        let previous = self
            .candidates
            .get(id)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Workflow candidate not found"))?;
        self.candidates
            .get_mut(id)
            .expect("candidate checked above")
            .static_verified = true;
        let out = self.candidates[id].clone();
        if let Err(error) = self.persist() {
            self.candidates.insert(id.into(), previous);
            return Err(error);
        }
        Ok(out)
    }

    pub fn mark_replay(&mut self, id: &str) -> Result<Candidate> {
        let previous = self
            .candidates
            .get(id)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Workflow candidate not found"))?;
        let candidate = self
            .candidates
            .get_mut(id)
            .expect("candidate checked above");
        candidate.successful_replays = candidate.successful_replays.saturating_add(1);
        let out = candidate.clone();
        if let Err(error) = self.persist() {
            self.candidates.insert(id.into(), previous);
            return Err(error);
        }
        Ok(out)
    }

    pub fn promote(
        &mut self,
        slug: &str,
        candidate_id: &str,
        capability: &str,
    ) -> Result<Promotion> {
        if !canonical_slug(slug) || self.promotions.contains_key(slug) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Promotion slug is invalid or already exists",
            ));
        }
        if self.promotions.len() >= MAX_PROMOTIONS {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Workflow promotion budget exhausted",
            ));
        }
        let candidate = self.candidate(candidate_id)?;
        if !candidate.static_verified || candidate.successful_replays == 0 {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Candidate must pass static verification and at least one successful replay",
            ));
        }
        let promotion = Promotion {
            version: 1,
            slug: slug.into(),
            capability: capability.into(),
            candidate,
            promoted_unix_ms: now_ms(),
        };
        self.promotions.insert(slug.into(), promotion.clone());
        if let Err(error) = self.persist() {
            self.promotions.remove(slug);
            return Err(error);
        }
        Ok(promotion)
    }

    pub fn demote(&mut self, slug: &str) -> Result<Promotion> {
        let promotion = self
            .promotions
            .remove(slug)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Promoted workflow not found"))?;
        if let Err(error) = self.persist() {
            self.promotions.insert(slug.into(), promotion.clone());
            return Err(error);
        }
        Ok(promotion)
    }

    pub fn promotions(&self) -> Vec<Promotion> {
        self.promotions.values().cloned().collect()
    }

    pub fn promotion_by_capability(&self, capability: &str) -> Result<Promotion> {
        self.promotions
            .values()
            .find(|p| p.capability == capability)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Promoted workflow unavailable"))
    }
}
