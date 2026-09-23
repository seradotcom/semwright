use crate::{
    Fault, FaultKind, Result, bounds, lifecycle::Operations, refs::Registry, state::Stamp,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    time::{SystemTime, UNIX_EPOCH},
};

pub const SUBSCRIPTIONS: u32 = 1535;
pub const FAMILIES: &[(&str, u32)] = &[
    ("General", 1),
    ("Config", 2),
    ("Scenes", 4),
    ("Inputs", 8),
    ("Transitions", 16),
    ("Filters", 32),
    ("Outputs", 64),
    ("SceneItems", 128),
    ("MediaInputs", 256),
    ("Vendors", 512),
    ("Ui", 1024),
    ("Canvases", 2048),
];

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub event_type: String,
    pub intent: u32,
    pub generation: u64,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub subject: Option<String>,
    pub resolution: &'static str,
    pub untrusted: bool,
    pub payload: Value,
}

#[derive(Default, Debug, Clone, Serialize)]
pub struct Metrics {
    pub received: u64,
    pub dropped: u64,
    pub malformed: u64,
    pub old_generation: u64,
    pub unknown_responses: u64,
    pub timeouts: u64,
}

pub struct EventEngine {
    queue: VecDeque<Event>,
    capacity: usize,
    pub metrics: Metrics,
    pub cache_stale: bool,
    sequence: u64,
}
impl EventEngine {
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            capacity: capacity.clamp(1, 512),
            metrics: Metrics::default(),
            cache_stale: true,
            sequence: 0,
        }
    }
    pub fn disconnected(
        &mut self,
        stamp: &mut Stamp,
        refs: &mut Registry,
        jobs: &mut Operations,
    ) -> Result<()> {
        self.queue.clear();
        self.cache_stale = true;
        refs.invalidate();
        jobs.loss();
        stamp.invalidate()
    }
    pub fn ingest(
        &mut self,
        data: &Value,
        generation: u64,
        stamp: &mut Stamp,
        refs: &mut Registry,
        jobs: &mut Operations,
    ) -> Result<()> {
        if generation != stamp.generation {
            self.metrics.old_generation += 1;
            return Ok(());
        }
        let parsed = (|| -> Result<(&str, u32, &Value)> {
            bounds::check(data, bounds::MAX_EVENT)?;
            let typ = data["eventType"]
                .as_str()
                .filter(|s| {
                    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric())
                })
                .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
            let intent = data["eventIntent"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
            let payload = data
                .get("eventData")
                .filter(|v| v.is_object())
                .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
            Ok((typ, intent, payload))
        })();
        let (typ, intent, payload) = match parsed {
            Ok(value) => value,
            Err(error) => {
                self.metrics.malformed += 1;
                self.cache_stale = true;
                refs.invalidate();
                jobs.loss();
                stamp.invalidate()?;
                return Err(error);
            }
        };
        self.metrics.received += 1;
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| Fault::new(FaultKind::ResourceLimit))?;
        let lost = self.queue.len() == self.capacity;
        if lost {
            self.queue.pop_front();
            self.metrics.dropped += 1;
            jobs.loss();
        }
        let structural = !matches!(
            typ,
            "RecordStateChanged"
                | "StreamStateChanged"
                | "ReplayBufferStateChanged"
                | "VirtualcamStateChanged"
                | "ReplayBufferSaved"
        ) || intent != 64;
        if structural || lost {
            stamp.invalidate()?;
            refs.invalidate();
        }
        self.cache_stale = true;
        let output = match typ {
            "RecordStateChanged" => Some("record"),
            "StreamStateChanged" => Some("stream"),
            "ReplayBufferStateChanged" => Some("replay"),
            "VirtualcamStateChanged" => Some("virtual_camera"),
            _ => None,
        };
        if let Some(output) = output
            && let Some(state) = payload["outputState"].as_str()
        {
            jobs.event(output, state, generation, self.sequence);
        }
        let retained = if intent == 512 || typ == "VendorEvent" {
            json!({"omitted":true})
        } else {
            bounds::scrub(payload)
        };
        self.queue.push_back(Event {
            event_type: typ.into(),
            intent,
            generation,
            sequence: self.sequence,
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            subject: None,
            resolution: "unresolved",
            untrusted: true,
            payload: retained,
        });
        Ok(())
    }
    pub fn poll(&self, after: u64, limit: usize) -> Result<Value> {
        if limit == 0 || limit > 64 {
            return Err(Fault::new(FaultKind::Configuration));
        }
        let gap = self
            .queue
            .front()
            .is_some_and(|e| after.saturating_add(1) < e.sequence)
            || after > self.sequence;
        let events: Vec<_> = self
            .queue
            .iter()
            .filter(|e| e.sequence > after)
            .take(limit)
            .collect();
        let cursor = events
            .last()
            .map_or(after.min(self.sequence), |e| e.sequence);
        Ok(json!({
            "events":events,
            "cursor":cursor,
            "gap":gap,
            "dropped_total":self.metrics.dropped,
            "cache_stale":self.cache_stale
        }))
    }
    pub fn len(&self) -> usize {
        self.queue.len()
    }
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}
