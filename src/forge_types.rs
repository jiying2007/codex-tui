use crate::{domain::ThreadId, operation::now_unix_ms};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, future::Future, pin::Pin};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ForgeProviderKind {
    GitLab,
    GitHub,
}

impl ForgeProviderKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::GitLab => "gitlab",
            Self::GitHub => "github",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeIdentity {
    pub provider: ForgeProviderKind,
    pub host: String,
    pub project_id: String,
    pub path_with_namespace: String,
    pub web_url: String,
    pub default_branch: Option<String>,
}

impl ForgeIdentity {
    pub fn issue_source_ref(&self, iid: u64) -> String {
        match self.provider {
            ForgeProviderKind::GitLab => format!(
                "gitlab://{}/projects/{}/issues/{iid}",
                self.host, self.project_id
            ),
            ForgeProviderKind::GitHub => format!(
                "github://{}/repositories/{}/issues/{iid}",
                self.host, self.project_id
            ),
        }
    }

    pub fn change_request_source_ref(&self, iid: u64) -> String {
        match self.provider {
            ForgeProviderKind::GitLab => format!(
                "gitlab://{}/projects/{}/merge-requests/{iid}",
                self.host, self.project_id
            ),
            ForgeProviderKind::GitHub => format!(
                "github://{}/repositories/{}/pull-requests/{iid}",
                self.host, self.project_id
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ForgeCapability {
    Issues,
    IssueBoards,
    MergeRequests,
    Pipelines,
    ApprovalSummary,
    Discussions,
    WorkItems,
}

impl ForgeCapability {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Issues => "issues",
            Self::IssueBoards => "issue-boards",
            Self::MergeRequests => "merge-requests",
            Self::Pipelines => "pipelines",
            Self::ApprovalSummary => "approval-summary",
            Self::Discussions => "discussions",
            Self::WorkItems => "work-items",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityState {
    Available,
    Unavailable,
    Unknown,
}

impl CapabilityState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForgeFreshness {
    Fresh,
    Aging,
    Stale,
    Unavailable,
}

impl ForgeFreshness {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Aging => "aging",
            Self::Stale => "stale",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeIssueSummary {
    pub iid: u64,
    pub title: String,
    pub state: String,
    pub web_url: String,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeRequestSummary {
    pub iid: u64,
    pub title: String,
    pub state: String,
    pub source_branch: String,
    pub target_branch: String,
    pub web_url: String,
    pub updated_at: Option<String>,
    pub draft: bool,
    pub detailed_merge_status: Option<String>,
    pub blocking_discussions_resolved: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipelineSummary {
    pub id: u64,
    pub status: String,
    pub reference: String,
    pub web_url: String,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueBoardSummary {
    pub id: u64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeReviewSummary {
    pub thread_id: ThreadId,
    pub cwd: String,
    pub change_request_iid: u64,
    pub approvals_required: Option<u64>,
    pub approvals_left: Option<u64>,
    pub approved_by_count: usize,
    pub changes_requested_by_count: usize,
    pub discussions_total: usize,
    pub unresolved_discussions: usize,
    pub approvals_available: bool,
    pub discussions_available: bool,
    pub observed_at_unix_ms: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeObservation {
    pub thread_id: ThreadId,
    pub cwd: String,
    pub remote_name: Option<String>,
    pub remote_url: Option<String>,
    pub identity: Option<ForgeIdentity>,
    pub capabilities: BTreeMap<ForgeCapability, CapabilityState>,
    pub issues: Vec<ForgeIssueSummary>,
    pub change_requests: Vec<ChangeRequestSummary>,
    pub pipelines: Vec<PipelineSummary>,
    pub review: Option<ForgeReviewSummary>,
    pub observed_at_unix_ms: u64,
    pub freshness: ForgeFreshness,
    pub error: Option<String>,
}

impl ForgeObservation {
    pub fn pending(thread_id: ThreadId, cwd: String) -> Self {
        Self {
            thread_id,
            cwd,
            remote_name: None,
            remote_url: None,
            identity: None,
            capabilities: default_capabilities(),
            issues: vec![],
            change_requests: vec![],
            pipelines: vec![],
            review: None,
            observed_at_unix_ms: 0,
            freshness: ForgeFreshness::Unavailable,
            error: None,
        }
    }

    pub fn unavailable(thread_id: ThreadId, cwd: String, error: impl Into<String>) -> Self {
        Self {
            thread_id,
            cwd,
            remote_name: None,
            remote_url: None,
            identity: None,
            capabilities: default_capabilities(),
            issues: vec![],
            change_requests: vec![],
            pipelines: vec![],
            review: None,
            observed_at_unix_ms: now_unix_ms(),
            freshness: ForgeFreshness::Unavailable,
            error: Some(error.into()),
        }
    }

    pub fn change_request_for_branch(&self, branch: &str) -> Option<&ChangeRequestSummary> {
        self.change_requests
            .iter()
            .find(|change| change.source_branch == branch)
    }

    pub fn pipeline_for_branch(&self, branch: &str) -> Option<&PipelineSummary> {
        self.pipelines
            .iter()
            .find(|pipeline| pipeline.reference == branch)
    }

    /// A timely Forge snapshot may still lack one or more core data capabilities.
    pub fn core_data_incomplete(&self) -> bool {
        [
            ForgeCapability::Issues,
            ForgeCapability::MergeRequests,
            ForgeCapability::Pipelines,
        ]
        .iter()
        .any(|kind| self.capabilities.get(kind) != Some(&CapabilityState::Available))
    }

    pub fn freshness_at(&self, now_unix_ms: u64) -> ForgeFreshness {
        if self.observed_at_unix_ms == 0 || self.error.is_some() || self.identity.is_none() {
            return ForgeFreshness::Unavailable;
        }
        let age = now_unix_ms.saturating_sub(self.observed_at_unix_ms);
        if age <= crate::planning::FRESH_AGE_MS {
            ForgeFreshness::Fresh
        } else if age <= crate::planning::AGING_AGE_MS {
            ForgeFreshness::Aging
        } else {
            ForgeFreshness::Stale
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteIdentity {
    pub remote_name: String,
    pub remote_url: String,
    pub host: String,
    pub path_with_namespace: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeDoctorSnapshot {
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    pub authenticated: Option<bool>,
    pub server_version: Option<String>,
    pub server_edition: Option<String>,
    pub server_tier: Option<String>,
    pub remote: Option<RemoteIdentity>,
    pub observation: ForgeObservation,
    pub boards: Vec<IssueBoardSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeReviewTarget {
    pub thread_id: ThreadId,
    pub cwd: String,
    pub provider: ForgeProviderKind,
    pub host: String,
    pub project_id: String,
    pub project_path: String,
    pub change_request_iid: u64,
}

/// In-memory result retaining the exact target that the actor actually requested.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgeReviewResult {
    pub target: ForgeReviewTarget,
    pub summary: ForgeReviewSummary,
}
impl ForgeReviewResult {
    pub fn unavailable(target: ForgeReviewTarget, error: String) -> Self {
        let summary = ForgeReviewSummary {
            thread_id: target.thread_id.clone(),
            cwd: target.cwd.clone(),
            change_request_iid: target.change_request_iid,
            approvals_required: None,
            approvals_left: None,
            approved_by_count: 0,
            changes_requested_by_count: 0,
            discussions_total: 0,
            unresolved_discussions: 0,
            approvals_available: false,
            discussions_available: false,
            observed_at_unix_ms: now_unix_ms(),
            error: Some(error),
        };
        Self { target, summary }
    }
}

pub type ForgeFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait ForgeProvider: Send + Sync {
    fn probe<'a>(&'a self, thread_id: ThreadId, cwd: String) -> ForgeFuture<'a, ForgeObservation>;

    fn probe_review<'a>(&'a self, target: ForgeReviewTarget)
    -> ForgeFuture<'a, ForgeReviewSummary>;
}

pub(crate) fn default_capabilities() -> BTreeMap<ForgeCapability, CapabilityState> {
    [
        (ForgeCapability::Issues, CapabilityState::Unknown),
        (ForgeCapability::IssueBoards, CapabilityState::Unknown),
        (ForgeCapability::MergeRequests, CapabilityState::Unknown),
        (ForgeCapability::Pipelines, CapabilityState::Unknown),
        (ForgeCapability::ApprovalSummary, CapabilityState::Unknown),
        (ForgeCapability::Discussions, CapabilityState::Unknown),
        (ForgeCapability::WorkItems, CapabilityState::Unknown),
    ]
    .into_iter()
    .collect()
}
