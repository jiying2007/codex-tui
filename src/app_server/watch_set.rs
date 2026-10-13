//! Desired subscriptions shared with the actor, independently of its bounded queue.
use super::*;

#[derive(Default)]
struct Membership {
    ids: BTreeSet<String>,
    closed: bool,
}

#[derive(Clone)]
pub(super) struct WatchSet {
    current: watch::Sender<Membership>,
}

impl Default for WatchSet {
    fn default() -> Self {
        Self {
            current: watch::channel(Membership::default()).0,
        }
    }
}

impl WatchSet {
    pub(super) fn admit(&self, id: &str, enqueue: impl FnOnce() -> Result<()>) -> Result<()> {
        let mut result = Ok(());
        // The actor's membership read is serialized with nonblocking admission.
        // No guard, channel borrow or user callback survives an await point.
        self.current.send_if_modified(|state| {
            if state.closed {
                result = Err(anyhow!("App Server actor is not available"));
                return false;
            }
            result = enqueue();
            result.is_ok() && state.ids.insert(id.to_owned())
        });
        result
    }

    pub(super) fn contains(&self, id: &str) -> bool {
        self.current.borrow().ids.contains(id)
    }

    pub(super) fn remove(&self, id: &str) -> bool {
        self.current.send_if_modified(|state| state.ids.remove(id))
    }

    fn close(&self) {
        self.current.send_modify(|state| {
            state.closed = true;
            state.ids.clear();
        });
    }
}

pub(super) struct WatchSetGuard(WatchSet, WatchSet);

impl WatchSetGuard {
    pub(super) fn new(history: WatchSet, queue: WatchSet) -> Self {
        Self(history, queue)
    }
}

impl Drop for WatchSetGuard {
    fn drop(&mut self) {
        self.0.close();
        self.1.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_does_not_need_a_queue_slot_or_retain_failed_admission() {
        let watches = WatchSet::default();
        let (tx, mut rx) = mpsc::channel(1);
        watches
            .admit("first", || tx.try_send(()).map_err(Into::into))
            .unwrap();
        assert!(watches.contains("first"));
        for i in 0..1000 {
            assert!(
                watches
                    .admit(&i.to_string(), || tx.try_send(()).map_err(Into::into))
                    .is_err()
            );
        }
        assert_eq!(watches.current.borrow().ids.len(), 1);
        watches.remove("first");
        assert!(watches.current.borrow().ids.is_empty());
        assert!(rx.try_recv().is_ok());
        // Consuming the old queued read cannot restore its former membership.
        assert!(!watches.contains("first"));
        watches
            .admit("first", || tx.try_send(()).map_err(Into::into))
            .unwrap();
        assert!(watches.contains("first"));
        drop(rx);
        assert!(
            watches
                .admit("other", || tx.try_send(()).map_err(Into::into))
                .is_err()
        );
        assert_eq!(watches.current.borrow().ids.len(), 1);
    }

    #[test]
    fn refused_refresh_preserves_an_existing_owner_and_actor_drop_clears_both_sets() {
        let history = WatchSet::default();
        let queue = WatchSet::default();
        history.admit("same", || Ok(())).unwrap();
        queue.admit("same", || Ok(())).unwrap();
        assert!(history.admit("same", || Err(anyhow!("full"))).is_err());
        assert!(history.contains("same"));
        history.remove("same");
        assert!(queue.contains("same"));
        let guard = WatchSetGuard::new(history.clone(), queue.clone());
        history.admit("again", || Ok(())).unwrap();
        drop(guard);
        assert!(history.current.borrow().ids.is_empty());
        assert!(queue.current.borrow().ids.is_empty());
        assert!(
            history
                .admit("late", || panic!("closed actor must not enqueue"))
                .is_err()
        );
    }
}
