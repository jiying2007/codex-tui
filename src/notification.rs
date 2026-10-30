use crate::{
    domain::{RuntimeStatus, ThreadId, ThreadSummary},
    goal::{GoalObservation, GoalStatus},
    i18n::UiLanguage,
    planning::{PlanningAttention, WorkCardProjection},
    text::truncate_display,
};

const MAX_NOTIFICATION_SUBJECT_COLUMNS: usize = 160;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

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
    ActivityStopped,
    SystemError,
    UsageLimited,
    BudgetLimited,
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
            Self::ActivityStopped => "activity-stopped",
            Self::SystemError => "system-error",
            Self::UsageLimited => "usage-limited",
            Self::BudgetLimited => "budget-limited",
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
        let labels = match self.kind {
            NotificationKind::ApprovalRequired => ("Approval required", "需要审批"),
            NotificationKind::UserInputRequired => ("Input required", "需要输入"),
            NotificationKind::GoalBlocked => ("Goal blocked", "Goal 已阻塞"),
            NotificationKind::Completion => ("Goal completed", "Goal 已完成"),
            NotificationKind::PipelineFailed => ("Pipeline failed", "流水线失败"),
            NotificationKind::ReviewRequested => ("Review needs attention", "评审需要处理"),
            NotificationKind::ActivityStopped => ("Now idle; check result", "已空闲；请核对结果"),
            NotificationKind::SystemError => {
                ("System error; inspect details", "系统错误；请查看详情")
            }
            NotificationKind::UsageLimited => ("Usage limited", "额度受限"),
            NotificationKind::BudgetLimited => ("Budget limited", "预算受限"),
        };
        let label = if chinese { labels.1 } else { labels.0 };
        (
            format!("codex-tui · {label}"),
            truncate_display(&self.subject, MAX_NOTIFICATION_SUBJECT_COLUMNS),
        )
    }
}

const RECENT_EVENT_DISPLAY_DURATION: Duration = Duration::from_secs(20);

/// One ephemeral, read-only indication of observed source state, not a new task
/// ledger. It expires on the existing UI loop and is never persisted to SQLite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentNotification {
    pub event: NotificationEvent,
    pub thread_id: Option<ThreadId>,
    pub other_count: usize,
    pub visible_until: Instant,
}

impl RecentNotification {
    pub fn new(event: NotificationEvent, thread_id: Option<ThreadId>, other_count: usize) -> Self {
        Self {
            event,
            thread_id,
            other_count,
            visible_until: Instant::now() + RECENT_EVENT_DISPLAY_DURATION,
        }
    }

    pub fn display(&self, language: UiLanguage) -> String {
        let (title, subject) = self.event.text(language);
        // Keep the action before the subject even in narrow SSH terminals.
        let label = title.strip_prefix("codex-tui · ").unwrap_or(&title);
        let action = match (self.thread_id.is_some(), language.is_simplified_chinese()) {
            (true, true) => "! 跳转",
            (true, false) => "! open",
            (false, true) => "看板",
            (false, false) => "Board",
        };
        let other = if self.other_count == 0 {
            String::new()
        } else if language.is_simplified_chinese() {
            format!(" · 另有 {} 项更新", self.other_count)
        } else {
            format!(" · {} more updates", self.other_count)
        };
        format!("{action} · {label}: {subject}{other}")
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
        let runtimes: BTreeMap<_, _> = threads
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
                        runtimes
                            .get(thread_id)
                            .map(|(_, title)| title.clone())
                            .unwrap_or_else(|| goal.objective.clone()),
                    ),
                )
            })
            .collect();
        let known_items = cards.iter().map(|card| card.local_id.clone()).collect();
        let mut active = BTreeMap::new();
        for card in cards.iter().filter(|card| !card.snoozed) {
            for kind in card_notification_kinds(card) {
                let event = NotificationEvent {
                    key: format!("{}:{}", kind.label(), card.local_id),
                    item_id: card.local_id.clone(),
                    kind,
                    subject: card.title.clone(),
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
    use NotificationKind as N;
    use PlanningAttention as A;
    [
        (A::ApprovalRequired, N::ApprovalRequired),
        (A::UserInputRequired, N::UserInputRequired),
        (A::GoalBlocked, N::GoalBlocked),
        (A::PipelineFailed, N::PipelineFailed),
        (A::ReviewUnseen, N::ReviewRequested),
        (A::ChangeRequested, N::ReviewRequested),
        (A::SystemError, N::SystemError),
        (A::UsageLimited, N::UsageLimited),
        (A::BudgetLimited, N::BudgetLimited),
    ]
    .into_iter()
    .filter_map(|(attention, kind)| card.attention.contains(&attention).then_some(kind))
    .collect()
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
            let stopped = self.runtimes.get(thread_id).is_some_and(|(previous, _)| {
                matches!(
                    previous,
                    RuntimeStatus::Working | RuntimeStatus::WaitingHuman
                ) && *runtime == RuntimeStatus::Ready
            });
            // Idle does not establish a successful turn, task or delivery.
            if stopped {
                let key = format!("activity-stopped:{thread_id}");
                emitted.insert(
                    key.clone(),
                    NotificationEvent {
                        key,
                        item_id: thread_id.clone(),
                        kind: NotificationKind::ActivityStopped,
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
                emitted.remove(&format!("activity-stopped:{thread_id}"));
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
