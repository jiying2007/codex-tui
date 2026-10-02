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
    pub item_id: String,
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
    known_items: BTreeSet<String>,
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

        let known_items = cards
            .iter()
            .map(|card| card.local_id.clone())
            .collect::<BTreeSet<_>>();
        let mut active = BTreeMap::new();
        for card in cards.iter().filter(|card| !card.snoozed) {
            let subject = card.title.clone();
            for kind in card_notification_kinds(card) {
                let event = NotificationEvent {
                    key: format!("{}:{}", kind.label(), card.local_id),
                    item_id: card.local_id.clone(),
                    kind,
                    subject: subject.clone(),
                };
                active.insert(event.key.clone(), event);
            }
        }

        Self {
            active,
            known_items,
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
    if card.attention.contains(&PlanningAttention::PipelineFailed) {
        kinds.insert(NotificationKind::PipelineFailed);
    }
    if card.attention.contains(&PlanningAttention::ReviewUnseen)
        || card.attention.contains(&PlanningAttention::ChangeRequested)
    {
        kinds.insert(NotificationKind::ReviewRequested);
    }
    kinds
}

#[derive(Clone, Debug, Default)]
pub struct NotificationTracker {
    initialized: bool,
    active: BTreeMap<String, NotificationEvent>,
    known_items: BTreeSet<String>,
    runtimes: BTreeMap<String, (RuntimeStatus, String)>,
    goals: BTreeMap<String, (GoalStatus, String)>,
}

impl NotificationTracker {
    pub fn advance(&mut self, observation: NotificationObservation) -> Vec<NotificationEvent> {
        if !self.initialized {
            self.initialized = true;
            self.active = observation.active;
            self.known_items = observation.known_items;
            self.runtimes = observation.runtimes;
            self.goals = observation.goals;
            return vec![];
        }

        let mut emitted = BTreeMap::<String, NotificationEvent>::new();

        for (key, event) in &observation.active {
            if !self.active.contains_key(key) && self.known_items.contains(&event.item_id) {
                emitted.insert(key.clone(), event.clone());
            }
        }

        for (thread_id, (runtime, subject)) in &observation.runtimes {
            let completed = self.runtimes.get(thread_id).is_some_and(|(previous, _)| {
                matches!(
                    previous,
                    RuntimeStatus::Working | RuntimeStatus::WaitingHuman
                ) && *runtime == RuntimeStatus::Ready
            });
            if completed {
                let key = format!("completion:{thread_id}");
                emitted.insert(
                    key.clone(),
                    NotificationEvent {
                        key,
                        item_id: thread_id.clone(),
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
                        item_id: thread_id.clone(),
                        kind: NotificationKind::Completion,
                        subject: subject.clone(),
                    },
                );
            }
        }

        self.active = observation.active;
        self.known_items = observation.known_items;
        self.runtimes = observation.runtimes;
        self.goals = observation.goals;
        emitted.into_values().collect()
    }
}

#[cfg(test)]
mod tests;
