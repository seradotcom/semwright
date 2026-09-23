use crate::{Fault, FaultKind, Result};
use serde::Serialize;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Starting,
    Active,
    Paused,
    Stopping,
    Stopped,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Operation {
    pub id: String,
    pub output: String,
    pub generation: u64,
    pub accepted: bool,
    pub phase: Phase,
    pub last_event_sequence: u64,
    pub outcome_known: bool,
}

#[derive(Default)]
pub struct Operations {
    records: VecDeque<Operation>,
    next: u64,
}
impl Operations {
    pub fn begin(&mut self, output: &str, start: bool, generation: u64) -> Result<String> {
        if !["record", "stream", "replay", "virtual_camera"].contains(&output) {
            return Err(Fault::new(FaultKind::Configuration));
        }
        if self
            .records
            .iter()
            .rev()
            .find(|o| o.output == output && o.generation == generation)
            .is_some_and(|o| matches!(o.phase, Phase::Starting | Phase::Stopping))
        {
            return Err(Fault::new(FaultKind::Precondition));
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| Fault::new(FaultKind::ResourceLimit))?;
        let id = format!("op:{generation}:{}", self.next);
        if self.records.len() == 64 {
            self.records.pop_front();
        }
        self.records.push_back(Operation {
            id: id.clone(),
            output: output.into(),
            generation,
            accepted: false,
            phase: if start {
                Phase::Starting
            } else {
                Phase::Stopping
            },
            last_event_sequence: 0,
            outcome_known: false,
        });
        Ok(id)
    }

    pub fn acceptance(&mut self, id: &str, result: &Result<serde_json::Value>) {
        if let Some(o) = self.records.iter_mut().find(|o| o.id == id) {
            match result {
                Ok(_) => o.accepted = true,
                Err(e) => {
                    o.outcome_known = e.outcome_known;
                    o.phase = if e.outcome_known {
                        Phase::Failed
                    } else {
                        Phase::Unknown
                    };
                }
            }
        }
    }

    pub fn event(&mut self, output: &str, state: &str, generation: u64, sequence: u64) {
        let phase = match state {
            "OBS_WEBSOCKET_OUTPUT_STARTING" => Phase::Starting,
            "OBS_WEBSOCKET_OUTPUT_STARTED"
            | "OBS_WEBSOCKET_OUTPUT_RECONNECTED"
            | "OBS_WEBSOCKET_OUTPUT_RESUMED" => Phase::Active,
            "OBS_WEBSOCKET_OUTPUT_PAUSED" => Phase::Paused,
            "OBS_WEBSOCKET_OUTPUT_STOPPING" => Phase::Stopping,
            "OBS_WEBSOCKET_OUTPUT_STOPPED" => Phase::Stopped,
            _ => Phase::Unknown,
        };
        if let Some(o) = self
            .records
            .iter_mut()
            .rev()
            .find(|o| o.output == output && o.generation == generation)
            && sequence > o.last_event_sequence
        {
            o.phase = phase;
            o.last_event_sequence = sequence;
            o.outcome_known = phase != Phase::Unknown;
        }
    }

    pub fn loss(&mut self) {
        for o in &mut self.records {
            o.phase = Phase::Unknown;
            o.outcome_known = false;
        }
    }

    pub fn get(&self, id: &str) -> Result<Operation> {
        self.records
            .iter()
            .find(|o| o.id == id)
            .cloned()
            .ok_or_else(|| Fault::new(FaultKind::StaleReference))
    }

    pub fn snapshot(&self) -> Vec<Operation> {
        self.records.iter().cloned().collect()
    }
}
