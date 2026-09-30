use crate::domain::{AttentionReason, RuntimeStatus, ThreadId, ThreadSummary};
use crate::forge::{ForgeFreshness, ForgeIssueSummary, ForgeObservation};
use crate::git::GitContext;
use crate::goal::{GoalObservation, GoalStatus};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    ScratchWork,
    CodexThread,
    ForgeWorkItem,
    Goal,
    Worktree,
    ChangeRequest,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceRef {
    pub kind: SourceKind,
    pub value: String,
}

impl SourceRef {
    pub fn codex_thread(thread_id: &ThreadId) -> Self {
        Self {
            kind: SourceKind::CodexThread,
            value: thread_id.0.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinkRole {
    PrimaryThread,
    ExperimentThread,
    ReviewThread,
    Goal,
    Worktree,
    ChangeRequest,
    RelatedWorkItem,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCardLink {
    pub role: LinkRole,
    pub source: SourceRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkflowStage {
    Inbox,
    Ready,
    Working,
    Review,
    Done,
}

impl WorkflowStage {
    pub const ALL: [Self; 5] = [
        Self::Inbox,
        Self::Ready,
        Self::Working,
        Self::Review,
        Self::Done,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Ready => "Ready",
            Self::Working => "Working",
            Self::Review => "Review",
            Self::Done => "Done",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Freshness {
    Fresh,
    Aging,
    Stale,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: String,
    pub observed_at_unix_ms: Option<u64>,
    pub source_revision: Option<String>,
    pub freshness: Freshness,
    pub degraded_reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCardOverlay {
    pub title_override: Option<String>,
    pub note: Option<String>,
    pub pinned: bool,
    pub tags: BTreeSet<String>,
    pub priority: Option<i32>,
    pub manual_ready: bool,
    pub done_at_unix_ms: Option<u64>,
    pub snooze_until_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCardRecord {
    pub local_id: String,
    pub anchor: SourceRef,
    pub links: Vec<WorkCardLink>,
    pub overlay: WorkCardOverlay,
}

impl WorkCardRecord {
    pub fn implicit_thread(thread_id: &ThreadId) -> Self {
        Self {
            local_id: format!("thread:{}", thread_id.0),
            anchor: SourceRef::codex_thread(thread_id),
            links: vec![],
            overlay: WorkCardOverlay::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlanningAttention {
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

impl PlanningAttention {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkCardProjection {
    pub local_id: String,
    pub anchor: SourceRef,
    pub title: String,
    pub workspace: Option<String>,
    pub stage: WorkflowStage,
    pub stage_reason: String,
    pub attention: BTreeSet<PlanningAttention>,
    pub snoozed: bool,
    pub overlay: WorkCardOverlay,
    pub links: Vec<WorkCardLink>,
    pub goal: Option<GoalObservation>,
    pub provenance: Vec<Provenance>,
}

impl WorkCardProjection {
    pub fn needs_you(&self) -> bool {
        !self.snoozed && !self.attention.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScratchState {
    Inbox,
    Ready,
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScratchWork {
    pub id: String,
    pub title: String,
    pub note: Option<String>,
    pub workspace: Option<String>,
    pub priority: Option<i32>,
    pub state: ScratchState,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SavedViewLayout {
    List,
    Board,
    ReviewQueue,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedView {
    pub id: String,
    pub name: String,
    pub source_scope: String,
    pub filter: String,
    pub group_by: Option<String>,
    pub order_by: Option<String>,
    pub layout: SavedViewLayout,
    pub visible_fields: Vec<String>,
}

impl SavedViewLayout {
    pub const fn label(self) -> &'static str {
        match self {
            Self::List => "List",
            Self::Board => "Board",
            Self::ReviewQueue => "Review Queue",
        }
    }
}

pub fn builtin_saved_views() -> Vec<SavedView> {
    vec![
        SavedView {
            id: "builtin:all".into(),
            name: "All Work".into(),
            source_scope: "all".into(),
            filter: String::new(),
            group_by: Some("stage".into()),
            order_by: Some("priority".into()),
            layout: SavedViewLayout::Board,
            visible_fields: vec!["stage".into(), "attention".into(), "workspace".into()],
        },
        SavedView {
            id: "builtin:attention".into(),
            name: "Needs You".into(),
            source_scope: "all".into(),
            filter: "status:needs-you".into(),
            group_by: Some("workspace".into()),
            order_by: Some("priority".into()),
            layout: SavedViewLayout::List,
            visible_fields: vec!["attention".into(), "stage".into(), "workspace".into()],
        },
        SavedView {
            id: "builtin:review".into(),
            name: "Needs Review".into(),
            source_scope: "all".into(),
            filter: "stage:review".into(),
            group_by: Some("workspace".into()),
            order_by: Some("priority".into()),
            layout: SavedViewLayout::ReviewQueue,
            visible_fields: vec!["workspace".into(), "attention".into()],
        },
        SavedView {
            id: "builtin:forge".into(),
            name: "Forge Work".into(),
            source_scope: "all".into(),
            filter: "source:forge".into(),
            group_by: Some("workspace".into()),
            order_by: Some("priority".into()),
            layout: SavedViewLayout::List,
            visible_fields: vec!["workspace".into(), "stage".into(), "attention".into()],
        },
    ]
}

pub fn apply_saved_view<'a>(
    cards: &'a [WorkCardProjection],
    view: &SavedView,
) -> Vec<&'a WorkCardProjection> {
    let mut selected = cards
        .iter()
        .filter(|card| card_matches_filter(card, &view.filter))
        .collect::<Vec<_>>();

    match view.order_by.as_deref() {
        Some("title") => selected.sort_by(|left, right| left.title.cmp(&right.title)),
        Some("stage") => selected.sort_by(|left, right| {
            left.stage
                .cmp(&right.stage)
                .then_with(|| left.title.cmp(&right.title))
        }),
        Some("workspace") => selected.sort_by(|left, right| {
            left.workspace
                .as_deref()
                .unwrap_or("")
                .cmp(right.workspace.as_deref().unwrap_or(""))
                .then_with(|| left.title.cmp(&right.title))
        }),
        _ => selected.sort_by(|left, right| {
            left.overlay
                .priority
                .unwrap_or(i32::MAX)
                .cmp(&right.overlay.priority.unwrap_or(i32::MAX))
                .then_with(|| left.title.cmp(&right.title))
        }),
    }
    selected
}

pub fn saved_view_group_key(card: &WorkCardProjection, group_by: Option<&str>) -> String {
    match group_by {
        Some("stage") => card.stage.label().to_string(),
        Some("workspace") => card
            .workspace
            .clone()
            .unwrap_or_else(|| "No workspace".into()),
        Some("source") => format!("{:?}", card.anchor.kind),
        _ => String::new(),
    }
}

fn card_matches_filter(card: &WorkCardProjection, filter: &str) -> bool {
    let normalized = filter.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return true;
    }

    normalized.split_whitespace().all(|token| {
        if token == "status:needs-you" || token == "needs-you" {
            return card.needs_you();
        }
        if let Some(stage) = token.strip_prefix("stage:") {
            return card.stage.label().eq_ignore_ascii_case(stage);
        }
        if let Some(workspace) = token.strip_prefix("workspace:") {
            return card
                .workspace
                .as_deref()
                .is_some_and(|value| value.to_ascii_lowercase().contains(workspace));
        }
        if let Some(tag) = token.strip_prefix("tag:") {
            return card
                .overlay
                .tags
                .iter()
                .any(|value| value.eq_ignore_ascii_case(tag));
        }
        if let Some(goal) = token.strip_prefix("goal:") {
            return card.goal.as_ref().is_some_and(|observation| {
                observation.objective.to_ascii_lowercase().contains(goal)
                    || observation
                        .status
                        .wire()
                        .to_ascii_lowercase()
                        .contains(goal)
            });
        }
        if let Some(source) = token.strip_prefix("source:") {
            return match source {
                "scratch" => card.anchor.kind == SourceKind::ScratchWork,
                "thread" | "codex" => card.anchor.kind == SourceKind::CodexThread,
                "forge" => {
                    card.anchor.kind == SourceKind::ForgeWorkItem
                        || card.links.iter().any(|link| {
                            matches!(
                                link.source.kind,
                                SourceKind::ForgeWorkItem | SourceKind::ChangeRequest
                            )
                        })
                }
                _ => false,
            };
        }

        let haystack = format!(
            "{} {} {} {} {}",
            card.title,
            card.workspace.as_deref().unwrap_or(""),
            card.stage.label(),
            card.goal
                .as_ref()
                .map(|goal| goal.objective.as_str())
                .unwrap_or(""),
            card.attention
                .iter()
                .map(PlanningAttention::label)
                .collect::<Vec<_>>()
                .join(" ")
        )
        .to_ascii_lowercase();
        haystack.contains(token)
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalNote {
    pub owner: SourceRef,
    pub text: String,
    pub updated_at_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub source: SourceRef,
    pub label: Option<String>,
    pub note: Option<String>,
    pub created_at_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotSlot {
    pub slot: u8,
    pub target: SourceRef,
    pub updated_at_unix_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlanningSnapshot {
    pub cards: Vec<WorkCardRecord>,
    pub scratch: Vec<ScratchWork>,
    pub saved_views: Vec<SavedView>,
    pub notes: Vec<LocalNote>,
    pub bookmarks: Vec<Bookmark>,
    pub hot_slots: Vec<HotSlot>,
}

#[derive(Clone, Debug)]
pub struct ReconcileInput<'a> {
    pub thread: &'a ThreadSummary,
    pub git: Option<&'a GitContext>,
    pub local: Option<&'a WorkCardRecord>,
    pub collision_count: usize,
    pub backend_observed_at_unix_ms: Option<u64>,
    pub backend_error: Option<&'a str>,
    pub now_unix_ms: u64,
}

pub fn reconcile_thread_card(input: ReconcileInput<'_>) -> WorkCardProjection {
    reconcile_thread_card_with_goal(input, None)
}

pub fn reconcile_thread_card_with_goal(
    input: ReconcileInput<'_>,
    goal: Option<&GoalObservation>,
) -> WorkCardProjection {
    reconcile_thread_card_with_goal_and_forge(input, goal, None)
}

pub fn reconcile_thread_card_with_goal_and_forge(
    input: ReconcileInput<'_>,
    goal: Option<&GoalObservation>,
    forge: Option<&ForgeObservation>,
) -> WorkCardProjection {
    let thread = input.thread;
    let local = input
        .local
        .cloned()
        .unwrap_or_else(|| WorkCardRecord::implicit_thread(&thread.id));

    let git_dirty = input.git.is_some_and(|git| git.is_repository && git.dirty);
    let (mut stage, mut stage_reason) =
        derive_stage(&thread.runtime, git_dirty, &local.overlay, goal);

    let branch = input.git.and_then(|git| git.branch.as_deref());
    let change_request = branch.and_then(|branch| {
        forge.and_then(|observation| observation.change_request_for_branch(branch))
    });
    let pipeline = branch
        .and_then(|branch| forge.and_then(|observation| observation.pipeline_for_branch(branch)));

    if change_request.is_some()
        && matches!(stage, WorkflowStage::Inbox | WorkflowStage::Ready)
        && local.overlay.done_at_unix_ms.is_none()
    {
        stage = WorkflowStage::Review;
        stage_reason = "open forge change request awaits review/delivery".into();
    }

    let mut attention = thread
        .attention
        .iter()
        .filter_map(map_attention)
        .collect::<BTreeSet<_>>();
    if input.collision_count > 0 {
        attention.insert(PlanningAttention::ConflictRisk);
    }
    if let Some(goal) = goal {
        match goal.status {
            GoalStatus::Blocked => {
                attention.insert(PlanningAttention::GoalBlocked);
            }
            GoalStatus::UsageLimited => {
                attention.insert(PlanningAttention::UsageLimited);
            }
            GoalStatus::BudgetLimited => {
                attention.insert(PlanningAttention::BudgetLimited);
            }
            GoalStatus::Complete => {
                if local.overlay.done_at_unix_ms.is_none() {
                    attention.insert(PlanningAttention::ReviewUnseen);
                }
            }
            GoalStatus::Active | GoalStatus::Paused => {}
        }
    }
    if pipeline.is_some_and(|pipeline| pipeline.status.eq_ignore_ascii_case("failed")) {
        attention.insert(PlanningAttention::PipelineFailed);
    }
    if forge
        .and_then(|observation| observation.review.as_ref())
        .is_some_and(|review| review.unresolved_discussions > 0)
    {
        attention.insert(PlanningAttention::ChangeRequested);
    }
    if stage == WorkflowStage::Review && local.overlay.done_at_unix_ms.is_none() {
        attention.insert(PlanningAttention::ReviewUnseen);
    }

    let snoozed = local
        .overlay
        .snooze_until_unix_ms
        .is_some_and(|until| until > input.now_unix_ms);

    let title = local
        .overlay
        .title_override
        .clone()
        .unwrap_or_else(|| thread.display_title().to_string());

    let mut provenance = vec![Provenance {
        source: "codex".into(),
        observed_at_unix_ms: input.backend_observed_at_unix_ms,
        source_revision: None,
        freshness: freshness(
            input.backend_observed_at_unix_ms,
            input.now_unix_ms,
            input.backend_error.is_some(),
        ),
        degraded_reason: input.backend_error.map(ToOwned::to_owned),
    }];

    if let Some(goal) = goal {
        provenance.push(Provenance {
            source: "goal".into(),
            observed_at_unix_ms: Some(goal.observed_at_unix_ms),
            source_revision: Some(goal.updated_at.to_string()),
            freshness: freshness(Some(goal.observed_at_unix_ms), input.now_unix_ms, false),
            degraded_reason: None,
        });
    }

    if let Some(git) = input.git {
        provenance.push(Provenance {
            source: "git".into(),
            observed_at_unix_ms: (git.observed_at_unix_ms > 0).then_some(git.observed_at_unix_ms),
            source_revision: git.head.clone(),
            freshness: freshness(
                (git.observed_at_unix_ms > 0).then_some(git.observed_at_unix_ms),
                input.now_unix_ms,
                git.error.is_some(),
            ),
            degraded_reason: git.error.clone(),
        });
    }

    let mut links = local.links;
    if let Some(forge) = forge {
        let identity = forge.identity.as_ref();
        provenance.push(Provenance {
            source: identity.map_or_else(
                || "forge".into(),
                |identity| format!("forge:{}", identity.host),
            ),
            observed_at_unix_ms: Some(forge.observed_at_unix_ms),
            source_revision: identity.map(|identity| identity.project_id.clone()),
            freshness: match forge.freshness {
                ForgeFreshness::Fresh => Freshness::Fresh,
                ForgeFreshness::Aging => Freshness::Aging,
                ForgeFreshness::Stale => Freshness::Stale,
                ForgeFreshness::Unavailable => Freshness::Unavailable,
            },
            degraded_reason: forge.error.clone(),
        });

        if let (Some(identity), Some(change_request)) = (identity, change_request) {
            let source = SourceRef {
                kind: SourceKind::ChangeRequest,
                value: format!(
                    "{}://{}/projects/{}/merge-requests/{}",
                    identity.provider.label(),
                    identity.host,
                    identity.project_id,
                    change_request.iid
                ),
            };
            if !links.iter().any(|link| link.source == source) {
                links.push(WorkCardLink {
                    role: LinkRole::ChangeRequest,
                    source,
                });
            }
        }
    }

    WorkCardProjection {
        local_id: local.local_id,
        anchor: local.anchor,
        title,
        workspace: Some(thread.workspace.clone()),
        stage,
        stage_reason,
        attention,
        snoozed,
        overlay: local.overlay,
        links,
        goal: goal.cloned(),
        provenance,
    }
}

pub fn forge_issue_source_ref(
    observation: &ForgeObservation,
    issue: &ForgeIssueSummary,
) -> Option<SourceRef> {
    let identity = observation.identity.as_ref()?;
    Some(SourceRef {
        kind: SourceKind::ForgeWorkItem,
        value: format!(
            "{}://{}/projects/{}/issues/{}",
            identity.provider.label(),
            identity.host,
            identity.project_id,
            issue.iid
        ),
    })
}

pub fn reconcile_forge_issue_card(
    observation: &ForgeObservation,
    issue: &ForgeIssueSummary,
    local: Option<&WorkCardRecord>,
    now_unix_ms: u64,
) -> Option<WorkCardProjection> {
    let identity = observation.identity.as_ref()?;
    let anchor = forge_issue_source_ref(observation, issue)?;
    let record = local.cloned().unwrap_or_else(|| WorkCardRecord {
        local_id: format!(
            "forge:{}:{}:issue:{}",
            identity.host, identity.project_id, issue.iid
        ),
        anchor: anchor.clone(),
        links: vec![],
        overlay: WorkCardOverlay::default(),
    });

    let (stage, stage_reason) = if record.overlay.done_at_unix_ms.is_some() {
        (
            WorkflowStage::Done,
            "completion explicitly acknowledged locally".into(),
        )
    } else if issue.state.eq_ignore_ascii_case("closed") {
        (WorkflowStage::Done, "GitLab issue is closed".into())
    } else if record.overlay.manual_ready {
        (
            WorkflowStage::Ready,
            "open GitLab issue selected locally as ready".into(),
        )
    } else {
        (
            WorkflowStage::Inbox,
            "open GitLab issue projected into personal planning".into(),
        )
    };

    let snoozed = record
        .overlay
        .snooze_until_unix_ms
        .is_some_and(|until| until > now_unix_ms);

    Some(WorkCardProjection {
        local_id: record.local_id,
        anchor,
        title: record
            .overlay
            .title_override
            .clone()
            .unwrap_or_else(|| format!("#{} {}", issue.iid, issue.title)),
        workspace: Some(identity.path_with_namespace.clone()),
        stage,
        stage_reason,
        attention: BTreeSet::new(),
        snoozed,
        overlay: record.overlay,
        links: record.links,
        goal: None,
        provenance: vec![Provenance {
            source: format!("forge:{}", identity.host),
            observed_at_unix_ms: Some(observation.observed_at_unix_ms),
            source_revision: issue.updated_at.clone(),
            freshness: match observation.freshness {
                ForgeFreshness::Fresh => Freshness::Fresh,
                ForgeFreshness::Aging => Freshness::Aging,
                ForgeFreshness::Stale => Freshness::Stale,
                ForgeFreshness::Unavailable => Freshness::Unavailable,
            },
            degraded_reason: observation.error.clone(),
        }],
    })
}

pub fn reconcile_scratch_card(scratch: &ScratchWork) -> WorkCardProjection {
    reconcile_scratch_card_with_local(scratch, None, scratch.updated_at_unix_ms)
}

pub fn reconcile_scratch_card_with_local(
    scratch: &ScratchWork,
    local: Option<&WorkCardRecord>,
    now_unix_ms: u64,
) -> WorkCardProjection {
    let stage = match scratch.state {
        ScratchState::Inbox => WorkflowStage::Inbox,
        ScratchState::Ready => WorkflowStage::Ready,
        ScratchState::Done => WorkflowStage::Done,
    };
    let anchor = SourceRef {
        kind: SourceKind::ScratchWork,
        value: scratch.id.clone(),
    };
    let mut record = local.cloned().unwrap_or_else(|| WorkCardRecord {
        local_id: scratch.id.clone(),
        anchor: anchor.clone(),
        links: vec![],
        overlay: WorkCardOverlay::default(),
    });
    if record.overlay.note.is_none() {
        record.overlay.note.clone_from(&scratch.note);
    }
    if record.overlay.priority.is_none() {
        record.overlay.priority = scratch.priority;
    }
    let snoozed = record
        .overlay
        .snooze_until_unix_ms
        .is_some_and(|until| until > now_unix_ms);

    WorkCardProjection {
        local_id: record.local_id,
        anchor,
        title: record
            .overlay
            .title_override
            .clone()
            .unwrap_or_else(|| scratch.title.clone()),
        workspace: scratch.workspace.clone(),
        stage,
        stage_reason: format!("local ScratchWork state is {}", stage.label()),
        attention: BTreeSet::new(),
        snoozed,
        overlay: record.overlay,
        links: record.links,
        goal: None,
        provenance: vec![Provenance {
            source: "local".into(),
            observed_at_unix_ms: Some(scratch.updated_at_unix_ms),
            source_revision: Some(scratch.updated_at_unix_ms.to_string()),
            freshness: Freshness::Fresh,
            degraded_reason: None,
        }],
    }
}

fn derive_stage(
    runtime: &RuntimeStatus,
    git_dirty: bool,
    overlay: &WorkCardOverlay,
    goal: Option<&GoalObservation>,
) -> (WorkflowStage, String) {
    if overlay.done_at_unix_ms.is_some() {
        return (
            WorkflowStage::Done,
            "completion explicitly acknowledged locally".into(),
        );
    }
    if let Some(goal) = goal {
        match goal.status {
            GoalStatus::Active => {
                return (WorkflowStage::Working, "Codex Goal is active".into());
            }
            GoalStatus::Paused => {
                return (WorkflowStage::Ready, "Codex Goal is paused".into());
            }
            GoalStatus::Blocked => {
                return (WorkflowStage::Working, "Codex Goal is blocked".into());
            }
            GoalStatus::UsageLimited => {
                return (WorkflowStage::Working, "Codex Goal is usage-limited".into());
            }
            GoalStatus::BudgetLimited => {
                return (
                    WorkflowStage::Working,
                    "Codex Goal is budget-limited".into(),
                );
            }
            GoalStatus::Complete => {
                return (
                    WorkflowStage::Review,
                    "Codex Goal is complete and awaits human review".into(),
                );
            }
        }
    }
    match runtime {
        RuntimeStatus::Working | RuntimeStatus::WaitingHuman => (
            WorkflowStage::Working,
            format!("Codex thread runtime is {}", runtime.label()),
        ),
        RuntimeStatus::Ready if git_dirty => (
            WorkflowStage::Review,
            "Codex thread is ready and its worktree has unreviewed changes".into(),
        ),
        RuntimeStatus::Ready => (
            WorkflowStage::Ready,
            "Codex thread is idle/ready with no projected dirty worktree".into(),
        ),
        RuntimeStatus::SystemError => (
            WorkflowStage::Working,
            "Codex thread has a system error while work remains active".into(),
        ),
        RuntimeStatus::Inactive if overlay.manual_ready => {
            (WorkflowStage::Ready, "selected locally as ready".into())
        }
        RuntimeStatus::Inactive => (
            WorkflowStage::Inbox,
            "inactive thread has not been selected as ready".into(),
        ),
    }
}

fn map_attention(reason: &AttentionReason) -> Option<PlanningAttention> {
    match reason {
        AttentionReason::ApprovalRequired => Some(PlanningAttention::ApprovalRequired),
        AttentionReason::UserInputRequired => Some(PlanningAttention::UserInputRequired),
        AttentionReason::SystemError => Some(PlanningAttention::SystemError),
        AttentionReason::MarkedUnread => Some(PlanningAttention::MarkedUnread),
        AttentionReason::ReadyForReview => Some(PlanningAttention::ReviewUnseen),
    }
}

fn freshness(observed_at: Option<u64>, now: u64, unavailable: bool) -> Freshness {
    if unavailable {
        return Freshness::Unavailable;
    }
    let Some(observed_at) = observed_at else {
        return Freshness::Unavailable;
    };
    let age = now.saturating_sub(observed_at);
    if age <= 10_000 {
        Freshness::Fresh
    } else if age <= 60_000 {
        Freshness::Aging
    } else {
        Freshness::Stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    fn first_thread() -> ThreadSummary {
        FakeBackend::seeded().snapshot().threads.remove(0)
    }

    #[test]
    fn builtin_attention_view_filters_attention_without_changing_stage() {
        let mut thread = first_thread();
        thread.runtime = RuntimeStatus::Working;
        thread.attention = vec![AttentionReason::ApprovalRequired];
        let card = reconcile_thread_card(ReconcileInput {
            thread: &thread,
            git: None,
            local: None,
            collision_count: 0,
            backend_observed_at_unix_ms: Some(100),
            backend_error: None,
            now_unix_ms: 100,
        });
        let cards = vec![card];
        let views = builtin_saved_views();
        let attention = views
            .iter()
            .find(|view| view.id == "builtin:attention")
            .expect("attention view");
        let visible = apply_saved_view(&cards, attention);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].stage, WorkflowStage::Working);
        assert!(visible[0].needs_you());
    }

    #[test]
    fn saved_view_filter_supports_stage_workspace_tag_and_source() {
        let mut thread = first_thread();
        thread.runtime = RuntimeStatus::Ready;
        let mut local = WorkCardRecord::implicit_thread(&thread.id);
        local.overlay.tags.insert("kws".into());
        let card = reconcile_thread_card(ReconcileInput {
            thread: &thread,
            git: None,
            local: Some(&local),
            collision_count: 0,
            backend_observed_at_unix_ms: Some(100),
            backend_error: None,
            now_unix_ms: 100,
        });
        let view = SavedView {
            id: "test".into(),
            name: "test".into(),
            source_scope: "all".into(),
            filter: "stage:ready tag:kws source:thread".into(),
            group_by: Some("workspace".into()),
            order_by: Some("title".into()),
            layout: SavedViewLayout::List,
            visible_fields: vec![],
        };
        assert_eq!(apply_saved_view(&[card], &view).len(), 1);
    }

    fn goal(status: GoalStatus) -> GoalObservation {
        GoalObservation {
            thread_id: ThreadId::new("thread-impl"),
            objective: "Ship M4".into(),
            status,
            token_budget: Some(10_000),
            tokens_used: 1_000,
            time_used_seconds: 60,
            created_at: 1,
            updated_at: 2,
            observed_at_unix_ms: 100,
        }
    }

    #[test]
    fn active_goal_drives_working_stage_without_becoming_local_authority() {
        let mut thread = first_thread();
        thread.runtime = RuntimeStatus::Inactive;
        let goal = goal(GoalStatus::Active);
        let card = reconcile_thread_card_with_goal(
            ReconcileInput {
                thread: &thread,
                git: None,
                local: None,
                collision_count: 0,
                backend_observed_at_unix_ms: Some(100),
                backend_error: None,
                now_unix_ms: 100,
            },
            Some(&goal),
        );
        assert_eq!(card.stage, WorkflowStage::Working);
        assert_eq!(card.stage_reason, "Codex Goal is active");
        assert!(card.provenance.iter().any(|p| p.source == "goal"));
    }

    #[test]
    fn blocked_and_limited_goals_are_attention_not_workflow_columns() {
        let thread = first_thread();
        for (status, attention) in [
            (GoalStatus::Blocked, PlanningAttention::GoalBlocked),
            (GoalStatus::UsageLimited, PlanningAttention::UsageLimited),
            (GoalStatus::BudgetLimited, PlanningAttention::BudgetLimited),
        ] {
            let goal = goal(status);
            let card = reconcile_thread_card_with_goal(
                ReconcileInput {
                    thread: &thread,
                    git: None,
                    local: None,
                    collision_count: 0,
                    backend_observed_at_unix_ms: Some(100),
                    backend_error: None,
                    now_unix_ms: 100,
                },
                Some(&goal),
            );
            assert_eq!(card.stage, WorkflowStage::Working);
            assert!(card.attention.contains(&attention));
        }
    }

    #[test]
    fn completed_goal_projects_to_review_until_local_completion_is_acknowledged() {
        let thread = first_thread();
        let goal = goal(GoalStatus::Complete);
        let card = reconcile_thread_card_with_goal(
            ReconcileInput {
                thread: &thread,
                git: None,
                local: None,
                collision_count: 0,
                backend_observed_at_unix_ms: Some(100),
                backend_error: None,
                now_unix_ms: 100,
            },
            Some(&goal),
        );
        assert_eq!(card.stage, WorkflowStage::Review);
        assert!(card.attention.contains(&PlanningAttention::ReviewUnseen));
    }

    #[test]
    fn workflow_and_attention_are_orthogonal() {
        let mut thread = first_thread();
        thread.runtime = RuntimeStatus::Working;
        thread.attention = vec![AttentionReason::ApprovalRequired];
        let card = reconcile_thread_card(ReconcileInput {
            thread: &thread,
            git: None,
            local: None,
            collision_count: 0,
            backend_observed_at_unix_ms: Some(100),
            backend_error: None,
            now_unix_ms: 100,
        });
        assert_eq!(card.stage, WorkflowStage::Working);
        assert!(
            card.attention
                .contains(&PlanningAttention::ApprovalRequired)
        );
        assert!(card.needs_you());
    }

    #[test]
    fn collision_adds_attention_without_rewriting_workflow() {
        let mut thread = first_thread();
        thread.runtime = RuntimeStatus::Working;
        let card = reconcile_thread_card(ReconcileInput {
            thread: &thread,
            git: None,
            local: None,
            collision_count: 2,
            backend_observed_at_unix_ms: Some(100),
            backend_error: None,
            now_unix_ms: 100,
        });
        assert_eq!(card.stage, WorkflowStage::Working);
        assert!(card.attention.contains(&PlanningAttention::ConflictRisk));
    }

    #[test]
    fn ready_thread_with_dirty_worktree_is_review_with_reason() {
        let mut thread = first_thread();
        thread.runtime = RuntimeStatus::Ready;
        let mut git = GitContext::pending(thread.id.clone(), "/repo");
        git.is_repository = true;
        git.dirty = true;
        git.observed_at_unix_ms = 100;

        let card = reconcile_thread_card(ReconcileInput {
            thread: &thread,
            git: Some(&git),
            local: None,
            collision_count: 0,
            backend_observed_at_unix_ms: Some(100),
            backend_error: None,
            now_unix_ms: 100,
        });
        assert_eq!(card.stage, WorkflowStage::Review);
        assert!(card.stage_reason.contains("unreviewed changes"));
        assert!(card.attention.contains(&PlanningAttention::ReviewUnseen));
    }

    #[test]
    fn snooze_suppresses_needs_you_not_source_attention() {
        let mut thread = first_thread();
        thread.runtime = RuntimeStatus::WaitingHuman;
        thread.attention = vec![AttentionReason::UserInputRequired];
        let mut local = WorkCardRecord::implicit_thread(&thread.id);
        local.overlay.snooze_until_unix_ms = Some(200);

        let card = reconcile_thread_card(ReconcileInput {
            thread: &thread,
            git: None,
            local: Some(&local),
            collision_count: 0,
            backend_observed_at_unix_ms: Some(100),
            backend_error: None,
            now_unix_ms: 100,
        });
        assert!(
            card.attention
                .contains(&PlanningAttention::UserInputRequired)
        );
        assert!(card.snoozed);
        assert!(!card.needs_you());
    }

    #[test]
    fn explicit_done_is_never_inferred_from_idle() {
        let mut thread = first_thread();
        thread.runtime = RuntimeStatus::Inactive;
        let implicit = reconcile_thread_card(ReconcileInput {
            thread: &thread,
            git: None,
            local: None,
            collision_count: 0,
            backend_observed_at_unix_ms: Some(100),
            backend_error: None,
            now_unix_ms: 100,
        });
        assert_eq!(implicit.stage, WorkflowStage::Inbox);

        let mut local = WorkCardRecord::implicit_thread(&thread.id);
        local.overlay.done_at_unix_ms = Some(90);
        let done = reconcile_thread_card(ReconcileInput {
            thread: &thread,
            git: None,
            local: Some(&local),
            collision_count: 0,
            backend_observed_at_unix_ms: Some(100),
            backend_error: None,
            now_unix_ms: 100,
        });
        assert_eq!(done.stage, WorkflowStage::Done);
    }
    #[test]
    fn forge_change_request_and_failed_pipeline_project_without_becoming_local_authority() {
        use crate::forge::{
            CapabilityState, ChangeRequestSummary, ForgeCapability, ForgeFreshness, ForgeIdentity,
            ForgeObservation, ForgeProviderKind, PipelineSummary,
        };
        use std::collections::BTreeMap;

        let mut thread = first_thread();
        thread.runtime = RuntimeStatus::Ready;

        let mut git = GitContext::pending(thread.id.clone(), "/repo");
        git.is_repository = true;
        git.branch = Some("feature/m6".into());
        git.observed_at_unix_ms = 100;

        let forge = ForgeObservation {
            thread_id: thread.id.clone(),
            cwd: "/repo".into(),
            remote_name: Some("origin".into()),
            remote_url: Some("git@gitlab.example.com:team/repo.git".into()),
            identity: Some(ForgeIdentity {
                provider: ForgeProviderKind::GitLab,
                host: "gitlab.example.com".into(),
                project_id: "42".into(),
                path_with_namespace: "team/repo".into(),
                web_url: "https://gitlab.example.com/team/repo".into(),
            }),
            capabilities: BTreeMap::from([
                (ForgeCapability::MergeRequests, CapabilityState::Available),
                (ForgeCapability::Pipelines, CapabilityState::Available),
            ]),
            issues: vec![],
            change_requests: vec![ChangeRequestSummary {
                iid: 7,
                title: "Ship M6".into(),
                state: "opened".into(),
                source_branch: "feature/m6".into(),
                target_branch: "main".into(),
                web_url: "https://gitlab.example.com/team/repo/-/merge_requests/7".into(),
                updated_at: None,
                draft: false,
                detailed_merge_status: Some("mergeable".into()),
                blocking_discussions_resolved: Some(true),
            }],
            pipelines: vec![PipelineSummary {
                id: 99,
                status: "failed".into(),
                reference: "feature/m6".into(),
                web_url: "https://gitlab.example.com/team/repo/-/pipelines/99".into(),
                updated_at: None,
            }],
            review: None,
            observed_at_unix_ms: 100,
            freshness: ForgeFreshness::Fresh,
            error: None,
        };

        let card = reconcile_thread_card_with_goal_and_forge(
            ReconcileInput {
                thread: &thread,
                git: Some(&git),
                local: None,
                collision_count: 0,
                backend_observed_at_unix_ms: Some(100),
                backend_error: None,
                now_unix_ms: 100,
            },
            None,
            Some(&forge),
        );

        assert_eq!(card.stage, WorkflowStage::Review);
        assert!(card.attention.contains(&PlanningAttention::PipelineFailed));
        assert!(card.attention.contains(&PlanningAttention::ReviewUnseen));
        assert!(card.links.iter().any(|link| {
            link.role == LinkRole::ChangeRequest
                && link.source.kind == SourceKind::ChangeRequest
                && link.source.value.ends_with("/merge-requests/7")
        }));
        assert!(
            card.provenance
                .iter()
                .any(|provenance| provenance.source == "forge:gitlab.example.com")
        );

        let mut reviewed_forge = forge.clone();
        reviewed_forge.review = Some(crate::forge::ForgeReviewSummary {
            thread_id: thread.id.clone(),
            cwd: "/repo".into(),
            change_request_iid: 7,
            approvals_required: Some(2),
            approvals_left: Some(1),
            approved_by_count: 1,
            discussions_total: 2,
            unresolved_discussions: 1,
            approvals_available: true,
            discussions_available: true,
            observed_at_unix_ms: 101,
            error: None,
        });
        let reviewed = reconcile_thread_card_with_goal_and_forge(
            ReconcileInput {
                thread: &thread,
                git: Some(&git),
                local: None,
                collision_count: 0,
                backend_observed_at_unix_ms: Some(101),
                backend_error: None,
                now_unix_ms: 101,
            },
            None,
            Some(&reviewed_forge),
        );
        assert!(
            reviewed
                .attention
                .contains(&PlanningAttention::ChangeRequested)
        );

        let view = SavedView {
            id: "forge".into(),
            name: "forge".into(),
            source_scope: "all".into(),
            filter: "source:forge".into(),
            group_by: None,
            order_by: None,
            layout: SavedViewLayout::List,
            visible_fields: vec![],
        };
        assert_eq!(apply_saved_view(&[card], &view).len(), 1);
    }
    #[test]
    fn forge_issue_projects_as_dedicated_work_item_without_copying_authority() {
        use crate::forge::{CapabilityState, ForgeCapability, ForgeIdentity, ForgeProviderKind};
        use std::collections::BTreeMap;

        let observation = ForgeObservation {
            thread_id: ThreadId::new("thread"),
            cwd: "/repo".into(),
            remote_name: Some("origin".into()),
            remote_url: Some("git@gitlab.example.com:team/repo.git".into()),
            identity: Some(ForgeIdentity {
                provider: ForgeProviderKind::GitLab,
                host: "gitlab.example.com".into(),
                project_id: "42".into(),
                path_with_namespace: "team/repo".into(),
                web_url: "https://gitlab.example.com/team/repo".into(),
            }),
            capabilities: BTreeMap::from([(ForgeCapability::Issues, CapabilityState::Available)]),
            issues: vec![],
            change_requests: vec![],
            pipelines: vec![],
            review: None,
            observed_at_unix_ms: 100,
            freshness: ForgeFreshness::Fresh,
            error: None,
        };
        let issue = ForgeIssueSummary {
            iid: 12,
            title: "Fix wake-word regression".into(),
            state: "opened".into(),
            web_url: "https://gitlab.example.com/team/repo/-/issues/12".into(),
            updated_at: Some("2026-09-30T00:00:00Z".into()),
        };

        let card =
            reconcile_forge_issue_card(&observation, &issue, None, 100).expect("forge issue card");
        assert_eq!(card.anchor.kind, SourceKind::ForgeWorkItem);
        assert_eq!(
            card.anchor.value,
            "gitlab://gitlab.example.com/projects/42/issues/12"
        );
        assert_eq!(card.workspace.as_deref(), Some("team/repo"));
        assert_eq!(card.stage, WorkflowStage::Inbox);
        assert_eq!(card.title, "#12 Fix wake-word regression");
        assert_eq!(card.provenance[0].source, "forge:gitlab.example.com");

        let view = SavedView {
            id: "forge".into(),
            name: "Forge".into(),
            source_scope: "all".into(),
            filter: "source:forge".into(),
            group_by: None,
            order_by: None,
            layout: SavedViewLayout::List,
            visible_fields: vec![],
        };
        assert_eq!(apply_saved_view(&[card], &view).len(), 1);
    }
}
