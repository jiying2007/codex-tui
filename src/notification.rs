use crate::{
    domain::{RuntimeStatus, ThreadSummary},
    goal::{GoalObservation, GoalStatus},
    i18n::UiLanguage,
    planning::{PlanningAttention, WorkCardProjection},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationMode {
    #[default]
    Off,
    Terminal,
    Os,
}

impl NotificationMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Terminal => "terminal",
            Self::Os => "os",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotificationKind {
    ApprovalRequired,
    UserInputRequired,
    GoalBlocked,
    Completion,
    PipelineFailed,
    ReviewRequested,
}

impl NotificationKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ApprovalRequired => "approval-required",
            Self::UserInputRequired => "user-input-required",
            Self::GoalBlocked => "goal-blocked",
            Self::Completion => "completion",
            Self::PipelineFailed => "pipeline-failed",
            Self::ReviewRequested => "review-requested",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationEvent {
    pub key: String,
    pub kind: NotificationKind,
    pub subject: String,
}

impl NotificationEvent {
    pub fn text(&self, language: UiLanguage) -> (String, String) {
        let chinese = language.is_simplified_chinese();
        let label = match (self.kind, chinese) {
            (NotificationKind::ApprovalRequired, false) => "Approval required",
            (NotificationKind::ApprovalRequired, true) => "需要审批",
            (NotificationKind::UserInputRequired, false) => "Input required",
            (NotificationKind::UserInputRequired, true) => "需要输入",
            (NotificationKind::GoalBlocked, false) => "Goal blocked",
            (NotificationKind::GoalBlocked, true) => "Goal 已阻塞",
            (NotificationKind::Completion, false) => "Work completed",
            (NotificationKind::Completion, true) => "工作已完成",
            (NotificationKind::PipelineFailed, false) => "Pipeline failed",
            (NotificationKind::PipelineFailed, true) => "流水线失败",
            (NotificationKind::ReviewRequested, false) => "Review needs attention",
            (NotificationKind::ReviewRequested, true) => "评审需要处理",
        };
        (format!("codex-tui · {label}"), self.subject.clone())
    }
}

#[derive(Clone, Debug, Default)]
pub struct NotificationObservation {
    active: BTreeMap<String, NotificationEvent>,
    runtimes: BTreeMap<String, (RuntimeStatus, String)>,
    goals: BTreeMap<String, (GoalStatus, String)>,
}

impl NotificationObservation {
    pub fn from_projection(
        threads: &[ThreadSummary],
        cards: &[WorkCardProjection],
        goals: &BTreeMap<String, GoalObservation>,
    ) -> Self {
        let titles = threads
            .iter()
            .map(|thread| (thread.id.0.clone(), thread.display_title().to_string()))
            .collect::<BTreeMap<_, _>>();
        let runtimes = threads
            .iter()
            .map(|thread| {
                (
                    thread.id.0.clone(),
                    (thread.runtime.clone(), thread.display_title().to_string()),
                )
            })
            .collect();
        let goals = goals
            .iter()
            .map(|(thread_id, goal)| {
                (
                    thread_id.clone(),
                    (
                        goal.status,
                        titles
                            .get(thread_id)
                            .cloned()
                            .unwrap_or_else(|| goal.objective.clone()),
                    ),
                )
            })
            .collect();

        let mut active = BTreeMap::new();
        for card in cards.iter().filter(|card| !card.snoozed) {
            let subject = card.title.clone();
            for kind in card_notification_kinds(card) {
                let event = NotificationEvent {
                    key: format!("{}:{}", kind.label(), card.local_id),
                    kind,
                    subject: subject.clone(),
                };
                active.insert(event.key.clone(), event);
            }
        }

        Self {
            active,
            runtimes,
            goals,
        }
    }
}

fn card_notification_kinds(card: &WorkCardProjection) -> BTreeSet<NotificationKind> {
    let mut kinds = BTreeSet::new();
    if card
        .attention
        .contains(&PlanningAttention::ApprovalRequired)
    {
        kinds.insert(NotificationKind::ApprovalRequired);
    }
    if card
        .attention
        .contains(&PlanningAttention::UserInputRequired)
    {
        kinds.insert(NotificationKind::UserInputRequired);
    }
    if card.attention.contains(&PlanningAttention::GoalBlocked) {
        kinds.insert(NotificationKind::GoalBlocked);
    }
    if card
        .attention
        .contains(&PlanningAttention::PipelineFailed)
    {
        kinds.insert(NotificationKind::PipelineFailed);
    }
    if card.attention.contains(&PlanningAttention::ReviewUnseen)
        || card
            .attention
            .contains(&PlanningAttention::ChangeRequested)
    {
        kinds.insert(NotificationKind::ReviewRequested);
    }
    kinds
}

#[derive(Clone, Debug, Default)]
pub struct NotificationTracker {
    initialized: bool,
    active: BTreeMap<String, NotificationEvent>,
    runtimes: BTreeMap<String, (RuntimeStatus, String)>,
    goals: BTreeMap<String, (GoalStatus, String)>,
}

impl NotificationTracker {
    pub fn advance(&mut self, observation: NotificationObservation) -> Vec<NotificationEvent> {
        if !self.initialized {
            self.initialized = true;
            self.active = observation.active;
            self.runtimes = observation.runtimes;
            self.goals = observation.goals;
            return vec![];
        }

        let mut emitted = BTreeMap::<String, NotificationEvent>::new();

        for (key, event) in &observation.active {
            if !self.active.contains_key(key) {
                emitted.insert(key.clone(), event.clone());
            }
        }

        for (thread_id, (runtime, subject)) in &observation.runtimes {
            let completed = self.runtimes.get(thread_id).is_some_and(|(previous, _)| {
                matches!(previous, RuntimeStatus::Working | RuntimeStatus::WaitingHuman)
                    && *runtime == RuntimeStatus::Ready
            });
            if completed {
                let key = format!("completion:{thread_id}");
                emitted.insert(
                    key.clone(),
                    NotificationEvent {
                        key,
                        kind: NotificationKind::Completion,
                        subject: subject.clone(),
                    },
                );
            }
        }

        for (thread_id, (status, subject)) in &observation.goals {
            let completed = self
                .goals
                .get(thread_id)
                .is_some_and(|(previous, _)| *previous != GoalStatus::Complete)
                && *status == GoalStatus::Complete;
            if completed {
                let key = format!("completion:{thread_id}");
                emitted.insert(
                    key.clone(),
                    NotificationEvent {
                        key,
                        kind: NotificationKind::Completion,
                        subject: subject.clone(),
                    },
                );
            }
        }

        self.active = observation.active;
        self.runtimes = observation.runtimes;
        self.goals = observation.goals;
        emitted.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
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
}
