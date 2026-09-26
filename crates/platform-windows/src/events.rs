use semwright_types::semantic_ui_event;
use serde_json::Value;
use std::collections::VecDeque;
use uiautomation::{events::UIEventType, types::UIProperty};

pub fn semantic_property_event(property: UIProperty) -> &'static str {
    match property {
        UIProperty::BoundingRectangle => semantic_ui_event::GEOMETRY_CHANGED,
        UIProperty::HasKeyboardFocus => semantic_ui_event::FOCUS_CHANGED,
        _ => semantic_ui_event::PROPERTY_CHANGED,
    }
}

pub fn semantic_event(event: UIEventType) -> (&'static str, bool) {
    match event {
        UIEventType::StructureChanged
        | UIEventType::LayoutInvalidated
        | UIEventType::Window_WindowOpened
        | UIEventType::Window_WindowClosed
        | UIEventType::HostedFragmentRootsInvalidated => {
            (semantic_ui_event::STRUCTURE_CHANGED, true)
        }
        UIEventType::SelectionItem_ElementAddedToSelection
        | UIEventType::SelectionItem_ElementRemovedFromSelection
        | UIEventType::SelectionItem_ElementSelected
        | UIEventType::Selection_Invalidated
        | UIEventType::Text_TextSelectionChanged => (semantic_ui_event::SELECTION_CHANGED, false),
        UIEventType::Text_TextChanged | UIEventType::TextEdit_TextChanged => {
            (semantic_ui_event::TEXT_CHANGED, false)
        }
        UIEventType::AutomationFocusChanged => (semantic_ui_event::FOCUS_CHANGED, false),
        UIEventType::AutomationPropertyChanged => (semantic_ui_event::PROPERTY_CHANGED, false),
        _ => (semantic_ui_event::OBJECT_CHANGED, false),
    }
}

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

    #[test]
    fn native_events_map_to_portable_semantics() {
        assert_eq!(
            semantic_event(UIEventType::StructureChanged),
            (semantic_ui_event::STRUCTURE_CHANGED, true)
        );
        assert_eq!(
            semantic_event(UIEventType::Text_TextChanged).0,
            semantic_ui_event::TEXT_CHANGED
        );
        assert_eq!(
            semantic_event(UIEventType::SelectionItem_ElementSelected).0,
            semantic_ui_event::SELECTION_CHANGED
        );
        assert_eq!(
            semantic_event(UIEventType::AutomationFocusChanged).0,
            semantic_ui_event::FOCUS_CHANGED
        );
        assert_eq!(
            semantic_property_event(UIProperty::BoundingRectangle),
            semantic_ui_event::GEOMETRY_CHANGED
        );
        assert_eq!(
            semantic_property_event(UIProperty::HasKeyboardFocus),
            semantic_ui_event::FOCUS_CHANGED
        );
        assert_eq!(
            semantic_property_event(UIProperty::Name),
            semantic_ui_event::PROPERTY_CHANGED
        );
    }
}
