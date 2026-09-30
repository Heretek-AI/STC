//! f06-meta-bus: tagged message bus schema for multi-pack teams.
//!
//! Clean-room port of the `cause_by` / `sent_from` / `send_to` envelope +
//! `MessageQueue` shape (c057
//! `f21ec5023b0bee58ff658af8db840cb5fd84e2bb5d1822035e9c134d3892d9f2`,
//! MIT). Field names are kept for envelope fidelity. Divergences: no
//! pydantic validators (typed constructors fail closed instead), no async
//! queue (a `VecDeque` with explicit `drain_for`), ids are content-derived
//! (`RolePack::lockfile_hash`) rather than uuid (deterministic, replayable).

use std::collections::VecDeque;

use super::RolePack;

/// Route-to-all marker (upstream `MESSAGE_ROUTE_TO_ALL`).
pub const ROUTE_TO_ALL: &str = "<all>";

/// Tagged inter-role message. `cause_by` names the action class that caused
/// it, `sent_from` the sender role, `send_to` the recipient roles
/// (`{ROUTE_TO_ALL}` = broadcast).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusMessage {
    pub id: String,
    pub content: String,
    pub role: String,
    pub cause_by: String,
    pub sent_from: String,
    pub send_to: Vec<String>,
}

impl BusMessage {
    /// Broadcast constructor: empty `send_to` normalizes to route-to-all
    /// (never an empty recipient set — an empty set would silently drop).
    pub fn new(content: &str, role: &str, cause_by: &str, sent_from: &str) -> Self {
        Self::addressed(
            content,
            role,
            cause_by,
            sent_from,
            vec![ROUTE_TO_ALL.into()],
        )
    }

    pub fn addressed(
        content: &str,
        role: &str,
        cause_by: &str,
        sent_from: &str,
        send_to: Vec<String>,
    ) -> Self {
        let send_to = if send_to.is_empty() {
            vec![ROUTE_TO_ALL.into()]
        } else {
            send_to
        };
        let id = RolePack::lockfile_hash(&format!(
            "{content}\0{role}\0{cause_by}\0{sent_from}\0{}",
            send_to.join(",")
        ));
        Self {
            id,
            content: content.into(),
            role: role.into(),
            cause_by: cause_by.into(),
            sent_from: sent_from.into(),
            send_to,
        }
    }

    pub fn is_broadcast(&self) -> bool {
        self.send_to.iter().any(|r| r == ROUTE_TO_ALL)
    }

    pub fn visible_to(&self, recipient: &str) -> bool {
        self.is_broadcast() || self.send_to.iter().any(|r| r == recipient)
    }
}

/// Ordered queue with recipient-scoped drains. A drain removes only messages
/// visible to that recipient; other recipients' mail is never consumed.
#[derive(Debug, Default)]
pub struct MessageQueue {
    queue: VecDeque<BusMessage>,
}

impl MessageQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&mut self, msg: BusMessage) {
        self.queue.push_back(msg);
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Remove and return every message visible to `recipient`, preserving
    /// order. Messages for others stay queued.
    pub fn drain_for(&mut self, recipient: &str) -> Vec<BusMessage> {
        let mut out = vec![];
        let mut rest = VecDeque::new();
        for msg in self.queue.drain(..) {
            if msg.visible_to(recipient) {
                out.push(msg);
            } else {
                rest.push_back(msg);
            }
        }
        self.queue = rest;
        out
    }

    /// Remove and return everything (operator drain).
    pub fn drain_all(&mut self) -> Vec<BusMessage> {
        self.queue.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_reaches_everyone_but_drains_per_recipient() {
        let mut q = MessageQueue::new();
        q.publish(BusMessage::new("hello", "user", "UserRequirement", "a"));
        q.publish(BusMessage::addressed(
            "secret",
            "assistant",
            "Review",
            "b",
            vec!["c".into()],
        ));
        // `b` sees only the broadcast; `c`'s mail stays queued.
        let b = q.drain_for("b");
        assert_eq!(b.len(), 1);
        assert_eq!(q.len(), 1);
        let c = q.drain_for("c");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].content, "secret");
        assert!(q.is_empty());
    }

    #[test]
    fn empty_recipients_normalize_to_broadcast_never_drop() {
        let m = BusMessage::addressed("x", "user", "C", "s", vec![]);
        assert!(m.is_broadcast());
        let mut q = MessageQueue::new();
        q.publish(m);
        assert_eq!(q.drain_for("anyone").len(), 1);
    }

    #[test]
    fn ids_are_content_derived_and_deterministic() {
        let a = BusMessage::new("hi", "user", "C", "s");
        let b = BusMessage::new("hi", "user", "C", "s");
        assert_eq!(a.id, b.id);
        assert!(!a.id.is_empty());
    }
}
