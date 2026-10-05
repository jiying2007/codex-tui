//! Time-derived cache invalidation. Scan only after computation, never per UI poll.
use crate::planning::{AGING_AGE_MS, FRESH_AGE_MS, Freshness, WorkCardProjection};

#[derive(Clone, Copy)]
pub(super) struct ProjectionClock {
    evaluated_at: u64,
    expires_at: Option<u64>,
}
impl ProjectionClock {
    pub(super) fn capture(cards: &[WorkCardProjection], now: u64) -> Self {
        let mut expires_at = None;
        let mut include = |deadline: u64| {
            if deadline > now {
                expires_at = Some(expires_at.map_or(deadline, |old: u64| old.min(deadline)));
            }
        };
        for card in cards {
            if let Some(until) = card.overlay.snooze_until_unix_ms {
                include(until);
            }
            for provenance in &card.provenance {
                // Locally authored data is not an expiring remote observation.
                if provenance.source == "local" {
                    continue;
                }
                if let Some(observed) = provenance.observed_at_unix_ms {
                    let age_limit = match provenance.freshness {
                        Freshness::Fresh => FRESH_AGE_MS,
                        Freshness::Aging => AGING_AGE_MS,
                        Freshness::Stale | Freshness::Unavailable => continue,
                    };
                    if let Some(deadline) = observed
                        .checked_add(age_limit)
                        .and_then(|at| at.checked_add(1))
                    {
                        include(deadline);
                    }
                }
            }
        }
        Self {
            evaluated_at: now,
            expires_at,
        }
    }
    pub(super) fn expired(self, now: u64) -> bool {
        // A wall-clock rollback may reactivate a snooze or make provenance fresh.
        now < self.evaluated_at || self.expires_at.is_some_and(|at| now >= at)
    }
}
