//! Mission Control reads native observations, not the asynchronously built Board.
use super::{AppState, ThreadSummary};
use crate::session_attention::{SessionAttentionReason as R, observed_reasons};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Included};

#[derive(Clone, Debug, Default)]
pub(super) struct SessionAttentionPolicy {
    scope: Option<String>,
    // Only current scope preferences participate in native Attention.
    overlays: BTreeMap<String, (Option<u64>, bool)>,
    deadlines: BTreeSet<u64>,
    notes: BTreeMap<String, String>,
    now: u64,
}

impl AppState {
    // Keep collision facts current on registry/Git events, even if the Board
    // worker is stopped or an old projection is still awaiting admission.
    pub(super) fn refresh_session_collisions(&mut self) {
        let active = super::active_worktree_counts(self);
        self.worktree_collision_counts = self
            .git_contexts
            .keys()
            .filter_map(|id| {
                let thread = self.thread_by_id(&super::ThreadId::new(id.clone()))?;
                let count = super::collision_count_from_active_worktrees(self, thread, &active);
                Some((id.clone(), count))
            })
            .collect();
    }

    pub(super) fn sync_session_attention_scope(&mut self) {
        if self.session_attention_policy.scope.as_deref() != self.shortcut_scope() {
            // An asynchronous cache receipt from the previous scope is never
            // a search result or a navigation authority in the next scope.
            self.transcript_search_results = None;
            self.transcript_search_active_hit = None;
            self.transcript_search_selected = 0;
            self.transcript_search_loading = false;
            self.transcript_search_query.clear();
            self.transcript_search_error = None;
            self.refresh_session_attention_policy();
            super::ensure_selection_visible(self);
        }
    }

    pub(super) fn refresh_session_attention_policy(&mut self) {
        let scope = self.shortcut_scope().map(str::to_owned);
        let policy = &mut self.session_attention_policy;
        policy.scope = scope;
        policy.overlays.clear();
        policy.deadlines.clear();
        policy.notes.clear();
        let Some(scope) = policy.scope.as_deref() else {
            return;
        };
        for card in &self.planning_snapshot.cards {
            let Some(thread) = card.anchor.thread_in_scope(scope) else {
                continue;
            };
            if scope.is_empty()
                && let Some(note) = &card.overlay.note
            {
                policy.notes.insert(thread.to_owned(), note.clone());
            }
            let until = card.overlay.snooze_until_unix_ms;
            let done = scope.is_empty() && card.overlay.done_at_unix_ms.is_some();
            if until.is_some() || done {
                policy.overlays.insert(thread.to_owned(), (until, done));
                if let Some(until) = until {
                    policy.deadlines.insert(until);
                }
            } else {
                policy.overlays.remove(thread);
            }
        }
        for note in &self.planning_snapshot.notes {
            if let Some(thread) = note.owner.thread_in_scope(scope) {
                policy.notes.insert(thread.to_owned(), note.text.clone());
            }
        }
    }

    pub fn session_note(&self, thread_id: &super::ThreadId) -> Option<&str> {
        self.session_attention_policy
            .notes
            .get(&thread_id.0)
            .map(String::as_str)
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
