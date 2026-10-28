//! Coalescing and scoping the ToolGuard allow-list fan-out (#180).
//!
//! `broadcast_toolguard_state` rebuilds and publishes the allow-list for every
//! registered device, at roughly `users × tools` access evaluations each, and
//! it was awaited inline by every swipe: a tool-on answered only after the
//! whole site had been republished, so a swipe took seconds and got slower
//! with every device added. The e2e saw it as a `journeys` stage of 25 to 54
//! minutes and an edge that could not fetch its wiring inside a minute.
//!
//! This module is the pure half of the fix and has no `crate::` imports, so it
//! is unit-tested without a database:
//!
//! - a [`Coalescer`] turns a burst of requests into one batch, published a
//!   bounded time after the FIRST request in it (not the last, so a steady
//!   stream cannot starve the publish);
//! - [`devices_for`] narrows a batch to the devices it concerns: a tool-on on
//!   tool T changes T's status for the devices bound to T and nobody else's
//!   allow-list, so only those are rebuilt; a change to who may use what
//!   (cards, training, a merge, a lockout) is everyone's.
//!
//! The runtime half -- one task draining the coalescer, building and
//! publishing payloads -- lives beside `broadcast_toolguard_state`, which now
//! only files a request and returns. The edge's periodic poll stays as the
//! safety net it always was.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use uuid::Uuid;

/// What a caller is asking to have republished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Every device: who may use what has changed.
    All,
    /// The devices bound to one tool: its status has changed.
    Tool(Uuid),
}

/// What the drain task publishes: the merge of every request in a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Batch {
    All,
    Tools(BTreeSet<Uuid>),
}

/// Requests merged into one pending batch, released a bounded time after the
/// first of them arrived.
#[derive(Debug)]
pub struct Coalescer {
    window: Duration,
    all: bool,
    tools: BTreeSet<Uuid>,
    /// When the first request of the pending batch arrived; `None` when
    /// nothing is pending.
    since: Option<Instant>,
}

impl Coalescer {
    pub fn new(window: Duration) -> Self {
        Coalescer {
            window,
            all: false,
            tools: BTreeSet::new(),
            since: None,
        }
    }

    /// File a request. Starts the window if nothing was pending; otherwise
    /// joins the batch already waiting, and the window does not move.
    pub fn note(&mut self, scope: Scope, now: Instant) {
        if self.since.is_none() {
            self.since = Some(now);
        }
        match scope {
            Scope::All => self.all = true,
            Scope::Tool(t) => {
                self.tools.insert(t);
            }
        }
    }

    /// When the pending batch is due, if one is pending.
    pub fn due_at(&self) -> Option<Instant> {
        self.since.map(|s| s + self.window)
    }

    /// The pending batch if its window has passed, leaving nothing pending.
    /// `All` absorbs every tool scope: one full fan-out already covers them.
    pub fn take(&mut self, now: Instant) -> Option<Batch> {
        if self.due_at().is_some_and(|due| now >= due) {
            self.take_now()
        } else {
            None
        }
    }

    /// The pending batch whatever the clock says: for shutdown, when there is
    /// no later.
    pub fn take_now(&mut self) -> Option<Batch> {
        self.since.take()?;
        let batch = if self.all {
            Batch::All
        } else {
            Batch::Tools(std::mem::take(&mut self.tools))
        };
        self.all = false;
        self.tools.clear();
        Some(batch)
    }
}

/// The devices a batch concerns, in a stable order with no repeats.
///
/// `bindings` is every `(device, resource)` binding on the site; `all_devices`
/// every approved device. A tool batch reaches the devices bound to any of its
/// tools -- the reader, the plugs, the sensor, the coordinating edge -- and no
/// device bound only elsewhere. A device bound to nothing receives nothing
/// from a tool batch, which is what it would have been sent: an empty
/// allow-list it already holds.
pub fn devices_for(batch: &Batch, all_devices: &[Uuid], bindings: &[(Uuid, Uuid)]) -> Vec<Uuid> {
    match batch {
        Batch::All => {
            let set: BTreeSet<Uuid> = all_devices.iter().copied().collect();
            set.into_iter().collect()
        }
        Batch::Tools(tools) => {
            let set: BTreeSet<Uuid> = bindings
                .iter()
                .filter(|(_, resource)| tools.contains(resource))
                .map(|(device, _)| *device)
                .collect();
            set.into_iter().collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }
    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }
    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn a_burst_inside_the_window_is_one_batch() {
        // Ten swipes on two tools in 90 ms: one publish, not ten.
        let mut c = Coalescer::new(ms(250));
        let start = t0();
        for i in 0..10u64 {
            c.note(Scope::Tool(id(u128::from(1 + i % 2))), start + ms(i * 10));
        }
        assert_eq!(c.take(start + ms(100)), None, "not due yet");
        assert_eq!(
            c.due_at(),
            Some(start + ms(250)),
            "due from the FIRST request"
        );
        let batch = c.take(start + ms(250)).expect("due");
        assert_eq!(batch, Batch::Tools([id(1), id(2)].into_iter().collect()));
        assert_eq!(c.take(start + ms(300)), None, "nothing left pending");
        assert_eq!(c.due_at(), None);
    }

    #[test]
    fn the_window_counts_from_the_first_request_not_the_last() {
        // A request every 100 ms for ever must still publish every 250 ms.
        let mut c = Coalescer::new(ms(250));
        let start = t0();
        c.note(Scope::Tool(id(1)), start);
        c.note(Scope::Tool(id(1)), start + ms(100));
        c.note(Scope::Tool(id(1)), start + ms(200));
        assert!(
            c.take(start + ms(250)).is_some(),
            "a steady stream starved the publish"
        );
    }

    #[test]
    fn all_absorbs_every_tool_scope() {
        let mut c = Coalescer::new(ms(250));
        let start = t0();
        c.note(Scope::Tool(id(1)), start);
        c.note(Scope::All, start + ms(10));
        c.note(Scope::Tool(id(2)), start + ms(20));
        assert_eq!(c.take(start + ms(250)), Some(Batch::All));
        // And the next window starts clean.
        c.note(Scope::Tool(id(3)), start + ms(300));
        assert_eq!(
            c.take(start + ms(550)),
            Some(Batch::Tools([id(3)].into_iter().collect()))
        );
    }

    #[test]
    fn take_now_hands_over_whatever_is_pending_for_shutdown() {
        let mut c = Coalescer::new(ms(250));
        assert_eq!(c.take_now(), None, "nothing pending, nothing to publish");
        c.note(Scope::Tool(id(1)), t0());
        assert_eq!(
            c.take_now(),
            Some(Batch::Tools([id(1)].into_iter().collect()))
        );
    }

    #[test]
    fn a_tool_batch_reaches_the_devices_bound_to_it_and_nobody_else() {
        let (laser, drill) = (id(0xA), id(0xB));
        let (reader, plug, sensor, edge, drill_plug, kiosk) =
            (id(1), id(2), id(3), id(4), id(5), id(6));
        let bindings = vec![
            (reader, laser),
            (plug, laser),
            (sensor, laser),
            (edge, laser),
            (edge, drill),
            (drill_plug, drill),
        ];
        let all = vec![reader, plug, sensor, edge, drill_plug, kiosk];

        let batch = Batch::Tools([laser].into_iter().collect());
        assert_eq!(
            devices_for(&batch, &all, &bindings),
            vec![reader, plug, sensor, edge],
            "the drill's plug and the unbound kiosk must not be rebuilt for a laser swipe"
        );
        // The edge is bound to both; it appears once.
        let both = Batch::Tools([laser, drill].into_iter().collect());
        assert_eq!(
            devices_for(&both, &all, &bindings),
            vec![reader, plug, sensor, edge, drill_plug]
        );
        assert_eq!(
            devices_for(&Batch::All, &all, &bindings),
            all,
            "everyone, kiosk included"
        );
    }
}
