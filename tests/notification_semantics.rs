//! Pure projection regressions: no account, process, network or model calls.
use codex_tui::{
    domain::{RuntimeStatus, ThreadId, ThreadMetadata, ThreadSummary},
    i18n::UiLanguage,
    notification::{NotificationKind, NotificationObservation, NotificationTracker},
    planning::{PlanningAttention, SourceRef, WorkCardOverlay, WorkCardProjection, WorkflowStage},
};
use std::collections::BTreeMap;

fn observation(
    runtime: RuntimeStatus,
    attention: &[PlanningAttention],
    snoozed: bool,
) -> NotificationObservation {
    let id = ThreadId::new("fixture");
    let thread = ThreadSummary {
        id: id.clone(),
        workspace: "fixture".into(),
        title: "opaque fixture".into(),
        runtime,
        attention: vec![],
        pinned: false,
        alias: None,
        metadata: ThreadMetadata::default(),
    };
    let card = WorkCardProjection {
        local_id: "thread:fixture".into(),
        anchor: SourceRef::codex_thread(&id),
        title: "opaque fixture".into(),
        workspace: None,
        branch: None,
        forge_provider: None,
        change_request_state: None,
        change_request_draft: false,
        stage: WorkflowStage::Ready,
        stage_reason: "fixture".into(),
        attention: attention.iter().cloned().collect(),
        snoozed,
        overlay: WorkCardOverlay::default(),
        links: vec![],
        goal: None,
        provenance: vec![],
    };
    NotificationObservation::from_projection(&[thread], &[card], &BTreeMap::new())
}

#[test]
fn idle_after_work_or_waiting_is_not_a_success_receipt() {
    for previous in [RuntimeStatus::Working, RuntimeStatus::WaitingHuman] {
        let mut tracker = NotificationTracker::default();
        assert!(tracker.advance(observation(previous, &[], false)).is_empty());
        let events = tracker.advance(observation(RuntimeStatus::Ready, &[], false));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, NotificationKind::ActivityStopped);
        assert_eq!(events[0].key, "activity-stopped:fixture");
        assert_eq!(
            events[0].text(UiLanguage::English).0,
            "codex-tui · Now idle; check result"
        );
        assert_eq!(
            events[0].text(UiLanguage::SimplifiedChinese).0,
            "codex-tui · 已空闲；请核对结果"
        );
        assert!(
            tracker
                .advance(observation(RuntimeStatus::Ready, &[], false))
                .is_empty()
        );
    }
}

#[test]
fn error_or_inactive_to_ready_does_not_claim_completion() {
    for previous in [RuntimeStatus::SystemError, RuntimeStatus::Inactive] {
        let mut tracker = NotificationTracker::default();
        tracker.advance(observation(previous, &[], false));
        assert!(
            tracker
                .advance(observation(RuntimeStatus::Ready, &[], false))
                .is_empty()
        );
    }
}

#[test]
fn errors_and_limits_notify_once_then_again_only_after_recovery() {
    for (attention, kind) in [
        (PlanningAttention::SystemError, NotificationKind::SystemError),
        (PlanningAttention::UsageLimited, NotificationKind::UsageLimited),
        (PlanningAttention::BudgetLimited, NotificationKind::BudgetLimited),
    ] {
        let mut tracker = NotificationTracker::default();
        tracker.advance(observation(RuntimeStatus::Ready, &[], false));
        for _ in 0..2 {
            let events = tracker.advance(observation(
                RuntimeStatus::Ready,
                std::slice::from_ref(&attention),
                false,
            ));
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].kind, kind);
            assert_eq!(events[0].item_id, "thread:fixture");
            assert!(
                tracker
                    .advance(observation(
                        RuntimeStatus::Ready,
                        std::slice::from_ref(&attention),
                        false,
                    ))
                    .is_empty()
            );
            assert!(
                tracker
                    .advance(observation(RuntimeStatus::Ready, &[], false))
                    .is_empty()
            );
        }
    }
}

#[test]
fn new_blocking_categories_respect_historical_seeding_and_snooze() {
    let attention = [
        PlanningAttention::SystemError,
        PlanningAttention::UsageLimited,
        PlanningAttention::BudgetLimited,
    ];
    let mut tracker = NotificationTracker::default();
    assert!(
        tracker
            .advance(observation(RuntimeStatus::Ready, &attention, false))
            .is_empty()
    );
    tracker.advance(observation(RuntimeStatus::Ready, &[], false));
    assert!(
        tracker
            .advance(observation(RuntimeStatus::Ready, &attention, true))
            .is_empty()
    );
    let events = tracker.advance(observation(RuntimeStatus::Ready, &attention, false));
    assert_eq!(events.len(), 3);
    assert!(events.iter().all(|event| event.kind != NotificationKind::Completion));
}
