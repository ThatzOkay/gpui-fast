use gpui::{Pixels, Point, TouchEvent, TouchId, TouchPhase};
use std::collections::BTreeMap;

struct Contact<W> {
    target: W,
    id: TouchId,
    position: Point<Pixels>,
}

pub(crate) struct Contacts<W> {
    next_id: u64,
    active: BTreeMap<i32, Contact<W>>,
    pending: Vec<(W, TouchEvent)>,
}

impl<W> Default for Contacts<W> {
    fn default() -> Self {
        Self {
            next_id: 0,
            active: BTreeMap::new(),
            pending: Vec::new(),
        }
    }
}

impl<W: Clone> Contacts<W> {
    fn queue(&mut self, contact: &Contact<W>, phase: TouchPhase) {
        self.pending.push((
            contact.target.clone(),
            TouchEvent {
                id: contact.id,
                phase,
                position: contact.position,
                predicted_position: None,
                force: None,
            },
        ));
    }

    pub fn down(&mut self, raw_id: i32, target: W, position: Point<Pixels>) {
        if let Some(previous) = self.active.remove(&raw_id) {
            self.queue(&previous, TouchPhase::Cancelled);
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("Wayland touch identifier exhausted");
        let contact = Contact {
            target,
            id: TouchId(self.next_id),
            position,
        };
        self.queue(&contact, TouchPhase::Started);
        self.active.insert(raw_id, contact);
    }

    pub fn motion(&mut self, raw_id: i32, position: Point<Pixels>) {
        let Some(contact) = self.active.get_mut(&raw_id) else {
            return;
        };
        contact.position = position;
        self.pending.push((
            contact.target.clone(),
            TouchEvent {
                id: contact.id,
                phase: TouchPhase::Moved,
                position,
                predicted_position: None,
                force: None,
            },
        ));
    }

    pub fn up(&mut self, raw_id: i32) {
        if let Some(contact) = self.active.remove(&raw_id) {
            self.queue(&contact, TouchPhase::Ended);
        }
    }

    pub fn cancel(&mut self) {
        for (_, contact) in std::mem::take(&mut self.active) {
            self.queue(&contact, TouchPhase::Cancelled);
        }
    }

    pub fn forget_where(&mut self, mut matches: impl FnMut(&W) -> bool) {
        self.active.retain(|_, contact| !matches(&contact.target));
        self.pending.retain(|(target, _)| !matches(target));
    }

    pub fn frame(&mut self) -> Vec<(W, TouchEvent)> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::Contacts;
    use gpui::{TouchPhase, point, px};

    #[test]
    fn preserves_target_position_and_order_until_frame() {
        let mut contacts = Contacts::default();
        contacts.down(7, "first window", point(px(12.0), px(20.0)));
        contacts.motion(7, point(px(16.0), px(24.0)));
        contacts.up(7);
        let events = contacts.frame();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].0, "first window");
        assert!(matches!(events[0].1.phase, TouchPhase::Started));
        assert!(matches!(events[1].1.phase, TouchPhase::Moved));
        assert!(matches!(events[2].1.phase, TouchPhase::Ended));
        assert_eq!(events[0].1.id, events[2].1.id);
        assert_eq!(events[2].1.position, point(px(16.0), px(24.0)));
        assert!(contacts.frame().is_empty());
    }

    #[test]
    fn protocol_id_reuse_gets_a_new_gpui_id() {
        let mut contacts = Contacts::default();
        contacts.down(0, (), point(px(0.0), px(0.0)));
        contacts.up(0);
        let first = contacts.frame()[0].1.id;
        contacts.down(0, (), point(px(0.0), px(0.0)));
        assert_ne!(first, contacts.frame()[0].1.id);
    }

    #[test]
    fn cancels_all_fingers_without_committing_a_tap() {
        let mut contacts = Contacts::default();
        contacts.down(0, "one", point(px(0.0), px(0.0)));
        contacts.down(1, "two", point(px(10.0), px(10.0)));
        contacts.frame();
        contacts.cancel();
        contacts.up(0);
        contacts.motion(1, point(px(20.0), px(20.0)));
        let events = contacts.frame();
        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .all(|(_, event)| matches!(event.phase, TouchPhase::Cancelled))
        );
        assert_ne!(events[0].1.id, events[1].1.id);
    }

    #[test]
    fn closed_window_does_not_receive_queued_or_later_events() {
        let mut contacts = Contacts::default();
        contacts.down(0, "closed", point(px(0.0), px(0.0)));
        contacts.down(1, "open", point(px(0.0), px(0.0)));
        contacts.forget_where(|window| *window == "closed");
        contacts.up(0);
        let events = contacts.frame();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "open");
    }
}
