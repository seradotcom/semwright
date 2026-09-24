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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(kind: &str, id: u64) -> WindowsEvent {
        WindowsEvent {
            kind: kind.to_owned(),
            payload: json!({"id": id}),
        }
    }

    #[test]
    fn preserves_order_without_loss() {
        let mut queue = EventQueue::new(3);
        queue.push(event("semantic.text.changed", 1));
        queue.push(event("semantic.selection.changed", 2));
        let (invalidated, events) = queue.drain();
        assert!(!invalidated);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].payload["id"], 1);
        assert_eq!(events[1].payload["id"], 2);
    }

    #[test]
    fn overflow_discards_partial_history_and_requires_resync() {
        let mut queue = EventQueue::new(2);
        queue.push(event("semantic.text.changed", 1));
        queue.push(event("semantic.text.changed", 2));
        queue.push(event("semantic.text.changed", 3));

        let (invalidated, events) = queue.drain();
        assert!(invalidated);
        assert!(events.is_empty());

        queue.push(event("semantic.text.changed", 4));
        let (invalidated, events) = queue.drain();
        assert!(!invalidated);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload["id"], 4);
    }

    #[test]
    fn zero_capacity_is_clamped_to_one() {
        let mut queue = EventQueue::new(0);
        queue.push(event("semantic.object.changed", 1));
        queue.push(event("semantic.object.changed", 2));
        let (invalidated, events) = queue.drain();
        assert!(invalidated);
        assert!(events.is_empty());
    }
}
