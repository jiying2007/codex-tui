//! Shared observed-session reasons. No WorkCard, SQLite or workflow authority.
use crate::{
    domain::{AttentionReason, RuntimeStatus, ThreadSummary},
    forge::ForgeObservation,
    git::GitContext,
    goal::{GoalObservation, GoalStatus},
};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SessionAttentionReason {
    ApprovalRequired,
    UserInputRequired,
    SystemError,
    MarkedUnread,
    ConflictRisk,
    GoalBlocked,
    UsageLimited,
    BudgetLimited,
    ReviewUnseen,
    PipelineFailed,
    ChangeRequested,
}

impl SessionAttentionReason {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ApprovalRequired => "approval",
            Self::UserInputRequired => "input",
            Self::SystemError => "error",
            Self::MarkedUnread => "unread",
            Self::ConflictRisk => "conflict",
            Self::GoalBlocked => "goal-blocked",
            Self::UsageLimited => "usage-limited",
            Self::BudgetLimited => "budget-limited",
            Self::ReviewUnseen => "review",
            Self::PipelineFailed => "pipeline-failed",
            Self::ChangeRequested => "change-requested",
        }
    }
}

pub(crate) fn observed_reasons(
    thread: &ThreadSummary,
    git: Option<&GitContext>,
    collisions: usize,
    goal: Option<&GoalObservation>,
    forge: Option<&ForgeObservation>,
    locally_done: bool,
) -> BTreeSet<SessionAttentionReason> {
    use SessionAttentionReason as R;
    let mut reasons = thread
        .attention
        .iter()
        .map(|reason| match reason {
            AttentionReason::ApprovalRequired => R::ApprovalRequired,
            AttentionReason::UserInputRequired => R::UserInputRequired,
            AttentionReason::SystemError => R::SystemError,
            AttentionReason::MarkedUnread => R::MarkedUnread,
            AttentionReason::ReadyForReview => R::ReviewUnseen,
        })
        .collect::<BTreeSet<_>>();
    if collisions > 0 {
        reasons.insert(R::ConflictRisk);
    }
    let status = goal.map(|goal| goal.status);
    match status {
        Some(GoalStatus::Blocked) => {
            reasons.insert(R::GoalBlocked);
        }
        Some(GoalStatus::UsageLimited) => {
            reasons.insert(R::UsageLimited);
        }
        Some(GoalStatus::BudgetLimited) => {
            reasons.insert(R::BudgetLimited);
        }
        _ => {}
    }
    let branch = git.and_then(|git| git.branch.as_deref());
    let change_request =
        branch.and_then(|branch| forge.and_then(|forge| forge.change_request_for_branch(branch)));
    let pipeline =
        branch.and_then(|branch| forge.and_then(|forge| forge.pipeline_for_branch(branch)));
    if pipeline.is_some_and(|pipeline| pipeline.status.eq_ignore_ascii_case("failed")) {
        reasons.insert(R::PipelineFailed);
    }
    if forge
        .and_then(|forge| forge.review.as_ref())
        .is_some_and(|review| {
            review.unresolved_discussions > 0 || review.changes_requested_by_count > 0
        })
    {
        reasons.insert(R::ChangeRequested);
    }
    // Review is an observation, not acceptance. This matches the old Board's
    // review promotion without constructing a workflow/card or stage string.
    let review = match status {
        Some(GoalStatus::Complete) => true,
        Some(GoalStatus::Paused) => change_request.is_some(),
        Some(_) => false,
        None => match thread.runtime {
            RuntimeStatus::Ready => {
                git.is_some_and(|git| git.is_repository && git.dirty) || change_request.is_some()
            }
            RuntimeStatus::Inactive | RuntimeStatus::Unknown => change_request.is_some(),
            _ => false,
        },
    };
    if review && !locally_done {
        reasons.insert(R::ReviewUnseen);
    }
    reasons
}
