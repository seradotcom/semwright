use crate::{Fault, FaultKind, Result, bounds, state::Stamp};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Scene,
    SceneItem,
    Input,
    Filter,
    Transition,
    Output,
    Profile,
    SceneCollection,
    Media,
}
impl Kind {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Scene => "obs-scene",
            Self::SceneItem => "obs-scene-item",
            Self::Input => "obs-input",
            Self::Filter => "obs-filter",
            Self::Transition => "obs-transition",
            Self::Output => "obs-output",
            Self::Profile => "obs-profile",
            Self::SceneCollection => "obs-scene-collection",
            Self::Media => "obs-media",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub kind: Kind,
    pub uuid: Option<String>,
    pub name: String,
    pub parent_uuid: Option<String>,
    pub item_id: Option<u64>,
    pub fingerprint: String,
}
impl Identity {
    pub fn validate(&self) -> Result<()> {
        if self.name.len() > bounds::MAX_NAME || self.fingerprint.len() > 128 {
            return Err(Fault::new(FaultKind::ResourceLimit));
        }
        for id in [&self.uuid, &self.parent_uuid].into_iter().flatten() {
            Uuid::parse_str(id).map_err(|_| Fault::new(FaultKind::Protocol))?;
        }
        if self.kind == Kind::SceneItem
            && (self.parent_uuid.is_none() || self.item_id.is_none() || self.uuid.is_none())
        {
            return Err(Fault::new(FaultKind::Unsupported));
        }
        Ok(())
    }
    pub fn mutable(&self) -> bool {
        self.uuid.is_some() && bounds::name(&self.name).is_ok() && self.kind != Kind::Filter
    }
}

#[derive(Debug, Clone)]
struct Entry {
    owner: String,
    identity: Identity,
    stamp: Stamp,
    expires: Instant,
}

pub struct Registry {
    entries: BTreeMap<String, Entry>,
    capacity: usize,
    ttl: Duration,
}
impl Registry {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            entries: BTreeMap::new(),
            capacity: capacity.min(4096),
            ttl: ttl.min(Duration::from_secs(300)),
        }
    }
    pub fn insert(&mut self, owner: &str, identity: Identity, stamp: Stamp) -> Result<String> {
        identity.validate()?;
        let now = Instant::now();
        self.entries.retain(|_, value| value.expires > now);
        if owner.len() > 128 || self.entries.len() >= self.capacity {
            return Err(Fault::new(FaultKind::ResourceLimit));
        }
        let id = format!("{}:{}", identity.kind.prefix(), Uuid::new_v4().simple());
        self.entries.insert(
            id.clone(),
            Entry {
                owner: owner.into(),
                identity,
                stamp,
                expires: now + self.ttl,
            },
        );
        Ok(id)
    }
    pub fn resolve(&self, id: &str, owner: &str, kind: Kind, stamp: Stamp) -> Result<Identity> {
        parse(id)?;
        self.entries
            .get(id)
            .filter(|entry| {
                entry.owner == owner
                    && entry.stamp == stamp
                    && entry.identity.kind == kind
                    && entry.expires > Instant::now()
            })
            .map(|entry| entry.identity.clone())
            .ok_or_else(|| Fault::new(FaultKind::StaleReference))
    }
    pub fn invalidate(&mut self) {
        self.entries.clear();
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub fn parse(value: &str) -> Result<(&str, &str)> {
    let (kind, id) = value
        .split_once(':')
        .ok_or_else(|| Fault::new(FaultKind::StaleReference))?;
    if !kind.starts_with("obs-")
        || id.len() != 32
        || !id.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Fault::new(FaultKind::StaleReference));
    }
    Ok((kind, id))
}
