//! Process-local opaque references, never Semwright native `$ref` markers or array indices.
use crate::{Error, Result};
use std::collections::BTreeMap;
use uuid::Uuid;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub project: String,
    pub revision: String,
    pub kind: String,
    pub id: String,
}
pub struct RefStore {
    entries: BTreeMap<String, Target>,
    index: BTreeMap<(String, String, String, String), String>,
    capacity: usize,
}
impl Default for RefStore {
    fn default() -> Self {
        Self::new(16384)
    }
}
impl RefStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            index: BTreeMap::new(),
            capacity,
        }
    }
    pub fn issue(&mut self, project: &str, revision: &str, kind: &str, id: &str) -> Result<String> {
        if ![
            "project",
            "sequence",
            "track",
            "clip",
            "asset",
            "transition",
            "effect",
            "keyframe",
            "marker",
        ]
        .contains(&kind)
        {
            return Err(Error::invalid("Unknown reference kind"));
        }
        let key = (project.into(), revision.into(), kind.into(), id.into());
        if let Some(value) = self.index.get(&key) {
            return Ok(value.clone());
        }
        if self.entries.len() >= self.capacity {
            return Err(Error::limit(
                "Reference capacity exhausted; close unused projects",
            ));
        }
        let token = format!("video:{}", Uuid::new_v4().simple());
        self.entries.insert(
            token.clone(),
            Target {
                project: project.into(),
                revision: revision.into(),
                kind: kind.into(),
                id: id.into(),
            },
        );
        self.index.insert(key, token.clone());
        Ok(token)
    }
    pub fn decode(token: &str) -> Result<()> {
        let s = token.strip_prefix("video:").ok_or_else(Error::stale)?;
        if s.len() != 32
            || !s
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::stale());
        }
        Ok(())
    }
    pub fn get(&self, token: &str, kind: &str) -> Result<Target> {
        Self::decode(token)?;
        self.entries
            .get(token)
            .filter(|r| r.kind == kind)
            .cloned()
            .ok_or_else(Error::stale)
    }
    pub fn resolve(
        &self,
        token: &str,
        project: &str,
        revision: &str,
        kind: &str,
    ) -> Result<String> {
        let t = self.get(token, kind)?;
        if t.project != project || t.revision != revision {
            return Err(Error::stale());
        }
        Ok(t.id)
    }
    pub fn invalidate(&mut self, project: &str) {
        self.entries.retain(|_, t| t.project != project);
        self.index.retain(|k, _| k.0 != project);
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
