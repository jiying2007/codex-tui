    use super::*;
    use crate::{
        domain::{ThreadId, ThreadMetadata},
        planning::{
            Freshness, Provenance, SourceRef, WorkCardOverlay, WorkflowStage,
        },
    };

    fn thread(id: &str, runtime: RuntimeStatus) -> ThreadSummary {
        ThreadSummary {
            id: ThreadId(id.into()),
            workspace: "test".into(),
            title: format!("Thread {id}"),
            runtime,
            attention: vec![],
            pinned: false,
            alias: None,
            metadata: ThreadMetadata::default(),
        }
    }

    fn card(id: &str, attention: &[PlanningAttention], snoozed: bool) -> WorkCardProjection {
        WorkCardProjection {
            local_id: id.into(),
            anchor: SourceRef {
                kind: crate::planning::SourceKind::ScratchWork,
                value: id.into(),
            },
            title: format!("Card {id}"),
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
            provenance: vec![Provenance {
                source: "fixture".into(),
                observed_at_unix_ms: Some(1),
                source_revision: None,
                freshness: Freshness::Fresh,
                degraded_reason: None,
            }],
        }
    }

    fn observation(
        threads: Vec<ThreadSummary>,
        cards: Vec<WorkCardProjection>,
    ) -> NotificationObservation {
        NotificationObservation::from_projection(&threads, &cards, &BTreeMap::new())
    }

    fn goal(thread_id: &str, status: GoalStatus) -> GoalObservation {
        GoalObservation {
            thread_id: ThreadId(thread_id.into()),
            objective: format!("Goal {thread_id}"),
            status,
            token_budget: None,
            tokens_used: 0,
            time_used_seconds: 0,
            created_at: 1,
            updated_at: 1,
            observed_at_unix_ms: 1,
        }
    }

    #[test]
    fn initial_projection_seeds_without_notifying_historical_attention() {
        let mut tracker = NotificationTracker::default();
        let events = tracker.advance(observation(
            vec![thread("1", RuntimeStatus::WaitingHuman)],
            vec![card(
                "1",
                &[PlanningAttention::ApprovalRequired],
                false,
            )],
        ));
        assert!(events.is_empty());
    }

    #[test]
    fn attention_is_edge_triggered_and_can_reappear_after_clearing() {
        let mut tracker = NotificationTracker::default();
        tracker.advance(observation(
            vec![thread("1", RuntimeStatus::Ready)],
            vec![card("1", &[], false)],
        ));

        let first = tracker.advance(observation(
            vec![thread("1", RuntimeStatus::WaitingHuman)],
            vec![card(
                "1",
                &[PlanningAttention::ApprovalRequired],
                false,
            )],
        ));
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].kind, NotificationKind::ApprovalRequired);

        assert!(
            tracker
                .advance(observation(
                    vec![thread("1", RuntimeStatus::WaitingHuman)],
                    vec![card(
                        "1",
                        &[PlanningAttention::ApprovalRequired],
                        false,
                    )],
                ))
                .is_empty()
        );

        tracker.advance(observation(
            vec![thread("1", RuntimeStatus::Working)],
            vec![card("1", &[], false)],
        ));
        let repeated = tracker.advance(observation(
            vec![thread("1", RuntimeStatus::WaitingHuman)],
            vec![card(
                "1",
                &[PlanningAttention::ApprovalRequired],
                false,
            )],
        ));
        assert_eq!(repeated.len(), 1);
    }

    #[test]
    fn newly_discovered_card_seeds_attention_before_future_edges() {
        let mut tracker = NotificationTracker::default();
        tracker.advance(observation(vec![], vec![]));

        let historical = tracker.advance(observation(
            vec![],
            vec![card(
                "late",
                &[PlanningAttention::PipelineFailed],
                false,
            )],
        ));
        assert!(historical.is_empty());

        tracker.advance(observation(vec![], vec![card("late", &[], false)]));
        let fresh = tracker.advance(observation(
            vec![],
            vec![card(
                "late",
                &[PlanningAttention::PipelineFailed],
                false,
            )],
        ));
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].kind, NotificationKind::PipelineFailed);
    }

    #[test]
    fn snooze_suppresses_routing_without_rewriting_attention() {
        let mut tracker = NotificationTracker::default();
        tracker.advance(observation(
            vec![thread("1", RuntimeStatus::Ready)],
            vec![card("1", &[], false)],
        ));
        let events = tracker.advance(observation(
            vec![thread("1", RuntimeStatus::WaitingHuman)],
            vec![card(
                "1",
                &[PlanningAttention::UserInputRequired],
                true,
            )],
        ));
        assert!(events.is_empty());
    }

    #[test]
    fn projection_maps_goal_pipeline_and_review_attention_without_duplicates() {
        let cards = vec![
            card("goal", &[PlanningAttention::GoalBlocked], false),
            card("pipe", &[PlanningAttention::PipelineFailed], false),
            card(
                "review",
                &[
                    PlanningAttention::ReviewUnseen,
                    PlanningAttention::ChangeRequested,
                ],
                false,
            ),
        ];
        let observation = NotificationObservation::from_projection(
            &[],
            &cards,
            &BTreeMap::new(),
        );
        let kinds = observation
            .active
            .values()
            .map(|event| event.kind)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            kinds,
            BTreeSet::from([
                NotificationKind::GoalBlocked,
                NotificationKind::PipelineFailed,
                NotificationKind::ReviewRequested,
            ])
        );
        assert_eq!(
            observation
                .active
                .values()
                .filter(|event| event.kind == NotificationKind::ReviewRequested)
                .count(),
            1
        );
    }

    #[test]
    fn goal_completion_is_edge_triggered_and_deduplicated_by_thread() {
        let mut tracker = NotificationTracker::default();
        let threads = vec![thread("1", RuntimeStatus::Working)];
        tracker.advance(NotificationObservation::from_projection(
            &threads,
            &[],
            &BTreeMap::from([("1".into(), goal("1", GoalStatus::Active))]),
        ));

        let ready = vec![thread("1", RuntimeStatus::Ready)];
        let events = tracker.advance(NotificationObservation::from_projection(
            &ready,
            &[],
            &BTreeMap::from([("1".into(), goal("1", GoalStatus::Complete))]),
        ));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, NotificationKind::Completion);
        assert_eq!(events[0].key, "completion:1");
    }

    #[test]
    fn working_to_ready_emits_completion_once() {
        let mut tracker = NotificationTracker::default();
        tracker.advance(observation(
            vec![thread("1", RuntimeStatus::Working)],
            vec![card("1", &[], false)],
        ));
        let events = tracker.advance(observation(
            vec![thread("1", RuntimeStatus::Ready)],
            vec![card("1", &[], false)],
        ));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, NotificationKind::Completion);
        assert!(
            tracker
                .advance(observation(
                    vec![thread("1", RuntimeStatus::Ready)],
                    vec![card("1", &[], false)],
                ))
                .is_empty()
        );
    }
