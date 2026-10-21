from lean_patch_support import replace, between, write

write('src/attention.rs', r'''//! Pure attention policy shared by registry presentation and scoped snapshots.
//! A source that was not observed is not evidence that it is healthy.
use crate::{domain::{AttentionReason, RuntimeStatus, ThreadSummary}, planning::{PlanningAttention, WorkCardProjection}};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AttentionLevel { Interaction, Blocked, Review, Unread }

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AttentionSignal {
    pub level: AttentionLevel,
    pub reason: &'static str,
}

impl AttentionSignal {
    pub const fn urgent(self) -> bool {
        matches!(self.level, AttentionLevel::Interaction | AttentionLevel::Blocked)
    }
}

pub fn planning_signal(reason: &PlanningAttention) -> AttentionSignal {
    use AttentionLevel::*;
    let level = match reason {
        PlanningAttention::ApprovalRequired | PlanningAttention::UserInputRequired => Interaction,
        PlanningAttention::SystemError | PlanningAttention::ConflictRisk | PlanningAttention::GoalBlocked
        | PlanningAttention::UsageLimited | PlanningAttention::BudgetLimited | PlanningAttention::PipelineFailed
        | PlanningAttention::ChangeRequested => Blocked,
        PlanningAttention::ReviewUnseen => Review,
        PlanningAttention::MarkedUnread => Unread,
    };
    AttentionSignal { level, reason: reason.label() }
}

pub fn thread_signal(reason: &AttentionReason) -> AttentionSignal {
    use AttentionLevel::*;
    let level = match reason {
        AttentionReason::ApprovalRequired | AttentionReason::UserInputRequired => Interaction,
        AttentionReason::SystemError => Blocked,
        AttentionReason::ReadyForReview => Review,
        AttentionReason::MarkedUnread => Unread,
    };
    AttentionSignal { level, reason: reason.label() }
}

pub fn effective_signal(
    thread: &ThreadSummary,
    card: Option<&WorkCardProjection>,
    pending_request: bool,
    acknowledged: bool,
) -> Option<AttentionSignal> {
    if card.is_some_and(|card| card.snoozed) { return None; }
    let pending = pending_request.then_some(AttentionSignal {
        level: AttentionLevel::Interaction, reason: "interactive",
    });
    let runtime = match thread.runtime {
        RuntimeStatus::WaitingHuman => Some(AttentionSignal { level: AttentionLevel::Interaction, reason: "waiting" }),
        RuntimeStatus::SystemError => Some(AttentionSignal { level: AttentionLevel::Blocked, reason: "error" }),
        _ => None,
    };
    pending.into_iter().chain(runtime)
        .chain(thread.attention.iter().map(thread_signal))
        .chain(card.into_iter().flat_map(|card| card.attention.iter().map(planning_signal)))
        .filter(|signal| signal.urgent() || !acknowledged)
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::{ThreadId, ThreadMetadata}, planning::{ReconcileInput, reconcile_thread_card}};
    fn thread() -> ThreadSummary {
        ThreadSummary { id: ThreadId::new("one"), workspace: "project".into(), title: "task".into(),
            runtime: RuntimeStatus::Inactive, attention: vec![], pinned: false, alias: None,
            metadata: ThreadMetadata::default() }
    }
    #[test]
    fn acknowledgement_never_hides_interaction_or_blockers() {
        let mut thread = thread();
        thread.attention = vec![AttentionReason::ReadyForReview, AttentionReason::MarkedUnread];
        assert_eq!(effective_signal(&thread, None, false, true), None);
        thread.attention.push(AttentionReason::ApprovalRequired);
        assert_eq!(effective_signal(&thread, None, false, true).unwrap().level, AttentionLevel::Interaction);
        thread.attention = vec![AttentionReason::SystemError];
        assert_eq!(effective_signal(&thread, None, false, true).unwrap().level, AttentionLevel::Blocked);
    }
    #[test]
    fn pending_request_is_visible_without_a_matching_status_notification() {
        assert_eq!(effective_signal(&thread(), None, true, true).unwrap().reason, "interactive");
    }
    #[test]
    fn observed_forge_blocker_and_snooze_share_one_policy() {
        let thread = thread();
        let mut card = reconcile_thread_card(ReconcileInput { thread: &thread, git: None, local: None,
            collision_count: 0, backend_observed_at_unix_ms: Some(1), backend_error: None, now_unix_ms: 1 });
        card.attention.clear();
        assert!(effective_signal(&thread, Some(&card), false, false).is_none());
        card.attention.insert(PlanningAttention::PipelineFailed);
        assert_eq!(effective_signal(&thread, Some(&card), false, true).unwrap().reason, "pipeline-failed");
        card.snoozed = true;
        assert!(effective_signal(&thread, Some(&card), true, false).is_none());
    }
}
''')
write('src/app/attention.rs', r'''use super::*;
use crate::attention::{AttentionSignal, effective_signal};

impl AppState {
    pub fn thread_attention_signal(&self, index: usize) -> Option<AttentionSignal> {
        let thread = self.threads.get(index)?;
        effective_signal(thread, self.work_card_for_thread(&thread.id),
            self.pending_request_threads.contains(&thread.id.0),
            self.acknowledged_attention.contains(&thread.id.0))
    }

    pub fn thread_needs_attention(&self, index: usize) -> bool {
        self.thread_attention_signal(index).is_some()
    }

    pub(super) fn order_registry_attention_first(&self, indices: &mut [usize]) {
        indices.sort_by_key(|index| {
            let thread = &self.threads[*index];
            let priority = self.thread_attention_signal(*index).map_or_else(
                || if thread.pinned { 4 } else if thread.runtime == RuntimeStatus::Working { 5 } else { 6 },
                |signal| signal.level as u8,
            );
            (priority, std::cmp::Reverse(thread.metadata.updated_at))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ThreadMetadata;
    fn thread(id: usize, status: RuntimeStatus) -> ThreadSummary {
        ThreadSummary { id: ThreadId::new(format!("task-{id}")), title: format!("task-{id}"),
            workspace: "project".into(), runtime: status, attention: vec![], pinned: false, alias: None,
            metadata: ThreadMetadata { updated_at: 200 - id as i64, ..Default::default() } }
    }
    #[test]
    fn blockers_rank_before_recent_and_pinned_without_reordering_entities() {
        let mut app = AppState::new(vec![thread(0, RuntimeStatus::Inactive), thread(1, RuntimeStatus::Working),
            thread(2, RuntimeStatus::Inactive), thread(3, RuntimeStatus::WaitingHuman), thread(4, RuntimeStatus::SystemError)]);
        app.host_local_only = false;
        app.repo_backed_only = false;
        app.threads[2].pinned = true;
        app.selected = 1;
        assert_eq!(app.visible_indices(), vec![3, 4, 2, 1, 0]);
        assert_eq!(app.selected_thread_id(), Some(ThreadId::new("task-1")));
        assert_eq!(app.threads[0].id, ThreadId::new("task-0"));
        app.show_all_history = true;
        assert_eq!(app.visible_indices(), vec![0, 1, 2, 3, 4]);
    }
    #[test]
    fn old_active_and_blocked_threads_survive_the_recent_window() {
        let mut app = AppState::new((0..150).map(|id| thread(id, RuntimeStatus::Inactive)).collect());
        app.host_local_only = false;
        app.repo_backed_only = false;
        app.threads[149].runtime = RuntimeStatus::WaitingHuman;
        app.threads[148].runtime = RuntimeStatus::Working;
        let visible = app.visible_indices();
        assert_eq!(visible[0], 149);
        assert!(visible.contains(&148));
        assert!(!visible.contains(&147));
        assert_eq!(visible.len(), 102);
    }
}
''')
replace('src/lib.rs', 'pub mod app;', 'pub mod app;\npub mod attention;')
replace('src/app.rs', 'mod context_menu;', 'mod attention;\nmod context_menu;')
between('src/app/state.rs', '    pub fn thread_needs_attention(&self, index: usize) -> bool {', '    pub(crate) fn planning_reconcile_count', '')
replace('src/app/state.rs', '                || thread.pinned\n                || self.thread_needs_attention(index)', '                || thread.pinned\n                || thread.runtime == RuntimeStatus::Working\n                || self.thread_needs_attention(index)')
replace('src/app/state.rs', '        (visible, matched_count)\n', '        if !show_all_matches {\n            self.order_registry_attention_first(&mut visible);\n        }\n        (visible, matched_count)\n')
replace('src/app/reducer.rs', '            } else {\n                state.selected = state.selected.min(state.threads.len() - 1);\n            }\n            ensure_selection_visible(state);', '            } else {\n                state.selected = state.visible_indices().first().copied().unwrap_or(0);\n            }\n            ensure_selection_visible(state);')
