use crate::{Fault, FaultKind, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Authenticating,
    Identified,
    Ready,
    Reconnecting,
    Closing,
    Closed,
    Failed,
}
impl ConnectionState {
    pub fn transition(&mut self, next: Self) -> Result<()> {
        use ConnectionState::*;
        let legal = matches!(
            (*self, next),
            (Disconnected, Connecting | Closing)
                | (Connecting, Authenticating | Reconnecting | Failed | Closing)
                | (Authenticating, Identified | Reconnecting | Failed | Closing)
                | (Identified, Ready | Reconnecting | Failed | Closing)
                | (Ready, Reconnecting | Closing | Failed)
                | (Reconnecting, Connecting | Closing | Failed)
                | (Failed, Closing)
                | (Closing, Closed)
                | (Closed, Closed)
        );
        if !legal {
            return Err(Fault::new(FaultKind::Protocol));
        }
        *self = next;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stamp {
    pub generation: u64,
    pub graph_revision: u64,
}
impl Stamp {
    pub fn next_connection(&mut self) -> Result<()> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| Fault::new(FaultKind::ResourceLimit))?;
        self.invalidate()
    }
    pub fn invalidate(&mut self) -> Result<()> {
        self.graph_revision = self
            .graph_revision
            .checked_add(1)
            .ok_or_else(|| Fault::new(FaultKind::ResourceLimit))?;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct RequestIds {
    counter: u64,
}
impl RequestIds {
    pub fn next(&mut self, generation: u64, batch: bool) -> Result<String> {
        self.counter = self
            .counter
            .checked_add(1)
            .ok_or_else(|| Fault::new(FaultKind::ResourceLimit))?;
        Ok(format!(
            "g{generation}:{}{}",
            if batch { "b" } else { "r" },
            self.counter
        ))
    }
}

pub struct ReconnectBudget {
    attempts: VecDeque<Instant>,
    limit: usize,
}
impl ReconnectBudget {
    pub fn new(limit: usize) -> Self {
        Self {
            attempts: VecDeque::new(),
            limit: limit.clamp(1, 5),
        }
    }
    pub fn reserve(&mut self, now: Instant) -> Result<Duration> {
        while self
            .attempts
            .front()
            .is_some_and(|x| now.saturating_duration_since(*x) >= Duration::from_secs(60))
        {
            self.attempts.pop_front();
        }
        if self.attempts.len() >= self.limit {
            return Err(Fault::new(FaultKind::ResourceLimit));
        }
        let delay = if self.attempts.is_empty() {
            0
        } else {
            100u64 << (self.attempts.len() - 1).min(4)
        };
        self.attempts.push_back(now);
        Ok(Duration::from_millis(delay.min(2000)))
    }
    pub fn retained(&self) -> usize {
        self.attempts.len()
    }
}
