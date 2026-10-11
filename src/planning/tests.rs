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

#[test]
fn registry_pin_projects_into_work_card_queries_and_sorting() {
    let mut thread = first_thread();
    thread.pinned = true;
    let card = reconcile_thread_card(ReconcileInput {
        thread: &thread,
        git: None,
        local: None,
        collision_count: 0,
        backend_observed_at_unix_ms: Some(100),
        backend_error: None,
        now_unix_ms: 100,
    });
    assert!(card.overlay.pinned);

    let view = SavedView {
        id: "pinned".into(),
        name: "pinned".into(),
        source_scope: "all".into(),
        filter: "pinned:true".into(),
        group_by: None,
        order_by: None,
        layout: SavedViewLayout::List,
        visible_fields: vec![],
    };
    assert_eq!(apply_saved_view(&[card], &view).len(), 1);
}

#[test]
fn saved_view_query_supports_quoted_terms_negation_and_project_alias() {
    let mut thread = first_thread();
    thread.workspace = "audio pipeline".into();
    thread.title = "Investigate noisy call".into();
    thread.runtime = RuntimeStatus::Ready;
    let mut local = WorkCardRecord::implicit_thread(&thread.id);
    local.overlay.tags.insert("research".into());
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
        id: "quoted".into(),
        name: "quoted".into(),
        source_scope: "all".into(),
        filter: "project:\"audio pipeline\" tag:research -stage:done".into(),
        group_by: None,
        order_by: None,
        layout: SavedViewLayout::List,
        visible_fields: vec![],
    };
    assert_eq!(apply_saved_view(&[card], &view).len(), 1);
}

#[test]
fn saved_view_query_uses_observed_branch_forge_and_change_request_state() {
    use crate::forge::{
        CapabilityState, ChangeRequestSummary, ForgeCapability, ForgeIdentity, ForgeProviderKind,
    };
    use std::collections::BTreeMap;

    let mut thread = first_thread();
    thread.runtime = RuntimeStatus::Ready;
    let mut git = GitContext::pending(thread.id.clone(), "/repo");
    git.is_repository = true;
    git.branch = Some("feature/search".into());
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
            default_branch: Some("main".into()),
        }),
        capabilities: BTreeMap::from([(
            ForgeCapability::MergeRequests,
            CapabilityState::Available,
        )]),
        issues: vec![],
        change_requests: vec![ChangeRequestSummary {
            iid: 8,
            title: "Search".into(),
            state: "opened".into(),
            source_branch: "feature/search".into(),
            target_branch: "main".into(),
            web_url: "https://gitlab.example.com/team/repo/-/merge_requests/8".into(),
            updated_at: None,
            draft: false,
            detailed_merge_status: None,
            blocking_discussions_resolved: None,
        }],
        pipelines: vec![],
        overview_source_page_saturated: false,
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
    let view = SavedView {
        id: "observed".into(),
        name: "observed".into(),
        source_scope: "all".into(),
        filter: "branch:feature/search forge:gitlab mr:open -attention:pipeline-failed".into(),
        group_by: None,
        order_by: None,
        layout: SavedViewLayout::List,
        visible_fields: vec![],
    };
    assert_eq!(apply_saved_view(&[card], &view).len(), 1);
}

#[test]
fn invalid_structured_query_fails_closed() {
    let card = reconcile_thread_card(ReconcileInput {
        thread: &first_thread(),
        git: None,
        local: None,
        collision_count: 0,
        backend_observed_at_unix_ms: Some(100),
        backend_error: None,
        now_unix_ms: 100,
    });
    for filter in [
        "unknown:value",
        "pinned:maybe",
        "stage:banana",
        "\"unterminated",
    ] {
        let view = SavedView {
            id: "invalid".into(),
            name: "invalid".into(),
            source_scope: "all".into(),
            filter: filter.into(),
            group_by: None,
            order_by: None,
            layout: SavedViewLayout::List,
            visible_fields: vec![],
        };
        assert!(apply_saved_view(std::slice::from_ref(&card), &view).is_empty());
    }
}

#[test]
fn saved_view_source_scope_is_enforced() {
    let card = reconcile_thread_card(ReconcileInput {
        thread: &first_thread(),
        git: None,
        local: None,
        collision_count: 0,
        backend_observed_at_unix_ms: Some(100),
        backend_error: None,
        now_unix_ms: 100,
    });
    let view = SavedView {
        id: "scope".into(),
        name: "scope".into(),
        source_scope: "scratch".into(),
        filter: String::new(),
        group_by: None,
        order_by: None,
        layout: SavedViewLayout::List,
        visible_fields: vec![],
    };
    assert!(apply_saved_view(&[card], &view).is_empty());
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
fn projected_goal_and_worktree_links_are_stable_and_deduplicated() {
    let thread = first_thread();
    let goal = GoalObservation {
        thread_id: thread.id.clone(),
        objective: "Ship v1.4".into(),
        status: GoalStatus::Active,
        token_budget: Some(10_000),
        tokens_used: 100,
        time_used_seconds: 10,
        created_at: 1,
        updated_at: 2,
        observed_at_unix_ms: 100,
    };
    let repo = crate::domain::LocalRepoIdentity {
        git_common_dir: "/repo/.git".into(),
        primary_root: "/repo".into(),
    };
    let mut git = GitContext::pending(thread.id.clone(), "/repo");
    git.is_repository = true;
    git.worktree = Some(crate::domain::WorktreeIdentity {
        repo,
        canonical_path: "/repo".into(),
        branch: Some("feature".into()),
        managed_by_codex_tui: true,
    });

    let card = reconcile_thread_card_with_goal(
        ReconcileInput {
            thread: &thread,
            git: Some(&git),
            local: None,
            collision_count: 0,
            backend_observed_at_unix_ms: Some(100),
            backend_error: None,
            now_unix_ms: 100,
        },
        Some(&goal),
    );

    assert!(card.links.iter().any(|link| {
        link.role == LinkRole::Goal
            && link.source.kind == SourceKind::Goal
            && link.source.value == thread.id.0
    }));
    assert!(card.links.iter().any(|link| {
        link.role == LinkRole::Worktree
            && link.source.kind == SourceKind::Worktree
            && link.source.value == "/repo"
    }));
    assert!(card_matches_filter(&card, "link:goal"));
    assert!(card_matches_filter(&card, "worktree:/repo"));
    assert!(card_matches_filter(&card, "repo"));

    let mut links = card.links.clone();
    push_link_if_missing(
        &mut links,
        LinkRole::Goal,
        SourceRef {
            kind: SourceKind::Goal,
            value: thread.id.0.clone(),
        },
    );
    assert_eq!(links.len(), card.links.len());
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
            default_branch: Some("main".into()),
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
        overview_source_page_saturated: false,
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
        changes_requested_by_count: 0,
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
            default_branch: Some("main".into()),
        }),
        capabilities: BTreeMap::from([(ForgeCapability::Issues, CapabilityState::Available)]),
        issues: vec![],
        change_requests: vec![],
        pipelines: vec![],
        overview_source_page_saturated: false,
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
