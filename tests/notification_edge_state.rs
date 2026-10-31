//! Offline edges must not retain presentation text between observations.
use codex_tui::{
    domain::{RuntimeStatus, ThreadId, ThreadMetadata, ThreadSummary},
    goal::{GoalObservation, GoalStatus},
    notification::{NotificationKind, NotificationObservation, NotificationTracker},
    planning::{PlanningAttention, SourceRef, WorkCardOverlay, WorkCardProjection, WorkflowStage},
};
use std::collections::BTreeMap;

fn thread(id: &str, runtime: RuntimeStatus, title: &str) -> ThreadSummary {
    ThreadSummary {
        id: ThreadId::new(id),
        workspace: "fixture".into(),
        title: title.into(),
        runtime,
        attention: vec![],
        pinned: false,
        alias: None,
        metadata: ThreadMetadata::default(),
    }
}

fn card(id: &str, attention: Vec<PlanningAttention>, title: &str) -> WorkCardProjection {
    WorkCardProjection {
        local_id: format!("thread:{id}"),
        anchor: SourceRef::codex_thread(&ThreadId::new(id)),
        title: title.into(),
        workspace: None,
        branch: None,
        forge_provider: None,
        change_request_state: None,
        change_request_draft: false,
        stage: WorkflowStage::Ready,
        stage_reason: "fixture".into(),
        attention: attention.into_iter().collect(),
        snoozed: false,
        overlay: WorkCardOverlay::default(),
        links: vec![],
        goal: None,
        provenance: vec![],
    }
}

fn goal(id: &str, status: GoalStatus, objective: &str) -> BTreeMap<String, GoalObservation> {
    BTreeMap::from([(
        id.into(),
        GoalObservation {
            thread_id: ThreadId::new(id),
            objective: objective.into(),
            status,
            token_budget: None,
            tokens_used: 0,
            time_used_seconds: 0,
            created_at: 1,
            updated_at: 1,
            observed_at_unix_ms: 1,
        },
    )])
}

fn observe(threads: &[ThreadSummary], cards: &[WorkCardProjection]) -> NotificationObservation {
    NotificationObservation::from_projection(threads, cards, &BTreeMap::new())
}

#[test]
fn edge_state_discards_thread_card_and_goal_subjects_after_seeding() {
    let mut tracker = NotificationTracker::default();
    let threads = [thread("a", RuntimeStatus::Working, "old-thread")];
    let cards = [card("a", vec![PlanningAttention::SystemError], "old-card")];
    let goals = goal("orphan", GoalStatus::Active, "old-goal");
    let input = NotificationObservation::from_projection(&threads, &cards, &goals);
    assert!(tracker.advance(input).is_empty());
    let state = format!("{tracker:?}");
    for marker in ["old-thread", "old-card", "old-goal"] {
        assert!(!state.contains(marker), "tracker retained presentation text");
    }
    // Cloning preserves de-duplication without retaining the old subjects.
    let input = NotificationObservation::from_projection(&threads, &cards, &goals);
    assert!(tracker.clone().advance(input).is_empty());
}

#[test]
fn idle_edge_uses_the_current_alias_not_a_retained_title() {
    let mut tracker = NotificationTracker::default();
    let before = [thread("a", RuntimeStatus::Working, "old-title")];
    tracker.advance(observe(&before, &[]));
    let mut latest = thread("a", RuntimeStatus::Ready, "new-title");
    latest.alias = Some("current-alias".into());
    let after = [latest];
    let events = tracker.advance(observe(&after, &[]));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, NotificationKind::ActivityStopped);
    assert_eq!(events[0].subject, "current-alias");
    assert!(tracker.advance(observe(&after, &[])).is_empty());
}

#[test]
fn goal_completion_uses_latest_fallback_and_still_suppresses_idle() {
    let mut tracker = NotificationTracker::default();
    let active = goal("a", GoalStatus::Active, "old-goal");
    let completed = goal("a", GoalStatus::Complete, "latest-goal");
    tracker.advance(NotificationObservation::from_projection(&[], &[], &active));
    let input = NotificationObservation::from_projection(&[], &[], &completed);
    let events = tracker.advance(input.clone());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, NotificationKind::Completion);
    assert_eq!(events[0].subject, "latest-goal");
    assert!(tracker.advance(input).is_empty());

    let mut tracker = NotificationTracker::default();
    let before = [thread("a", RuntimeStatus::Working, "before")];
    let after = [thread("a", RuntimeStatus::Ready, "after")];
    let input = NotificationObservation::from_projection(&before, &[], &active);
    tracker.advance(input);
    let input = NotificationObservation::from_projection(&after, &[], &completed);
    let events = tracker.advance(input);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, NotificationKind::Completion);
    assert_eq!(events[0].subject, "after");
}

#[test]
fn renamed_attention_is_quiet_until_a_new_edge_uses_its_current_subject() {
    let mut tracker = NotificationTracker::default();
    tracker.advance(observe(&[], &[card("a", vec![], "before")]));
    let first = [card("a", vec![PlanningAttention::SystemError], "first")];
    assert_eq!(tracker.advance(observe(&[], &first))[0].subject, "first");
    let renamed = [card("a", vec![PlanningAttention::SystemError], "renamed")];
    assert!(tracker.advance(observe(&[], &renamed)).is_empty());
    tracker.advance(observe(&[], &[card("a", vec![], "clear")]));
    let events = tracker.advance(observe(&[], &renamed));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, NotificationKind::SystemError);
    assert_eq!(events[0].subject, "renamed");
}

#[test]
fn removed_identity_is_forgotten_and_rediscovered_history_is_not_replayed() {
    let mut tracker = NotificationTracker::default();
    let before = [thread("removed-id", RuntimeStatus::Working, "old")];
    let after = [thread("removed-id", RuntimeStatus::Ready, "new")];
    let attention = vec![PlanningAttention::BudgetLimited];
    let cards = [card("removed-id", attention, "old")];
    tracker.advance(observe(&before, &cards));
    assert!(tracker.advance(observe(&[], &[])).is_empty());
    assert!(!format!("{tracker:?}").contains("removed-id"));
    assert!(tracker.advance(observe(&after, &cards)).is_empty());
}

#[test]
fn snoozed_attention_is_known_but_not_replayed_until_unsnoozed() {
    let mut tracker = NotificationTracker::default();
    tracker.advance(observe(&[], &[card("a", vec![], "before")]));
    let mut cards = [card("a", vec![PlanningAttention::UsageLimited], "latest")];
    cards[0].snoozed = true;
    assert!(tracker.advance(observe(&[], &cards)).is_empty());
    cards[0].snoozed = false;
    let events = tracker.advance(observe(&[], &cards));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, NotificationKind::UsageLimited);
    assert_eq!(events[0].subject, "latest");
}

#[test]
fn large_registry_keeps_edges_not_long_titles_and_refreshes_the_correct_target() {
    for rows in [10_000, 50_000] {
        let title = "retained-subject-marker ".repeat(12);
        let mut threads: Vec<_> = (0..rows)
            .map(|index| thread(&format!("id-{index}"), RuntimeStatus::Working, &title))
            .collect();
        let mut tracker = NotificationTracker::default();
        assert!(tracker.advance(observe(&threads, &[])).is_empty());
        assert!(!format!("{tracker:?}").contains("retained-subject-marker"));
        assert!(tracker.advance(observe(&threads, &[])).is_empty());
        let last = threads.last_mut().unwrap();
        last.runtime = RuntimeStatus::Ready;
        last.title = "current-target".into();
        let events = tracker.advance(observe(&threads, &[]));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].item_id, format!("id-{}", rows - 1));
        assert_eq!(events[0].subject, "current-target");
        assert_eq!(events[0].kind, NotificationKind::ActivityStopped);
        assert!(tracker.advance(observe(&[], &[])).is_empty());
        assert!(!format!("{tracker:?}").contains("id-"));
    }
}
