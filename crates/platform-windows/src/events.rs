use serde_json::Value;
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct WindowsEvent {
    pub kind: String,
    pub payload: Value,
}

/// UIA events are invalidation hints, never authorization facts.
/// Overflow invalidates all cached assumptions instead of silently losing detail.
pub struct EventQueue {
    queue: VecDeque<WindowsEvent>,
    capacity: usize,
    invalidated: bool,
}

impl EventQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(capacity.min(4096)),
            capacity: capacity.clamp(1, 4096),
            invalidated: false,
        }
    }

    pub fn push(&mut self, event: WindowsEvent) {
        if self.queue.len() == self.capacity {
            self.queue.clear();
            self.invalidated = true;
            return;
        }
        self.queue.push_back(event);
    }

    pub fn drain(&mut self) -> (bool, Vec<WindowsEvent>) {
        let invalidated = std::mem::take(&mut self.invalidated);
        (invalidated, self.queue.drain(..).collect())
    }
}
