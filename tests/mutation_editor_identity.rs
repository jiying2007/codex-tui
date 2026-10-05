//! Editing intent must not follow asynchronous target changes.
use codex_tui::{
    app::{Action, AppState, ContextChoice, Effect, InputMode, View, reduce},
    backend::{CodexBackend, FakeBackend},
    domain::{LocalRepoIdentity, ThreadId},
    forge::{ChangeRequestSummary, ForgeIdentity, ForgeObservation, ForgeProviderKind},
    git::GitContext,
    goal::{GoalObservation, GoalStatus},
    planning::SourceRef,
};
fn fixture(with_change: bool) -> AppState {
    let mut threads = FakeBackend::scaled(2).snapshot().threads;
    for (i, t) in threads.iter_mut().enumerate() {
        t.metadata.cwd = format!("/repo-{i}");
    }
    let mut app = AppState::new(threads);
    for (i, t) in app.threads.clone().iter().enumerate() {
        let mut git = GitContext::pending(t.id.clone(), t.metadata.cwd.clone());
        git.is_repository = true;
        git.repo = Some(LocalRepoIdentity {
            git_common_dir: format!("/repo-{i}/.git"),
            primary_root: t.metadata.cwd.clone(),
        });
        git.branch = Some("feature".into());
        git.observed_at_unix_ms = 1;
        app.git_contexts.insert(t.id.0.clone(), git);
        let mut forge = ForgeObservation::pending(t.id.clone(), t.metadata.cwd.clone());
        forge.identity = Some(ForgeIdentity {
            provider: ForgeProviderKind::GitLab,
            host: "gitlab.example.com".into(),
            project_id: format!("{}", 42 + i),
            path_with_namespace: format!("team/repo-{i}"),
            web_url: format!("https://gitlab.example.com/team/repo-{i}"),
            default_branch: Some("main".into()),
        });
        forge.observed_at_unix_ms = 1;
        if with_change {
            forge.change_requests.push(ChangeRequestSummary {
                iid: 7,
                title: "change".into(),
                state: "opened".into(),
                source_branch: "feature".into(),
                target_branch: "main".into(),
                web_url: "https://gitlab.example.com/team/repo/-/merge_requests/7".into(),
                updated_at: None,
                draft: false,
                detailed_merge_status: None,
                blocking_discussions_resolved: None,
            });
        }
        app.forge_observations.insert(t.id.0.clone(), forge);
    }
    app.view = View::Workspace(app.threads[0].id.clone());
    app
}
fn open_forge(app: &mut AppState, choice: ContextChoice) {
    reduce(app, Action::OpenContext);
    app.context_selected = app
        .context_choices()
        .iter()
        .position(|c| *c == choice)
        .unwrap();
    assert!(reduce(app, Action::ExecuteContext).is_empty());
    assert!(matches!(
        app.input_mode,
        InputMode::ForgeComment | InputMode::ForgeMergeRequestTitle
    ));
    app.input_buffer = "draft that must not change targets".into();
}
fn rejected(app: &mut AppState) {
    let mode = app.input_mode;
    let draft = app.input_buffer.clone();
    assert!(
        reduce(app, Action::CommitInput).is_empty(),
        "unexpected write effect"
    );
    assert!(
        app.pending_forge_operation.is_none(),
        "wrong Forge plan created"
    );
    assert!(app.pending_operation.is_none(), "wrong Git plan created");
    assert_eq!(app.input_mode, mode);
    assert_eq!(app.input_buffer, draft);
    assert!(app.mutation_notice.is_some(), "conflict must be visible");
}
#[test]
fn forge_comment_cannot_follow_another_thread() {
    let mut app = fixture(true);
    open_forge(&mut app, ContextChoice::ForgeComment);
    app.view = View::Workspace(app.threads[1].id.clone());
    rejected(&mut app);
}
#[test]
fn forge_editor_cannot_use_removed_thread_with_cached_context() {
    let mut app = fixture(true);
    open_forge(&mut app, ContextChoice::ForgeComment);
    reduce(&mut app, Action::ReplaceThreads(vec![]));
    rejected(&mut app);
}
#[test]
fn forge_comment_cannot_follow_replaced_change_request() {
    let mut app = fixture(true);
    open_forge(&mut app, ContextChoice::ForgeComment);
    let id = app.threads[0].id.0.clone();
    app.forge_observations.get_mut(&id).unwrap().change_requests[0].iid = 8;
    rejected(&mut app);
}
#[test]
fn forge_create_cannot_follow_repository_or_default_branch_changes() {
    for drift in 0..5 {
        let mut app = fixture(false);
        open_forge(&mut app, ContextChoice::ForgeCreateMergeRequest);
        let id = app.threads[0].id.0.clone();
        let identity = app
            .forge_observations
            .get_mut(&id)
            .unwrap()
            .identity
            .as_mut()
            .unwrap();
        match drift {
            0 => identity.provider = ForgeProviderKind::GitHub,
            1 => identity.host = "other.example.com".into(),
            2 => identity.project_id = "999".into(),
            3 => identity.path_with_namespace = "team/other".into(),
            _ => identity.default_branch = Some("release".into()),
        };
        rejected(&mut app);
    }
}
#[test]
fn forge_editor_cannot_use_a_former_cwd() {
    let mut app = fixture(true);
    open_forge(&mut app, ContextChoice::ForgeComment);
    let mut threads = app.threads.clone();
    threads[0].metadata.cwd = "/changed".into();
    reduce(&mut app, Action::ReplaceThreads(threads));
    rejected(&mut app);
}
#[test]
fn same_forge_target_metadata_refresh_still_requires_confirmation() {
    let mut app = fixture(true);
    open_forge(&mut app, ContextChoice::ForgeComment);
    let id = app.threads[0].id.0.clone();
    let obs = app.forge_observations.get_mut(&id).unwrap();
    obs.observed_at_unix_ms += 100;
    obs.change_requests[0].title = "renamed title".into();
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(
        app.pending_forge_operation
            .as_ref()
            .unwrap()
            .change_request_iid,
        Some(7)
    );
    assert!(matches!(
        reduce(&mut app, Action::ConfirmPendingOperation).as_slice(),
        [Effect::ExecuteForgeOperation(_)]
    ));
}
#[test]
fn worktree_create_does_not_switch_repository_mid_wizard() {
    let mut app = fixture(false);
    reduce(&mut app, Action::BeginCreateWorktree);
    app.input_buffer = "feature/new".into();
    reduce(&mut app, Action::CommitInput);
    let root = tempfile::tempdir().unwrap();
    app.input_buffer = root.path().join("new-worktree").to_string_lossy().into();
    reduce(&mut app, Action::CommitInput);
    assert_eq!(app.input_mode, InputMode::WorktreeCreateStartPoint);
    app.view = View::Workspace(app.threads[1].id.clone());
    rejected(&mut app);
    assert_eq!(app.create_worktree_branch.as_deref(), Some("feature/new"));
    assert!(app.create_worktree_path.is_some());
}
#[test]
fn delete_branch_does_not_switch_repository_during_input() {
    let mut app = fixture(false);
    reduce(&mut app, Action::BeginDeleteBranch);
    app.input_buffer = "feature/delete".into();
    let id = app.threads[0].id.0.clone();
    app.git_contexts
        .get_mut(&id)
        .unwrap()
        .repo
        .as_mut()
        .unwrap()
        .git_common_dir = "/replacement/.git".into();
    rejected(&mut app);
}
#[test]
fn worktree_editor_refuses_removed_thread() {
    let mut app = fixture(false);
    reduce(&mut app, Action::BeginDeleteBranch);
    reduce(&mut app, Action::ReplaceThreads(vec![]));
    rejected(&mut app);
}
#[test]
fn unchanged_branch_delete_requires_confirmation() {
    let mut app = fixture(false);
    reduce(&mut app, Action::BeginDeleteBranch);
    app.input_buffer = "feature/delete".into();
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(
        app.pending_operation.as_ref().unwrap().repo.primary_root,
        "/repo-0"
    );
    assert!(matches!(
        reduce(&mut app, Action::ConfirmPendingOperation).as_slice(),
        [Effect::ExecuteOperation(_)]
    ));
}
fn open_goal(app: &mut AppState) -> ThreadId {
    let id = app.threads[0].id.clone();
    app.view = View::Thread(id.clone());
    reduce(
        app,
        Action::GoalObserved(GoalObservation {
            thread_id: id.clone(),
            objective: "original".into(),
            status: GoalStatus::Paused,
            token_budget: None,
            tokens_used: 1,
            time_used_seconds: 1,
            created_at: 1,
            updated_at: 1,
            observed_at_unix_ms: 1,
        }),
    );
    reduce(app, Action::BeginGoalObjective);
    assert_eq!(app.input_mode, InputMode::GoalObjective);
    app.input_buffer = "edited objective".into();
    id
}
#[test]
fn goal_editor_cannot_follow_another_thread() {
    let mut app = fixture(false);
    open_goal(&mut app);
    app.view = View::Thread(app.threads[1].id.clone());
    rejected(&mut app);
}
#[test]
fn goal_editor_refuses_changed_objective_or_replaced_goal() {
    for replacement in [false, true] {
        let mut app = fixture(false);
        let id = open_goal(&mut app);
        let goal = app.goals.get_mut(&id.0).unwrap();
        if replacement {
            goal.created_at = 2;
        } else {
            goal.objective = "external edit".into();
        }
        rejected(&mut app);
    }
}
#[test]
fn goal_editor_does_not_recreate_cleared_goal() {
    let mut app = fixture(false);
    let id = open_goal(&mut app);
    reduce(&mut app, Action::GoalCleared(id));
    rejected(&mut app);
}
#[test]
fn goal_progress_does_not_invalidate_objective_edit() {
    let mut app = fixture(false);
    let id = open_goal(&mut app);
    let goal = app.goals.get_mut(&id.0).unwrap();
    goal.tokens_used += 10;
    goal.time_used_seconds += 10;
    goal.observed_at_unix_ms += 10;
    assert_eq!(
        reduce(&mut app, Action::CommitInput),
        vec![Effect::SetGoal {
            thread_id: id,
            objective: Some("edited objective".into()),
            status: None
        }]
    );
}
#[test]
fn hot_slot_binding_keeps_original_target_during_navigation() {
    let mut app = fixture(false);
    app.view = View::Registry;
    let original = SourceRef::codex_thread(&app.threads[0].id);
    reduce(&mut app, Action::BeginHotSlotBind);
    app.selected = 1;
    assert_eq!(
        reduce(&mut app, Action::UseHotSlot(3)),
        vec![Effect::SetHotSlot {
            slot: 3,
            target: original
        }]
    );
}
#[test]
fn hot_slot_binding_refuses_removed_target_instead_of_neighbour() {
    let mut app = fixture(false);
    app.view = View::Registry;
    reduce(&mut app, Action::BeginHotSlotBind);
    let remaining = app.threads[1..].to_vec();
    reduce(&mut app, Action::ReplaceThreads(remaining));
    assert!(reduce(&mut app, Action::UseHotSlot(3)).is_empty());
    assert!(app.mutation_notice.is_some());
}
#[test]
fn cancelled_editor_does_not_reuse_old_scope() {
    let mut app = fixture(true);
    open_forge(&mut app, ContextChoice::ForgeComment);
    reduce(&mut app, Action::CancelInput);
    app.view = View::Workspace(app.threads[1].id.clone());
    open_forge(&mut app, ContextChoice::ForgeComment);
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert_eq!(
        app.pending_forge_operation.as_ref().unwrap().project_id,
        "43"
    );
}

#[test]
fn context_menu_does_not_change_highlighted_action_when_forge_options_change() {
    let mut app = fixture(false);
    reduce(&mut app, Action::OpenContext);
    app.context_selected = app
        .context_choices()
        .iter()
        .position(|c| *c == ContextChoice::ForgeCreateMergeRequest)
        .unwrap();
    let populated = fixture(true);
    let id = app.threads[0].id.0.clone();
    app.forge_observations.get_mut(&id).unwrap().change_requests =
        populated.forge_observations[&id].change_requests.clone();
    assert_eq!(
        app.context_choice(),
        Some(ContextChoice::ForgeCreateMergeRequest)
    );
    assert!(reduce(&mut app, Action::ExecuteContext).is_empty());
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.pending_forge_operation.is_none());
    assert!(app.mutation_notice.is_some());
}
#[test]
fn context_menu_refuses_a_removed_target_instead_of_snoozing_its_neighbour() {
    let mut app = fixture(false);
    app.view = View::Registry;
    reduce(&mut app, Action::OpenContext);
    app.context_selected = app
        .context_choices()
        .iter()
        .position(|c| *c == ContextChoice::Snooze)
        .unwrap();
    let remaining = app.threads[1..].to_vec();
    reduce(&mut app, Action::ReplaceThreads(remaining));
    assert!(reduce(&mut app, Action::ExecuteContext).is_empty());
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.snooze_target.is_none());
    assert!(app.mutation_notice.is_some());
}
#[test]
fn context_menu_approve_cannot_follow_another_project_with_the_same_iid() {
    let mut app = fixture(true);
    reduce(&mut app, Action::OpenContext);
    app.context_selected = app
        .context_choices()
        .iter()
        .position(|c| *c == ContextChoice::ForgeApprove)
        .unwrap();
    let id = app.threads[0].id.0.clone();
    app.forge_observations
        .get_mut(&id)
        .unwrap()
        .identity
        .as_mut()
        .unwrap()
        .project_id = "999".into();
    assert!(reduce(&mut app, Action::ExecuteContext).is_empty());
    assert!(app.pending_forge_operation.is_none());
    assert!(app.mutation_notice.is_some());
}
#[test]
fn context_menu_launch_does_not_follow_replaced_repository() {
    let mut app = fixture(true);
    reduce(&mut app, Action::OpenContext);
    app.context_selected = app
        .context_choices()
        .iter()
        .position(|c| *c == ContextChoice::LaunchPreset)
        .unwrap();
    let id = app.threads[0].id.0.clone();
    app.git_contexts
        .get_mut(&id)
        .unwrap()
        .repo
        .as_mut()
        .unwrap()
        .primary_root = "/other-repo".into();
    assert!(reduce(&mut app, Action::ExecuteContext).is_empty());
    assert!(app.mutation_notice.is_some());
}
#[test]
fn context_menu_cannot_edit_a_different_saved_view() {
    let mut app = fixture(false);
    app.view = View::Board;
    reduce(&mut app, Action::OpenContext);
    app.context_selected = app
        .context_choices()
        .iter()
        .position(|c| *c == ContextChoice::SaveCurrentView)
        .unwrap();
    app.planning_view_index += 1;
    assert!(reduce(&mut app, Action::ExecuteContext).is_empty());
    assert!(app.saved_view_editor.is_none());
    assert!(app.mutation_notice.is_some());
}
#[test]
fn context_menu_same_target_refresh_and_cancel_remain_usable() {
    let mut app = fixture(true);
    reduce(&mut app, Action::OpenContext);
    reduce(&mut app, Action::CloseContext);
    open_forge(&mut app, ContextChoice::ForgeComment);
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
    assert!(app.pending_forge_operation.is_some());
}

#[test]
fn context_menu_rejects_a_removed_thread_even_when_the_view_keeps_its_id() {
    let mut app = fixture(false);
    reduce(&mut app, Action::OpenContext);
    app.context_selected = app
        .context_choices()
        .iter()
        .position(|c| *c == ContextChoice::Snooze)
        .unwrap();
    reduce(&mut app, Action::ReplaceThreads(vec![]));
    assert!(reduce(&mut app, Action::ExecuteContext).is_empty());
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.snooze_target.is_none());
    assert!(app.mutation_notice.is_some());
}
