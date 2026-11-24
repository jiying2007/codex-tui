//! Native session notification edges; non-session legacy cards remain separate.
use super::*;
use crate::{app::AppState, conversation::InteractiveRequestKind, planning::SourceKind};

impl NotificationObservation {
    pub fn from_app(app: &AppState) -> Self {
        let mut result = Self::from_projection(&app.threads, &[], &app.goals);
        result.scope = Some((
            app.backend_status.source.clone(),
            app.backend_status.codex_home.clone(),
        ));
        let known: BTreeMap<_, _> = app
            .threads
            .iter()
            .map(|thread| (thread.id.0.as_str(), thread))
            .collect();
        result
            .known_items
            .extend(known.keys().map(|id| (*id).to_owned()));
        // Only non-session legacy cards use the old Board projection.
        for card in &app.work_cards {
            if card.anchor.kind == SourceKind::CodexThread {
                continue;
            }
            result.known_items.insert(card.local_id.clone());
            if card.snoozed {
                continue;
            }
            for kind in card_notification_kinds(card) {
                let key = format!("{}:{}", kind.label(), card.local_id);
                result.active.insert(
                    key.clone(),
                    NotificationEvent {
                        key,
                        item_id: card.local_id.clone(),
                        kind,
                        subject: card.title.clone(),
                    },
                );
            }
        }
        for thread in &app.threads {
            if app.session_attention_snoozed(thread) {
                continue;
            }
            for kind in app
                .session_attention_reasons(thread)
                .iter()
                .filter_map(attention_notification_kind)
            {
                let key = format!("native:{}:{}", kind.label(), thread.id.0);
                result.active.insert(
                    key.clone(),
                    NotificationEvent {
                        key,
                        item_id: thread.id.0.clone(),
                        kind,
                        subject: thread.display_title().into(),
                    },
                );
            }
        }
        for request in &app.pending_requests {
            let Some(thread) = known.get(request.thread_id.0.as_str()) else {
                continue;
            };
            if app.session_attention_snoozed(thread) {
                continue;
            }
            let kind = match &request.kind {
                InteractiveRequestKind::UserInput { .. } => NotificationKind::UserInputRequired,
                _ => NotificationKind::ApprovalRequired,
            };
            // Track individual requests without storing approval/command payloads.
            let key = format!(
                "native:request:{}:{:?}:{:?}:{:?}:{:?}",
                kind.label(),
                request.thread_id.0,
                request.turn_id,
                request.item_id,
                request.request_id
            );
            result.active.insert(
                key.clone(),
                NotificationEvent {
                    key,
                    item_id: thread.id.0.clone(),
                    kind,
                    subject: thread.display_title().into(),
                },
            );
        }
        result
    }
}

impl NotificationTracker {
    pub fn scope_changed(&self, observation: &NotificationObservation) -> bool {
        self.scope != observation.scope
    }

    pub(super) fn prepare_scope(&mut self, observation: &NotificationObservation) {
        if self.scope_changed(observation) {
            self.scope = observation.scope.clone();
            self.active.clear();
            self.known_items.clear();
            self.runtimes.clear();
            self.goals.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        backend::{CodexBackend, FakeBackend},
        conversation::{InteractiveRequest, RpcRequestId},
        domain::AttentionReason,
    };

    #[test]
    fn live_goal_limit_edges_do_not_wait_for_board() {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        let mut tracker = NotificationTracker::default();
        tracker.advance(NotificationObservation::from_app(&app));
        let id = app.threads[0].id.clone();
        let observation = crate::goal::GoalObservation {
            thread_id: id.clone(),
            objective: "fixture".into(),
            status: GoalStatus::BudgetLimited,
            token_budget: Some(10),
            tokens_used: 10,
            time_used_seconds: 1,
            created_at: 1,
            updated_at: 1,
            observed_at_unix_ms: 1,
        };
        app.goals.insert(id.0.clone(), observation);
        assert!(app.work_cards.is_empty());
        let events = tracker.advance(NotificationObservation::from_app(&app));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, NotificationKind::BudgetLimited);
        assert_eq!(events[0].item_id, id.0);
        assert!(
            tracker
                .advance(NotificationObservation::from_app(&app))
                .is_empty()
        );
        app.goals.clear();
        assert!(
            tracker
                .advance(NotificationObservation::from_app(&app))
                .is_empty()
        );
    }

    #[test]
    fn native_approval_without_planning_is_edge_triggered() {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.threads[1].attention.clear();
        app.work_cards.clear();
        let mut tracker = NotificationTracker::default();
        tracker.advance(NotificationObservation::from_app(&app));
        app.threads[1]
            .attention
            .push(AttentionReason::ApprovalRequired);
        let events = tracker.advance(NotificationObservation::from_app(&app));
        assert_eq!(events.len(), 1);
        assert!(events[0].key.starts_with("native:"));
        assert_eq!(events[0].item_id, app.threads[1].id.0);
        assert!(
            tracker
                .advance(NotificationObservation::from_app(&app))
                .is_empty()
        );
        app.threads[1].attention.clear();
        tracker.advance(NotificationObservation::from_app(&app));
        app.threads[1]
            .attention
            .push(AttentionReason::ApprovalRequired);
        assert_eq!(
            tracker
                .advance(NotificationObservation::from_app(&app))
                .len(),
            1
        );
    }

    #[test]
    fn real_request_identity_triggers_each_new_request_without_duplicates() {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.threads[1].attention.clear();
        let mut tracker = NotificationTracker::default();
        tracker.advance(NotificationObservation::from_app(&app));
        let request = InteractiveRequest {
            request_id: RpcRequestId::Integer(31),
            thread_id: app.threads[1].id.clone(),
            turn_id: "turn".into(),
            item_id: "input".into(),
            kind: InteractiveRequestKind::UserInput { questions: vec![] },
        };
        app.pending_requests.push(request.clone());
        let events = tracker.advance(NotificationObservation::from_app(&app));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, NotificationKind::UserInputRequired);
        assert!(events[0].key.starts_with("native:request:"));
        let first_key = events[0].key.clone();
        app.threads[1]
            .attention
            .push(AttentionReason::UserInputRequired);
        app.pending_requests.push(InteractiveRequest {
            request_id: RpcRequestId::Integer(32),
            ..request
        });
        let second = tracker.advance(NotificationObservation::from_app(&app));
        assert_eq!(second.len(), 1, "second real request must not be hidden");
        assert_eq!(second[0].kind, NotificationKind::UserInputRequired);
        assert_ne!(first_key, second[0].key);
        assert_eq!(second[0].item_id, app.threads[1].id.0);
        assert!(
            tracker
                .advance(NotificationObservation::from_app(&app))
                .is_empty()
        );
        app.pending_requests.clear();
        assert!(
            tracker
                .advance(NotificationObservation::from_app(&app))
                .is_empty()
        );
    }

    #[test]
    fn source_and_codex_home_changes_reset_prior_edges() {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.threads[1].attention.clear();
        app.backend_status.source = "target-A".into();
        app.backend_status.codex_home = Some("/one".into());
        let mut tracker = NotificationTracker::default();
        tracker.advance(NotificationObservation::from_app(&app));
        app.threads[1]
            .attention
            .push(AttentionReason::ApprovalRequired);
        assert_eq!(
            tracker
                .advance(NotificationObservation::from_app(&app))
                .len(),
            1
        );
        app.backend_status.source = "target-B".into();
        assert!(
            tracker
                .advance(NotificationObservation::from_app(&app))
                .is_empty()
        );
        app.backend_status.codex_home = Some("/two".into());
        assert!(
            tracker
                .advance(NotificationObservation::from_app(&app))
                .is_empty()
        );
        app.threads[1].attention.clear();
        tracker.advance(NotificationObservation::from_app(&app));
        app.threads[1]
            .attention
            .push(AttentionReason::ApprovalRequired);
        assert_eq!(
            tracker
                .advance(NotificationObservation::from_app(&app))
                .len(),
            1
        );
    }
}
