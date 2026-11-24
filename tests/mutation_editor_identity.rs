//! Editing intent must not follow asynchronous target changes.
use codex_tui::{
    app::{Action, AppState, ContextChoice, Effect, InputMode, View, reduce},
    backend::{CodexBackend, FakeBackend},
    domain::LocalRepoIdentity,
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
                head_sha: None,
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
#[test]
fn goal_refresh_reads_only_the_current_thread_and_never_recreates_a_cleared_goal() {
    let mut app = fixture(false);
    let first = app.threads[0].id.clone();
    let second = app.threads[1].id.clone();
    app.view = View::Thread(first.clone());
    reduce(
        &mut app,
        Action::GoalObserved(GoalObservation {
            thread_id: first.clone(),
            objective: "native objective".into(),
            status: GoalStatus::Paused,
            token_budget: None,
            tokens_used: 1,
            time_used_seconds: 1,
            created_at: 1,
            updated_at: 1,
            observed_at_unix_ms: 1,
        }),
    );
    assert_eq!(
        reduce(&mut app, Action::RefreshGoal),
        vec![Effect::RefreshGoal(first.clone())]
    );
    let observed = app.goals.get(&first.0).unwrap().clone();
    app.view = View::Thread(second.clone());
    assert_eq!(
        reduce(&mut app, Action::RefreshGoal),
        vec![Effect::RefreshGoal(second)]
    );
    assert_eq!(app.goals.get(&first.0), Some(&observed));
    reduce(&mut app, Action::GoalCleared(first.clone()));
    app.view = View::Thread(first.clone());
    assert_eq!(
        reduce(&mut app, Action::RefreshGoal),
        vec![Effect::RefreshGoal(first.clone())]
    );
    assert!(!app.goals.contains_key(&first.0));
}

#[test]
fn goal_refresh_refuses_a_stale_thread_identity() {
    let mut app = fixture(false);
    let removed = app.threads[0].id.clone();
    let remaining = app.threads[1..].to_vec();
    reduce(&mut app, Action::ReplaceThreads(remaining));
    app.view = View::Thread(removed);
    assert!(reduce(&mut app, Action::RefreshGoal).is_empty());
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
fn context_menu_cannot_retarget_after_the_saved_view_changes() {
    let mut app = fixture(false);
    reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 10 });
    reduce(&mut app, Action::OpenBoard);
    for stage in 0..codex_tui::planning::WorkflowStage::ALL.len() {
        app.board_stage_index = stage;
        if !app.visible_planning_cards().is_empty() {
            break;
        }
    }
    reduce(&mut app, Action::OpenContext);
    app.context_selected = app
        .context_choices()
        .iter()
        .position(|c| *c == ContextChoice::Snooze)
        .unwrap();
    app.planning_view_index += 1;
    assert!(reduce(&mut app, Action::ExecuteContext).is_empty());
    assert_eq!(app.input_mode, InputMode::Normal);
    assert!(app.snooze_target.is_none());
    assert!(app.mutation_notice.is_some());
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
