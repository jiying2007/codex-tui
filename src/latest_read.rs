//! Ephemeral latest-accepted-read fencing; never use this for mutations.
//! Tokens travel through both bounded queues so buffered results are checked at
//! consumption, not just worker completion. The weak index retains no payloads.
use anyhow::{Result, anyhow};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::mpsc;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ReadScope {
    Snapshot(String),
    Review(String),
}

#[derive(Default)]
pub(crate) struct ReadFence {
    latest: Mutex<BTreeMap<ReadScope, Weak<AtomicBool>>>,
}

#[derive(Debug)]
pub(crate) struct ReadTicket(Arc<AtomicBool>);

impl ReadTicket {
    pub(crate) fn is_current(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
pub(crate) struct ReadEnvelope<T> {
    pub(crate) value: T,
    pub(crate) ticket: ReadTicket,
}

impl ReadFence {
    pub(crate) fn submit<T>(
        &self,
        sender: &mpsc::Sender<ReadEnvelope<T>>,
        scope: ReadScope,
        value: T,
        actor: &str,
    ) -> Result<()> {
        // Admission must succeed before superseding an earlier accepted read.
        let permit = sender.try_reserve().map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => anyhow!("{actor} actor queue is full"),
            mpsc::error::TrySendError::Closed(_) => anyhow!("{actor} actor is not available"),
        })?;
        let mut latest = self
            .latest
            .try_lock()
            .map_err(|_| anyhow!("{actor} read admission is busy or unavailable"))?;
        latest.retain(|_, ticket| ticket.strong_count() > 0);
        let ticket = ReadTicket(Arc::new(AtomicBool::new(true)));
        if let Some(previous) = latest
            .insert(scope, Arc::downgrade(&ticket.0))
            .and_then(|previous| previous.upgrade())
        {
            previous.store(false, Ordering::Release);
        }
        permit.send(ReadEnvelope { value, ticket });
        Ok(())
    }
}

pub(crate) fn next_current<T>(
    receiver: &mut mpsc::Receiver<ReadEnvelope<T>>,
    budget: usize,
) -> Option<T> {
    // A concurrent producer cannot make one UI drain spin without a bound.
    for _ in 0..budget {
        let envelope = receiver.try_recv().ok()?;
        if envelope.ticket.is_current() {
            return Some(envelope.value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> ReadScope {
        ReadScope::Snapshot("/repo".into())
    }

    #[test]
    fn buffered_and_late_old_results_are_both_rejected() {
        let fence = ReadFence::default();
        let (tx, mut commands) = mpsc::channel(4);
        let (events, mut results) = mpsc::channel(4);
        fence.submit(&tx, scope(), 1, "test").unwrap();
        let old = commands.try_recv().unwrap();
        events.try_send(old).unwrap();
        fence.submit(&tx, scope(), 2, "test").unwrap();
        let fresh = commands.try_recv().unwrap();
        assert_eq!(next_current(&mut results, 4), None);
        events.try_send(fresh).unwrap();
        assert_eq!(next_current(&mut results, 4), Some(2));
        fence.submit(&tx, scope(), 3, "test").unwrap();
        let slow = commands.try_recv().unwrap();
        fence.submit(&tx, scope(), 4, "test").unwrap();
        events.try_send(commands.try_recv().unwrap()).unwrap();
        assert_eq!(next_current(&mut results, 4), Some(4));
        events.try_send(slow).unwrap();
        assert_eq!(next_current(&mut results, 4), None);
    }

    #[test]
    fn full_closed_and_busy_admission_do_not_retire_an_accepted_read() {
        let fence = ReadFence::default();
        let (tx, mut rx) = mpsc::channel(1);
        fence.submit(&tx, scope(), 1, "test").unwrap();
        let accepted = rx.try_recv().unwrap();
        fence
            .submit(&tx, ReadScope::Snapshot("/other".into()), 2, "test")
            .unwrap();
        assert!(fence.submit(&tx, scope(), 3, "test").is_err());
        assert!(accepted.ticket.is_current());
        drop(rx.try_recv().unwrap());
        let guard = fence.latest.lock().unwrap();
        assert!(fence.submit(&tx, scope(), 4, "test").is_err());
        drop(guard);
        assert!(accepted.ticket.is_current());
        drop(rx);
        assert!(fence.submit(&tx, scope(), 5, "test").is_err());
        assert!(accepted.ticket.is_current());
    }

    #[test]
    fn scopes_remain_independent_and_retired_keys_do_not_accumulate() {
        let fence = ReadFence::default();
        let (tx, mut rx) = mpsc::channel(2);
        fence.submit(&tx, scope(), 1, "test").unwrap();
        let snapshot = rx.try_recv().unwrap();
        fence
            .submit(&tx, ReadScope::Review("/repo".into()), 2, "test")
            .unwrap();
        drop(rx.try_recv().unwrap());
        for index in 0..1000 {
            fence
                .submit(
                    &tx,
                    ReadScope::Snapshot(format!("/other/{index}")),
                    index,
                    "test",
                )
                .unwrap();
            drop(rx.try_recv().unwrap());
            assert!(fence.latest.lock().unwrap().len() <= 2);
        }
        assert!(snapshot.ticket.is_current());
    }

    #[test]
    fn stale_drain_respects_budget_without_losing_a_current_result() {
        let fence = ReadFence::default();
        let (tx, mut rx) = mpsc::channel(4);
        for value in 1..=4 {
            fence.submit(&tx, scope(), value, "test").unwrap();
        }
        assert_eq!(next_current(&mut rx, 2), None);
        assert_eq!(rx.len(), 2);
        assert_eq!(next_current(&mut rx, 2), Some(4));
    }
}
