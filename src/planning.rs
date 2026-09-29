use crate::domain::{AttentionReason, RuntimeStatus, ThreadId, ThreadSummary};
use crate::git::GitContext;
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
    ReviewUnseen,
}

impl PlanningAttention {
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ApprovalRequired => "approval",
            Self::UserInputRequired => "input",
            Self::SystemError => "error",
            Self::MarkedUnread => "unread",
            Self::ConflictRisk => "conflict",
            Self::ReviewUnseen => "review",
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
    let thread = input.thread;
    let local = input
        .local
        .cloned()
        .unwrap_or_else(|| WorkCardRecord::implicit_thread(&thread.id));

    let git_dirty = input.git.is_some_and(|git| git.is_repository && git.dirty);
    let (stage, stage_reason) = derive_stage(&thread.runtime, git_dirty, &local.overlay);

    let mut attention = thread
        .attention
        .iter()
        .filter_map(map_attention)
        .collect::<BTreeSet<_>>();
    if input.collision_count > 0 {
        attention.insert(PlanningAttention::ConflictRisk);
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
        links: local.links,
        provenance,
    }
}

pub fn reconcile_scratch_card(scratch: &ScratchWork) -> WorkCardProjection {
    let stage = match scratch.state {
        ScratchState::Inbox => WorkflowStage::Inbox,
        ScratchState::Ready => WorkflowStage::Ready,
        ScratchState::Done => WorkflowStage::Done,
    };
    WorkCardProjection {
        local_id: scratch.id.clone(),
        anchor: SourceRef {
            kind: SourceKind::ScratchWork,
            value: scratch.id.clone(),
        },
        title: scratch.title.clone(),
        workspace: scratch.workspace.clone(),
        stage,
        stage_reason: format!("local ScratchWork state is {}", stage.label()),
        attention: BTreeSet::new(),
        snoozed: false,
        overlay: WorkCardOverlay {
            note: scratch.note.clone(),
            priority: scratch.priority,
            ..WorkCardOverlay::default()
        },
        links: vec![],
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
) -> (WorkflowStage, String) {
    if overlay.done_at_unix_ms.is_some() {
        return (
            WorkflowStage::Done,
            "completion explicitly acknowledged locally".into(),
        );
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
}
