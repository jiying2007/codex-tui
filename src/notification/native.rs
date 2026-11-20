//! Native request notification edges; WorkCards remain only for derived signals.
use super::*;
use crate::{
    app::AppState, conversation::InteractiveRequestKind, domain::AttentionReason,
    planning::SourceKind,
};

impl NotificationObservation {
    pub fn from_app(app: &AppState) -> Self {
        let mut result = Self::from_projection(&app.threads, &app.work_cards, &app.goals);
        result.scope = Some((
            app.backend_status.source.clone(),
            app.backend_status.codex_home.clone(),
        ));
        let known: BTreeSet<_> = app.threads.iter().map(|t| t.id.0.as_str()).collect();
        result
            .known_items
            .extend(known.iter().map(|id| (*id).to_owned()));
        let mut muted = BTreeSet::new();
        for card in &app.work_cards {
            if card.anchor.kind != SourceKind::CodexThread {
                continue;
            }
            if card.snoozed {
                muted.insert(card.anchor.value.as_str());
            }
            // The native thread/request is the sole authority for these events.
            for kind in [
                NotificationKind::ApprovalRequired,
                NotificationKind::UserInputRequired,
                NotificationKind::SystemError,
            ] {
                result
                    .active
                    .remove(&format!("{}:{}", kind.label(), card.local_id));
            }
        }
        let mut native: BTreeMap<&str, BTreeSet<NotificationKind>> = BTreeMap::new();
        for thread in &app.threads {
            let kinds = native.entry(&thread.id.0).or_default();
            for reason in &thread.attention {
                let kind = match reason {
                    AttentionReason::ApprovalRequired => Some(NotificationKind::ApprovalRequired),
                    AttentionReason::UserInputRequired => Some(NotificationKind::UserInputRequired),
                    AttentionReason::SystemError => Some(NotificationKind::SystemError),
                    _ => None,
                };
                if let Some(kind) = kind {
                    kinds.insert(kind);
                }
            }
        }
        for request in &app.pending_requests {
            if !known.contains(request.thread_id.0.as_str()) {
                continue;
            }
            let kind = match &request.kind {
                InteractiveRequestKind::UserInput { .. } => NotificationKind::UserInputRequired,
                _ => NotificationKind::ApprovalRequired,
            };
            native.entry(&request.thread_id.0).or_default().insert(kind);
        }
        for thread in &app.threads {
            if muted.contains(thread.id.0.as_str()) {
                continue;
            }
            if let Some(kinds) = native.get(thread.id.0.as_str()) {
                for &kind in kinds {
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
    };

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
    fn real_request_triggers_without_flag_and_deduplicates() {
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
        app.threads[1]
            .attention
            .push(AttentionReason::UserInputRequired);
        app.pending_requests.push(InteractiveRequest {
            request_id: RpcRequestId::Integer(32),
            ..request
        });
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
