use semwright_types::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    #[default]
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
    pub fn can_transition(self, next: Self) -> bool {
        use ConnectionState::*;
        matches!(
            (self, next),
            (Disconnected, Connecting | Closing)
                | (Connecting, Authenticating | Reconnecting | Failed | Closing)
                | (
                    Authenticating,
                    Identified | Ready | Reconnecting | Failed | Closing
                )
                | (Identified, Ready | Reconnecting | Failed | Closing)
                | (Ready, Reconnecting | Closing | Failed)
                | (Reconnecting, Connecting | Authenticating | Closing | Failed)
                | (Failed, Closing)
                | (Closing, Closed)
                | (Closed, Closed)
        )
    }

    pub fn transition(&mut self, next: Self) -> Result<()> {
        if !self.can_transition(next) {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Illegal driver continuity state transition",
            ));
        }
        *self = next;
        Ok(())
    }

    pub fn is_available(self) -> bool {
        matches!(self, Self::Ready)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuityStamp {
    pub generation: u64,
    pub revision: u64,
}

impl ContinuityStamp {
    pub fn next_connection(&mut self) -> Result<()> {
        self.generation = self.generation.checked_add(1).ok_or_else(|| {
            Error::new(ErrorCode::ResourceExhausted, "Driver generation exhausted")
        })?;
        self.invalidate()
    }

    pub fn invalidate(&mut self) -> Result<()> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Driver revision exhausted"))?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconnectPolicy {
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub max_attempts_per_window: usize,
    pub window_ms: u64,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            initial_delay_ms: 100,
            max_delay_ms: 2_000,
            max_attempts_per_window: 5,
            window_ms: 60_000,
        }
    }
}

impl ReconnectPolicy {
    pub fn validate(self) -> Result<Self> {
        if self.initial_delay_ms == 0
            || self.max_delay_ms < self.initial_delay_ms
            || self.max_delay_ms > 60_000
            || !(1..=64).contains(&self.max_attempts_per_window)
            || self.window_ms < self.max_delay_ms
            || self.window_ms > 3_600_000
        {
            return Err(Error::invalid("Driver reconnect policy exceeds bounds"));
        }
        Ok(self)
    }

    fn delay_for_attempt(self, attempt: usize) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        let shift = (attempt - 1).min(16) as u32;
        let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
        Duration::from_millis(
            self.initial_delay_ms
                .saturating_mul(factor)
                .min(self.max_delay_ms),
        )
    }
}

pub struct ReconnectBudget {
    policy: ReconnectPolicy,
    attempts: VecDeque<Instant>,
}

impl ReconnectBudget {
    pub fn new(policy: ReconnectPolicy) -> Result<Self> {
        Ok(Self {
            policy: policy.validate()?,
            attempts: VecDeque::new(),
        })
    }

    pub fn standard(limit: usize) -> Result<Self> {
        Self::new(ReconnectPolicy {
            max_attempts_per_window: limit,
            ..ReconnectPolicy::default()
        })
    }

    pub fn reserve(&mut self, now: Instant) -> Result<Duration> {
        let window = Duration::from_millis(self.policy.window_ms);
        while self
            .attempts
            .front()
            .is_some_and(|oldest| now.saturating_duration_since(*oldest) >= window)
        {
            self.attempts.pop_front();
        }
        if self.attempts.len() >= self.policy.max_attempts_per_window {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Driver reconnect budget exhausted",
            ));
        }
        let delay = self.policy.delay_for_attempt(self.attempts.len());
        self.attempts.push_back(now);
        Ok(delay)
    }

    pub fn reset(&mut self) {
        self.attempts.clear();
    }

    pub fn retained(&self) -> usize {
        self.attempts.len()
    }

    pub fn policy(&self) -> ReconnectPolicy {
        self.policy
    }
}

#[derive(Debug, Default)]
pub struct RequestIds {
    counter: u64,
}

impl RequestIds {
    pub fn next(&mut self, generation: u64, class: &str) -> Result<String> {
        if class.is_empty()
            || class.len() > 16
            || !class
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(Error::invalid("Driver request class is invalid"));
        }
        self.counter = self.counter.checked_add(1).ok_or_else(|| {
            Error::new(ErrorCode::ResourceExhausted, "Driver request IDs exhausted")
        })?;
        Ok(format!("g{generation}:{class}:{}", self.counter))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuitySnapshot {
    pub state: ConnectionState,
    pub generation: u64,
    pub revision: u64,
    pub reconnect_attempts: u64,
    pub pending_requests: usize,
}

impl ContinuitySnapshot {
    pub fn connected(
        generation: u64,
        revision: u64,
        reconnect_attempts: u64,
        pending_requests: usize,
    ) -> Self {
        Self {
            state: ConnectionState::Ready,
            generation,
            revision,
            reconnect_attempts,
            pending_requests,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine_rejects_skips_and_allows_reconnect() {
        let mut state = ConnectionState::Disconnected;
        for next in [
            ConnectionState::Connecting,
            ConnectionState::Authenticating,
            ConnectionState::Ready,
            ConnectionState::Reconnecting,
            ConnectionState::Connecting,
            ConnectionState::Authenticating,
            ConnectionState::Ready,
            ConnectionState::Closing,
            ConnectionState::Closed,
        ] {
            state.transition(next).unwrap();
        }
        assert!(state.transition(ConnectionState::Connecting).is_err());

        let mut invalid = ConnectionState::Connecting;
        assert!(invalid.transition(ConnectionState::Ready).is_err());
    }

    #[test]
    fn stamp_advances_generation_and_invalidates_revision() {
        let mut stamp = ContinuityStamp {
            generation: 7,
            revision: 11,
        };
        stamp.next_connection().unwrap();
        assert_eq!(
            stamp,
            ContinuityStamp {
                generation: 8,
                revision: 12
            }
        );
    }

    #[test]
    fn reconnect_budget_is_bounded_and_exponential() {
        let start = Instant::now();
        let mut budget = ReconnectBudget::new(ReconnectPolicy {
            initial_delay_ms: 100,
            max_delay_ms: 400,
            max_attempts_per_window: 4,
            window_ms: 1_000,
        })
        .unwrap();
        assert_eq!(budget.reserve(start).unwrap(), Duration::ZERO);
        assert_eq!(
            budget.reserve(start + Duration::from_millis(1)).unwrap(),
            Duration::from_millis(100)
        );
        assert_eq!(
            budget.reserve(start + Duration::from_millis(2)).unwrap(),
            Duration::from_millis(200)
        );
        assert_eq!(
            budget.reserve(start + Duration::from_millis(3)).unwrap(),
            Duration::from_millis(400)
        );
        assert!(budget.reserve(start + Duration::from_millis(4)).is_err());
        assert_eq!(
            budget
                .reserve(start + Duration::from_millis(1_001))
                .unwrap(),
            Duration::from_millis(200)
        );
    }

    #[test]
    fn request_ids_bind_generation_and_class() {
        let mut ids = RequestIds::default();
        assert_eq!(ids.next(9, "read").unwrap(), "g9:read:1");
        assert_eq!(ids.next(10, "mutation").unwrap(), "g10:mutation:2");
        assert!(ids.next(10, "bad-class").is_err());
    }
}
