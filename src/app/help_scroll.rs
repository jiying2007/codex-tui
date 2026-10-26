//! Layout-derived help scroll bound, safely independent across cloned snapshots.
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub(crate) struct Limit(AtomicUsize);

impl Limit {
    pub(crate) fn new(value: usize) -> Self {
        Self(AtomicUsize::new(value))
    }

    pub(crate) fn get(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }

    pub(crate) fn set(&self, value: usize) {
        self.0.store(value, Ordering::Relaxed);
    }
}

impl Clone for Limit {
    fn clone(&self) -> Self {
        Self::new(self.get())
    }
}
