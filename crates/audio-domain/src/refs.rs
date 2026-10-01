//! Process-local opaque audio references.

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
        Self::new(16_384)
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
            "sample",
            "synth",
            "signal",
            "stem",
            "clip",
            "effect",
            "automation",
            "bus",
        ]
        .contains(&kind)
        {
            return Err(Error::invalid("Unknown audio reference kind"));
        }
        let key = (project.into(), revision.into(), kind.into(), id.into());
        if let Some(token) = self.index.get(&key) {
            return Ok(token.clone());
        }
        if self.entries.len() >= self.capacity {
            return Err(Error::limit("Audio reference capacity exhausted"));
        }
        let token = format!("audio:{}", Uuid::new_v4().simple());
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
        let suffix = token.strip_prefix("audio:").ok_or_else(Error::stale)?;
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::stale());
        }
        Ok(())
    }
    pub fn resolve(
        &self,
        token: &str,
        project: &str,
        revision: &str,
        kind: &str,
    ) -> Result<String> {
        Self::decode(token)?;
        let target = self
            .entries
            .get(token)
            .filter(|target| target.kind == kind)
            .ok_or_else(Error::stale)?;
        if target.project != project || target.revision != revision {
            return Err(Error::stale());
        }
        Ok(target.id.clone())
    }
    pub fn invalidate(&mut self, project: &str) {
        self.entries.retain(|_, target| target.project != project);
        self.index.retain(|key, _| key.0 != project);
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
