//! Mission Control reads native observations, not the asynchronously built Board.
use super::{AppState, SourceKind, ThreadSummary};
use crate::session_attention::{SessionAttentionReason as R, observed_reasons};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Included};

#[derive(Clone, Debug, Default)]
pub(super) struct LegacyAttentionPolicy {
    // Transitional, read-only index of the two existing local preferences.
    // No second task state or persisted schema is introduced.
    overlays: BTreeMap<String, (Option<u64>, bool)>,
    deadlines: BTreeSet<u64>,
    now: u64,
}

impl AppState {
    pub(super) fn refresh_session_attention_policy(&mut self) {
        let policy = &mut self.session_attention_policy;
        policy.overlays.clear();
        policy.deadlines.clear();
        for card in &self.planning_snapshot.cards {
            if card.anchor.kind != SourceKind::CodexThread {
                continue;
            }
            let until = card.overlay.snooze_until_unix_ms;
            let done = card.overlay.done_at_unix_ms.is_some();
            if until.is_some() || done {
                policy.overlays.insert(card.anchor.value.clone(), (until, done));
                if let Some(until) = until {
                    policy.deadlines.insert(until);
                }
            } else {
                policy.overlays.remove(&card.anchor.value);
            }
        }
    }

    /// Use the existing event loop's clock; no new timer or worker.
    pub fn advance_session_attention_clock(&mut self, now: u64) -> bool {
        let policy = &mut self.session_attention_policy;
        let previous = policy.now;
        policy.now = now;
        let (lo, hi) = if previous < now {
            (previous, now)
        } else {
            (now, previous)
        };
        lo != hi
            && policy
                .deadlines
                .range((Excluded(lo), Included(hi)))
                .next()
                .is_some()
    }

    pub fn session_attention_snoozed(&self, thread: &ThreadSummary) -> bool {
        self.session_attention_policy
            .overlays
            .get(&thread.id.0)
            .and_then(|(until, _)| *until)
            .is_some_and(|until| until > self.session_attention_policy.now)
    }

    pub fn session_attention_reasons(&self, thread: &ThreadSummary) -> BTreeSet<R> {
        let git = self
            .git_context(&thread.id)
            .filter(|context| context.cwd == thread.metadata.cwd && context.error.is_none());
        let forge = self
            .forge_observation(&thread.id)
            .filter(|observation| observation.cwd == thread.metadata.cwd);
        let collisions = if git.is_some() {
            self.worktree_collision_count(&thread.id)
        } else {
            0
        };
        let done = self
            .session_attention_policy
            .overlays
            .get(&thread.id.0)
            .is_some_and(|(_, done)| *done);
        observed_reasons(
            thread,
            git,
            collisions,
            self.goals.get(&thread.id.0),
            forge,
            done,
        )
    }

    pub fn thread_needs_attention(&self, index: usize) -> bool {
        let Some(thread) = self.threads.get(index) else {
            return false;
        };
        if self.pending_request_threads.contains(&thread.id.0)
            || thread.attention.iter().any(|reason| {
                matches!(
                    reason,
                    crate::domain::AttentionReason::ApprovalRequired
                        | crate::domain::AttentionReason::UserInputRequired
                        | crate::domain::AttentionReason::SystemError
                )
            })
        {
            return true;
        }
        if self.session_attention_snoozed(thread) {
            return false;
        }
        self.session_attention_reasons(thread).iter().any(|reason| {
            !matches!(reason, R::MarkedUnread | R::ReviewUnseen)
                || !self.acknowledged_attention.contains(&thread.id.0)
        })
    }
}
