use super::*;
use crate::app::AppState;
use crate::backend::{CodexBackend, FakeBackend};
use crate::keymap::HELP_BINDINGS;
use pretty_assertions::assert_eq;
use ratatui::{Terminal, backend::TestBackend};
use std::collections::BTreeSet;

fn render_snapshot(width: u16) -> String {
    let backend = TestBackend::new(width, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let app = AppState::new(FakeBackend::seeded().snapshot().threads);
    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

#[test]
fn english_help_tokens_exactly_match_the_executable_key_contract() {
    let surfaces = HELP_BINDINGS
        .iter()
        .map(|binding| binding.surface)
        .collect::<BTreeSet<_>>();
    for surface in surfaces.iter() {
        let expected = HELP_BINDINGS
            .iter()
            .filter(|binding| binding.surface == *surface)
            .map(|binding| binding.token)
            .collect::<BTreeSet<_>>();
        let prefix = format!("{surface}:");
        let advertised = HELP_LINES
            .iter()
            .filter(|line| line.starts_with(&prefix))
            .flat_map(|line| {
                line.split_once(':')
                    .map(|(_, tail)| tail)
                    .unwrap_or_default()
                    .split('·')
            })
            .filter_map(|segment| segment.split_whitespace().next())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            advertised, expected,
            "Help/keymap contract drift on {surface}"
        );
    }
}

#[test]
fn registry_history_label_distinguishes_partial_and_complete_history() {
    assert_eq!(
        registry_history_label(UiLanguage::English, true, false, false),
        "RECENT · HYDRATING"
    );
    assert_eq!(
        registry_history_label(UiLanguage::English, true, true, false),
        "SEARCH PARTIAL · HYDRATING"
    );
    assert_eq!(
        registry_history_label(UiLanguage::English, false, true, false),
        "SEARCH ALL"
    );
    assert_eq!(
        registry_history_label(UiLanguage::SimplifiedChinese, true, false, true),
        "全部历史（加载中）"
    );
    assert_eq!(
        registry_history_label(UiLanguage::SimplifiedChinese, false, false, true),
        "全部历史"
    );
    assert_eq!(
        registry_history_label(UiLanguage::English, false, false, false),
        "RECENT"
    );
}

#[test]
fn production_ui_never_performs_authoritative_cwd_filesystem_classification() {
    let source = include_str!("../ui.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production ui source");
    assert!(
        !production.contains(".cwd_locality("),
        "rendering must not call the filesystem-backed AppState cwd locality API"
    );
    assert!(
        !production.contains("classify_cwd("),
        "rendering must not call the filesystem-backed cwd classifier"
    );
}

#[test]
fn simplified_chinese_ui_localizes_daily_chrome_and_help() {
    let backend = TestBackend::new(160, 28);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.language = UiLanguage::SimplifiedChinese;
    app.backend_status.connected = true;
    app.backend_status.registry_complete = true;
    app.backend_status.source = "codex-app-server".into();
    app.backend_status.platform = None;
    app.threads[0].metadata.model = None;

    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let mut snapshot = String::new();
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }
    for glyph in [
        '任', '务', '中', '心', '已', '选', '择', '搜', '索', '未', '知',
    ] {
        assert!(
            snapshot.contains(glyph),
            "missing localized glyph {glyph:?}"
        );
    }

    app.show_help = true;
    terminal
        .draw(|frame| render(frame, &app))
        .expect("draw help");
    let mut help_snapshot = String::new();
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            help_snapshot.push_str(buffer[(x, y)].symbol());
        }
        help_snapshot.push('\n');
    }
    for glyph in ['帮', '助', '权', '限', '边', '界'] {
        assert!(
            help_snapshot.contains(glyph),
            "missing localized help glyph {glyph:?}"
        );
    }
}

#[test]
fn registry_viewport_keeps_selected_row_visible() {
    let mut app = AppState::new(FakeBackend::scaled(100).snapshot().threads);
    app.selected = 99;

    let (_visible, viewport) = registry_viewport(&app, 10);
    assert_eq!(
        viewport,
        RegistryViewport {
            start: 93,
            end: 100,
            total: 100,
            matched: 100,
            row_capacity: 7,
        }
    );
}

#[test]
fn registry_renders_scrollbar_and_last_selected_row() {
    let backend = TestBackend::new(100, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = AppState::new(FakeBackend::scaled(100).snapshot().threads);
    app.selected = 99;

    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut snapshot = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }

    assert!(snapshot.contains("Synthetic work item 00099"));
    assert!(!snapshot.contains("Synthetic work item 00000"));
    assert!(
        snapshot.contains('█'),
        "overflowing registry must render a scrollbar thumb"
    );
}

#[test]
fn registry_viewport_uses_filtered_thread_count() {
    let mut app = AppState::new(FakeBackend::scaled(100).snapshot().threads);
    app.filter = "repo-001".into();
    let visible = app.visible_indices();
    app.selected = *visible.last().expect("filtered row");

    let (viewport_visible, viewport) = registry_viewport(&app, 10);
    assert_eq!(viewport.total, viewport_visible.len());
    assert_eq!(viewport.end, viewport.total);
    assert!(viewport.total < 100);
}

#[test]
fn ordinary_registry_search_renders_unprobed_native_cwds_without_locality_cache() {
    let backend = TestBackend::new(140, 14);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = AppState::new(FakeBackend::scaled(150).snapshot().threads);
    app.backend_status = FakeBackend::seeded().snapshot().status;
    let cwd = std::env::current_dir()
        .expect("cwd")
        .to_string_lossy()
        .into_owned();
    for thread in &mut app.threads {
        thread.metadata.cwd.clone_from(&cwd);
    }
    app.filter = "Synthetic".into();

    assert_eq!(app.cwd_locality_for_display(&cwd), None);
    terminal.draw(|frame| render(frame, &app)).expect("draw");

    let buffer = terminal.backend().buffer();
    let mut snapshot = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }

    assert!(snapshot.contains("150 unprobed"));
    assert_eq!(app.cwd_locality_for_display(&cwd), None);
}

#[cfg(not(windows))]
#[test]
fn registry_marks_foreign_windows_cwd_without_linux_prefix() {
    let backend = TestBackend::new(160, 28);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.threads[0].metadata.cwd = r"/vsdata/repo/C:\Users\jun\repo".into();

    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut snapshot = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }

    assert!(snapshot.contains("foreign-windows"));
    assert!(snapshot.contains(r"C:\Users\jun\repo"));
    assert!(!snapshot.contains(r"/vsdata/repo/C:\Users\jun\repo"));
    assert!(snapshot.contains("Git: skipped · cwd foreign-windows on this host"));
}

#[test]
fn registry_summary_surfaces_host_local_mode() {
    let backend = TestBackend::new(120, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.threads[0].metadata.cwd = std::env::current_dir()
        .expect("cwd")
        .to_string_lossy()
        .into_owned();
    crate::app::reduce(&mut app, crate::app::Action::ToggleHostLocalFilter);

    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut snapshot = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }

    assert!(snapshot.contains("LOCAL ONLY"));
    assert!(snapshot.contains("l local-only"));
}

#[test]
fn registry_summary_surfaces_repo_only_mode() {
    let backend = TestBackend::new(120, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.threads[0].metadata.cwd = std::env::current_dir()
        .expect("cwd")
        .to_string_lossy()
        .into_owned();
    crate::app::reduce(&mut app, crate::app::Action::ToggleRepoBackedFilter);

    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut snapshot = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }

    assert!(snapshot.contains("REPO ONLY"));
    assert!(snapshot.contains("g repo-only"));
}

#[test]
fn registry_footer_exposes_terminal_shortcut() {
    let snapshot = render_snapshot(100);
    assert!(
        snapshot.contains("t terminal"),
        "Mission Control must expose the Terminal Drawer shortcut"
    );
}

#[test]
fn registry_surfaces_backend_provenance_and_terminal_readiness() {
    let backend = TestBackend::new(160, 16);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.backend_status.source = "codex-app-server".into();
    app.backend_status.connected = true;
    app.backend_status.registry_complete = true;
    app.backend_status.platform = Some("linux/linux".into());
    app.backend_status.codex_home = Some("/home/jun/.codex".into());
    app.threads[0].metadata.cwd.clear();

    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut snapshot = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }

    assert!(snapshot.contains("Mission Control · codex-app-server · linux/linux"));
    assert!(snapshot.contains("Selected"));
    assert!(!snapshot.contains("Backend: codex-app-server"));
    assert!(snapshot.contains("Codex home: /home/jun/.codex"));
    assert!(snapshot.contains("selected cwd: empty · terminal blocked · git skipped"));

    app.threads[0].metadata.cwd = std::env::current_dir()
        .expect("cwd")
        .to_string_lossy()
        .into_owned();
    let status = registry_scope_status(&app, 160);
    assert!(status.contains("selected cwd: unprobed · terminal unchecked · git not-probed"));

    let effects = crate::app::reduce(&mut app, crate::app::Action::RefreshGitProjections);
    assert!(!effects.is_empty());
    let status = registry_scope_status(&app, 160);
    assert!(status.contains("selected cwd: local · terminal ready · git probing"));
}

#[test]
fn daily_selected_hides_unavailable_forge_internals_but_workspace_keeps_doctor_hint() {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let thread = app.threads[0].clone();
    app.forge_observations.insert(
        thread.id.0.clone(),
        crate::forge::ForgeObservation::unavailable(
            thread.id.clone(),
            thread.metadata.cwd.clone(),
            "resolve GitLab project: glab api /projects/example failed",
        ),
    );

    assert!(forge_context_lines(&app, &thread.id, false).is_empty());
    let diagnostic = forge_context_lines(&app, &thread.id, true);
    let diagnostic_text = diagnostic
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<Vec<_>>()
        .join("");
    assert!(diagnostic_text.contains("doctor forge"));
    assert!(!diagnostic_text.contains("resolve GitLab project"));
}

#[test]
fn registry_selected_detail_and_offline_footer_neutralize_untrusted_labels() {
    let mut threads = FakeBackend::seeded().snapshot().threads;
    threads[0].id = crate::domain::ThreadId::new("thread\u{202e}spoof");
    threads[0].metadata.model = Some("model\u{202e}spoof".into());
    threads[0].metadata.cwd = std::env::current_dir()
        .expect("current dir")
        .to_string_lossy()
        .into_owned();
    let mut app = AppState::new(threads);
    app.backend_status.source = "backend\u{202e}spoof".into();
    let selected = app.threads[0].clone();
    let mut git_context =
        crate::git::GitContext::pending(selected.id.clone(), selected.metadata.cwd.clone());
    git_context.observed_at_unix_ms = 1;
    git_context.is_repository = true;
    git_context.branch = Some("feature\u{202e}spoof".into());
    app.git_contexts.insert(selected.id.0.clone(), git_context);

    let render_detail = |app: &AppState| {
        let mut terminal = Terminal::new(TestBackend::new(180, 24)).expect("terminal");
        terminal.draw(|frame| render(frame, app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    };

    let live = render_detail(&app);
    for visible in [
        "Thread: thread\u{fffd}spoof",
        "Model: model\u{fffd}spoof",
        "Git: feature\u{fffd}spoof",
        "backend\u{fffd}spoof",
    ] {
        assert!(
            live.contains(visible),
            "selected detail missing {visible:?}"
        );
    }
    assert!(!live.contains('\u{202e}'));

    app.git_contexts
        .get_mut(&selected.id.0)
        .expect("git context")
        .error = Some("fatal\u{202e}spoof".into());
    let degraded = render_detail(&app);
    assert!(degraded.contains("fatal\u{fffd}spoof"));
    assert!(!degraded.contains('\u{202e}'));
}

#[test]
fn registry_scope_status_distinguishes_repository_backing() {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.threads[0].metadata.cwd = std::env::current_dir()
        .expect("cwd")
        .to_string_lossy()
        .into_owned();
    let thread_id = app.threads[0].id.clone();

    assert_eq!(selected_git_status(&app), "not-probed");

    let pending =
        crate::git::GitContext::pending(thread_id.clone(), app.threads[0].metadata.cwd.clone());
    app.git_contexts.insert(thread_id.0.clone(), pending);
    assert_eq!(selected_git_status(&app), "probing");

    let context = app.git_contexts.get_mut(&thread_id.0).expect("git context");
    context.observed_at_unix_ms = 1;
    assert_eq!(selected_git_status(&app), "not-repo");

    app.git_contexts
        .get_mut(&thread_id.0)
        .expect("git context")
        .is_repository = true;
    assert_eq!(selected_git_status(&app), "repo");

    app.git_contexts
        .get_mut(&thread_id.0)
        .expect("git context")
        .error = Some("git unavailable".into());
    assert_eq!(selected_git_status(&app), "degraded");
}

#[test]
fn registry_status_surfaces_terminal_failure_notice() {
    let backend = TestBackend::new(120, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.mutation_notice =
        Some("terminal drawer unavailable: selected Codex thread has no cwd".into());

    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut snapshot = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }

    assert!(snapshot.contains("notice"));
    assert!(snapshot.contains("terminal drawer unavailable"));
}

#[test]
fn compact_registry_handles_cjk_emoji_graphemes_and_control_text() {
    let backend = TestBackend::new(60, 12);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.threads[0].workspace = "机器人研发中心".into();
    app.threads[0].alias = None;
    app.threads[0].title = "唤醒词👨‍👩‍👧‍👦 e\u{301} 测试\n控制\u{0007}字符".into();

    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut snapshot = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }

    for glyph in ['机', '器', '人', '唤', '醒', '词'] {
        assert!(
            snapshot.contains(glyph),
            "wide glyph {glyph:?} missing from TestBackend buffer"
        );
    }
    assert!(!snapshot.contains('\u{0007}'));
}

#[test]
fn simplified_chinese_status_helpers_cover_terminal_forge_and_receipts() {
    assert_eq!(
        terminal_process_state_label(
            &TerminalProcessState::Running,
            UiLanguage::SimplifiedChinese
        ),
        "运行中"
    );
    assert_eq!(
        forge_freshness_label(ForgeFreshness::Stale, UiLanguage::SimplifiedChinese),
        "过期"
    );
    assert_eq!(
        operation_state_label(OperationState::Succeeded, UiLanguage::SimplifiedChinese),
        "已成功"
    );
    assert_eq!(
        scratch_state_label(ScratchState::Ready, UiLanguage::SimplifiedChinese),
        "就绪"
    );
}

#[test]
fn managed_worktree_confirmation_neutralizes_untrusted_labels_and_input() {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let thread = app.threads[0].clone();
    app.view = View::ManagedWorktrees(thread.id.clone());
    let repo = crate::domain::LocalRepoIdentity {
        git_common_dir: "/repo/.git".into(),
        primary_root: "/repo\u{202e}spoof".into(),
    };
    let mut context = crate::git::GitContext::pending(thread.id.clone(), thread.metadata.cwd);
    context.observed_at_unix_ms = 1;
    context.is_repository = true;
    context.repo = Some(repo.clone());
    app.git_contexts.insert(thread.id.0, context);

    let mut plan = crate::operation::OperationPlan::create_worktree(
        repo,
        "/work\u{202e}spoof".into(),
        "/target\u{202e}spoof".into(),
        "branch\u{202e}spoof".into(),
        "base\u{202e}spoof".into(),
        1,
    );
    plan.expected_side_effect = "expected\u{202e}spoof".into();
    plan.preconditions[0].key = "precondition\u{202e}spoof".into();
    plan.preconditions[0].expected = "condition\u{202e}spoof".into();
    app.pending_operation = Some(plan);
    app.input_mode = InputMode::WorktreeDeleteBranch;
    app.input_buffer = "typed\u{202e}spoof".into();

    let mut terminal = Terminal::new(TestBackend::new(180, 48)).expect("terminal");
    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut snapshot = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }

    for label in [
        "Repository: /repo\u{fffd}spoof",
        "Cwd: /work\u{fffd}spoof",
        "Target worktree: /target\u{fffd}spoof",
        "Target branch: branch\u{fffd}spoof",
        "Expected: expected\u{fffd}spoof",
        "precondition\u{fffd}spoof = condition\u{fffd}spoof",
        "typed\u{fffd}spoof",
    ] {
        assert!(
            snapshot.contains(label),
            "missing safe confirmation label {label:?}"
        );
    }
    assert!(!snapshot.contains('\u{202e}'));
}

#[test]
fn board_change_request_state_is_display_sanitized() {
    let mut threads = FakeBackend::seeded().snapshot().threads;
    let thread = threads.remove(0);
    let mut card = crate::planning::reconcile_thread_card(crate::planning::ReconcileInput {
        thread: &thread,
        git: None,
        local: None,
        collision_count: 0,
        backend_observed_at_unix_ms: None,
        backend_error: None,
        now_unix_ms: 1,
    });
    card.change_request_state = Some("opened\u{202e}spoof".into());
    card.change_request_draft = true;
    assert_eq!(
        planning_card_field(&card, "change-request", UiLanguage::English),
        Some("opened\u{fffd}spoof/draft".into())
    );
    assert_eq!(
        card.change_request_state.as_deref(),
        Some("opened\u{202e}spoof"),
        "display must not rewrite canonical Forge state"
    );
}

fn overlay_snapshot(
    app: &AppState,
    paint: fn(&mut ratatui::Frame<'_>, &AppState),
) -> String {
    let mut terminal = Terminal::new(TestBackend::new(180, 48)).expect("terminal");
    terminal.draw(|frame| paint(frame, app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut snapshot = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }
    snapshot
}

#[test]
fn forge_confirm_overlay_escapes_untrusted_identity_branches_and_guards() {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.pending_forge_operation = Some(crate::forge_mutation::ForgeMutationPlan {
        operation_id: "id".into(),
        kind: crate::forge_mutation::ForgeMutationKind::CreateMergeRequest,
        provider: crate::forge::ForgeProviderKind::GitLab,
        cwd: "/repo".into(),
        host: "host\u{202e}spoof".into(),
        project_id: "42".into(),
        project_path: "team\u{202e}spoof".into(),
        change_request_iid: None,
        source_branch: Some("source\u{202e}spoof".into()),
        target_branch: Some("target\u{202e}spoof".into()),
        title: Some("title\u{202e}spoof".into()),
        payload_bytes: None,
        expected_side_effect: "effect\u{202e}spoof".into(),
        preconditions: vec![crate::forge_mutation::ForgeMutationPrecondition {
            key: "guard\u{202e}spoof".into(),
            expected: "expected\u{202e}spoof".into(),
        }],
        planned_at_unix_ms: 1,
    });
    let snapshot = overlay_snapshot(&app, render_forge_mutation_confirmation);
    for fragment in [
        "host\u{fffd}spoof/team\u{fffd}spoof",
        "source\u{fffd}spoof -> target\u{fffd}spoof",
        "title\u{fffd}spoof",
        "effect\u{fffd}spoof",
        "guard\u{fffd}spoof = expected\u{fffd}spoof",
    ] {
        assert!(
            snapshot.contains(fragment),
            "Forge confirmation missing safe fragment {fragment:?}"
        );
    }
    assert!(!snapshot.contains('\u{202e}'));
    assert_eq!(
        app.pending_forge_operation.as_ref().expect("plan").host,
        "host\u{202e}spoof"
    );
}

#[test]
fn launch_confirmation_overlay_escapes_paths_without_mutating_exact_argv() {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.pending_launch_plan = Some(crate::launch::LaunchPlan {
        name: "preset\u{202e}spoof".into(),
        argv: vec!["printf".into(), "arg\u{202e}spoof".into()],
        cwd: "/cwd\u{202e}spoof".into(),
        config_path: "/config\u{202e}spoof".into(),
    });
    let snapshot = overlay_snapshot(&app, render_launch_confirmation);
    for fragment in [
        "Preset: preset\u{fffd}spoof",
        "Config: /config\u{fffd}spoof",
        "Cwd: /cwd\u{fffd}spoof",
        "Exact argv:",
    ] {
        assert!(
            snapshot.contains(fragment),
            "Launch confirmation missing safe fragment {fragment:?}"
        );
    }
    assert!(!snapshot.contains('\u{202e}'));
    assert_eq!(
        app.pending_launch_plan.as_ref().expect("plan").argv[1],
        "arg\u{202e}spoof"
    );
}

#[test]
fn local_batch_confirmation_and_input_overlays_escape_untrusted_text() {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.pending_local_batch = Some(crate::batch_local::LocalBatchPlan {
        action: crate::batch_local::LocalBatchAction::AddTag("tag\u{202e}spoof".into()),
        targets: vec![crate::batch_local::LocalBatchTarget {
            local_id: "item\u{202e}spoof".into(),
            anchor: crate::planning::SourceRef {
                kind: crate::planning::SourceKind::ScratchWork,
                value: "scratch:1".into(),
            },
            title: "title\u{202e}spoof".into(),
        }],
        planned_at_unix_ms: 1,
    });
    let batch = overlay_snapshot(&app, render_local_batch_confirmation);
    assert!(batch.contains("item\u{fffd}spoof"));
    assert!(batch.contains("title\u{fffd}spoof"));
    assert!(!batch.contains('\u{202e}'));

    app.input_mode = InputMode::Note;
    app.input_buffer = "typed\u{202e}spoof".into();
    let input = overlay_snapshot(&app, render_local_input_overlay);
    assert!(input.contains("typed\u{fffd}spoof"));
    assert!(!input.contains('\u{202e}'));

    app.command_palette_query = "query\u{202e}spoof".into();
    let palette = overlay_snapshot(&app, render_command_palette);
    assert!(palette.contains("query\u{fffd}spoof"));
    assert!(!palette.contains('\u{202e}'));
}

#[test]
fn terminal_drawer_preserves_semantic_cjk_rows() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.terminal_drawer_open = true;
    app.terminal_focused = false;
    app.terminal_snapshot = Some(crate::terminal_drawer::TerminalSnapshot {
        cwd: "/repo/机器人".into(),
        size: TerminalSize { rows: 7, cols: 78 },
        rows: vec!["中文终端 e\u{301} 👩‍💻".into()],
        cursor_row: 0,
        cursor_col: 0,
        scrollback: 0,
        state: crate::terminal_drawer::TerminalProcessState::Running,
    });

    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut snapshot = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }
    for glyph in ['中', '文', '终', '端', '机', '器', '人'] {
        assert!(
            snapshot.contains(glyph),
            "wide glyph {glyph:?} missing from TestBackend buffer"
        );
    }
}

#[test]
fn terminal_drawer_reserves_rows_instead_of_covering_main_view() {
    let full = Rect::new(0, 0, 100, 30);
    let drawer = terminal_drawer_rect(full);
    let main = primary_view_rect(full, true);
    assert_eq!(main.height + drawer.height, full.height);
    assert_eq!(main.y + main.height, drawer.y);

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = AppState::new(FakeBackend::scaled(100).snapshot().threads);
    app.selected = 99;
    app.terminal_drawer_open = true;
    app.terminal_snapshot = Some(crate::terminal_drawer::TerminalSnapshot {
        cwd: "/repo".into(),
        size: terminal_drawer_pty_size(100, 30),
        rows: vec!["drawer row".into()],
        cursor_row: 0,
        cursor_col: 0,
        scrollback: 0,
        state: crate::terminal_drawer::TerminalProcessState::Running,
    });

    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut scrollbar_rows = Vec::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if buffer[(x, y)].symbol() == "█" {
                scrollbar_rows.push(y);
            }
        }
    }
    assert!(!scrollbar_rows.is_empty());
    assert!(
        scrollbar_rows.iter().all(|row| *row < drawer.y),
        "Mission Control scrollbar must stay above the Drawer"
    );

    let drawer_row = (drawer.y..drawer.y + drawer.height)
        .flat_map(|y| (0..buffer.area.width).map(move |x| buffer[(x, y)].symbol()))
        .collect::<String>();
    assert!(drawer_row.contains("Terminal Drawer"));
    assert!(drawer_row.contains("drawer row"));
}

#[test]
fn terminal_drawer_overlay_renders_status_rows_and_focus_hint() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.terminal_drawer_open = true;
    app.terminal_focused = true;
    app.terminal_snapshot = Some(crate::terminal_drawer::TerminalSnapshot {
        cwd: "/repo".into(),
        size: TerminalSize { rows: 10, cols: 98 },
        rows: vec!["hello from PTY".into(), "$ ".into()],
        cursor_row: 1,
        cursor_col: 2,
        scrollback: 0,
        state: crate::terminal_drawer::TerminalProcessState::Running,
    });

    terminal.draw(|frame| render(frame, &app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut snapshot = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            snapshot.push_str(buffer[(x, y)].symbol());
        }
        snapshot.push('\n');
    }
    assert!(snapshot.contains("Terminal Drawer"));
    assert!(snapshot.contains("hello from PTY"));
    assert!(snapshot.contains("F6 release"));
}

#[test]
fn terminal_drawer_size_is_bounded_and_accounts_for_border() {
    assert_eq!(
        terminal_drawer_pty_size(100, 30),
        TerminalSize { rows: 10, cols: 98 }
    );
    let small = terminal_drawer_pty_size(20, 8);
    assert!(small.rows > 0);
    assert!(small.cols > 0);
}

#[test]
fn board_viewport_keeps_large_selection_visible() {
    let viewport = board_viewport(10_000, 7_321, 22);
    assert_eq!(viewport.row_capacity, 20);
    assert!(7_321 >= viewport.start);
    assert!(7_321 < viewport.start + viewport.row_capacity);
    assert!(viewport.start <= 10_000 - viewport.row_capacity);
}

#[test]
fn board_viewport_stays_at_zero_when_content_fits() {
    assert_eq!(
        board_viewport(5, 4, 12),
        BoardViewport {
            start: 0,
            row_capacity: 10,
        }
    );
}

#[test]
fn board_viewport_handles_zero_height_without_underflow() {
    assert_eq!(
        board_viewport(100, 73, 1),
        BoardViewport {
            start: 0,
            row_capacity: 0,
        }
    );
}

#[test]
fn help_hints_match_locked_keyboard_commands() {
    use crate::app::ViewKind;
    use crate::{command::Command, keymap::command_for_key};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let help = HELP_LINES.join("\n");
    for (needle, key, view, command) in [
        (
            "? help",
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
            ViewKind::Registry,
            Command::Help,
        ),
        (
            "/ search",
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
            ViewKind::Registry,
            Command::Search,
        ),
        (
            ". actions",
            KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE),
            ViewKind::Registry,
            Command::ContextActions,
        ),
        (
            "Terminal drawer: t open",
            KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
            ViewKind::Workspace,
            Command::TerminalDrawer,
        ),
        (
            "T close",
            KeyEvent::new(KeyCode::Char('T'), KeyModifiers::NONE),
            ViewKind::Workspace,
            Command::CloseTerminalDrawer,
        ),
    ] {
        assert!(help.contains(needle), "missing help hint {needle:?}");
        assert_eq!(command_for_key(key, view), Some(command));
    }
}

#[test]
fn ui_source_does_not_require_private_use_icon_fonts() {
    fn private_use(ch: char) -> bool {
        matches!(
            ch as u32,
            0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd
        )
    }

    assert!(
        !include_str!("../ui.rs").chars().any(private_use),
        "UI source must not depend on Nerd Font/private-use glyphs"
    );
}

#[test]
fn very_narrow_registry_layouts_render_without_panicking() {
    for width in [20, 30, 40] {
        let snapshot = render_snapshot(width);
        assert!(!snapshot.is_empty(), "width={width}");
    }
}

#[test]
fn responsive_layout_breakpoints_are_locked() {
    assert_eq!(layout_mode(40), LayoutMode::Compact);
    assert_eq!(layout_mode(80), LayoutMode::Standard);
    assert_eq!(layout_mode(120), LayoutMode::Wide);
    assert_eq!(layout_mode(160), LayoutMode::Wide);
}

#[test]
fn snapshots_cover_40_80_120_160_columns() {
    for width in [40, 80, 120, 160] {
        let snapshot = render_snapshot(width);
        assert!(snapshot.contains("Mission Control"), "width={width}");
        assert!(snapshot.contains("M0 bootstrap"), "width={width}");
        if width >= 120 {
            assert!(snapshot.contains("Selected"), "width={width}");
        }
    }
}
