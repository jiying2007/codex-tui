use super::*;
use crate::app::{Action, ViewKind, reduce};
use crate::backend::{CodexBackend, FakeBackend};
use crate::command::Command;
use crate::conversation::{InteractiveRequestKind, RpcRequestId};
use crate::domain::ThreadId;
use crate::keymap::command_for_key;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

fn render_app(app: &AppState, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).expect("terminal");
    terminal.draw(|frame| render(frame, app)).expect("draw");
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            let mut line = String::new();
            let mut x = 0;
            while x < buffer.area.width {
                let symbol = buffer[(x, y)].symbol();
                line.push_str(symbol);
                x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn app() -> AppState {
    let mut source = FakeBackend::scaled(3).snapshot().threads;
    for thread in &mut source {
        thread.attention.clear();
        thread.pinned = false;
        thread.runtime = RuntimeStatus::Working;
    }
    source[2].attention.push(AttentionReason::ApprovalRequired);
    let mut app = AppState::new(vec![]);
    app.backend_status = FakeBackend::seeded().snapshot().status;
    reduce(&mut app, Action::ReplaceThreads(source));
    app
}

fn linked_app() -> AppState {
    use crate::app_server_target::ResolvedAppServerTarget;
    use crate::workref::{ContractRef, EvidenceRef, SessionRef, WorkItem, WorkRefLedger};
    let mut app = app();
    let target = ResolvedAppServerTarget::implicit_local(None);
    app.configure_operator_target(&target);
    let mut status = crate::backend::BackendStatus::starting(target.status_source());
    status.connected = true;
    status.codex_home = Some("/home/codex/.codex".into());
    app.backend_status = status;
    app.apply_local_state(&crate::store::LocalStateV1::default());
    let mut ledger = WorkRefLedger::default();
    ledger
        .add_item(
            "BUG17".into(),
            WorkItem::new(
                "tapd".into(),
                "robot".into(),
                "bug".into(),
                "17".into(),
                Some("https://www.tapd.cn/".into()),
            )
            .unwrap(),
        )
        .unwrap();
    let thread = app.threads[0].id.0.clone();
    ledger
        .add_session(
            "BUG17",
            "soc".into(),
            SessionRef {
                target_identity: crate::store::target_instance_identity(&target),
                codex_home: "/home/codex/.codex".into(),
                thread_id: thread.clone(),
                repository: "robot-app".into(),
                role: "implementation".into(),
            },
        )
        .unwrap();
    ledger
        .add_session(
            "BUG17",
            "mcu".into(),
            SessionRef {
                target_identity: "different-target-identity".into(),
                codex_home: "/home/codex/.codex".into(),
                thread_id: thread,
                repository: "mcu-fw".into(),
                role: "firmware".into(),
            },
        )
        .unwrap();
    ledger
        .add_contract(
            "BUG17",
            ContractRef {
                kind: "protocol".into(),
                source: "docs/protocol.md".into(),
                revision: "rev-3".into(),
                git_sha: Some("a".repeat(40)),
            },
        )
        .unwrap();
    for (kind, result) in [("unit", "passed"), ("integration", "unknown")] {
        ledger
            .add_evidence(
                "BUG17",
                EvidenceRef {
                    kind: kind.into(),
                    source: format!("test/{kind}"),
                    revision: "appA+fwB".into(),
                    result: result.into(),
                    environment: "bench".into(),
                },
            )
            .unwrap();
    }
    ledger
        .prepare_handoff("BUG17", "soc".into(), "mcu".into(), "Review rev-3".into())
        .unwrap();
    app.install_workrefs(Some(ledger));
    // The fixture's native approval moves registry selection to row 2.
    // Select the actually linked row before asserting its detail.
    app.selected = 0;
    app
}

#[test]
fn g23_manual_links_appear_in_wide_mission_control_and_80_column_thread() {
    let mut app = linked_app();
    let thread_id = app.threads[0].id.0.clone();
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        app.language = language;
        let lines = super::super::provenance::manual_workref_lines(&app, &thread_id)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(lines.contains("BUG17"), "{language:?}: {lines}");
        assert!(lines.contains("unit=passed"), "{lines}");
        assert!(lines.contains("integration=unknown"), "{lines}");
        assert!(lines.contains("prepared=1 ack=0"), "{lines}");
        assert!(!lines.contains("integration=passed"));
        let wide = render_app(&app, 140);
        assert!(wide.contains("BUG17"), "{language:?}: {wide}");
        app.view = crate::app::View::Thread(app.threads[0].id.clone());
        let narrow = render_app(&app, 80);
        let label = if language.is_simplified_chinese() {
            "关联（手工未核验）"
        } else {
            "Linked (manual/unverified)"
        };
        assert!(narrow.contains(label), "{language:?}: {narrow}");
        assert!(narrow.contains("BUG17"), "{narrow}");
        app.view = crate::app::View::Registry;
    }
}

#[test]
fn g23_unverified_identity_or_broken_local_data_never_exposes_links() {
    let mut app = linked_app();
    let thread_id = app.threads[0].id.0.clone();
    app.backend_status.codex_home = Some("/wrong-home".into());
    assert!(app.manual_workrefs_for_thread(&thread_id).is_empty());
    assert!(!render_app(&app, 140).contains("BUG17"));
    app.backend_status.connected = false;
    assert!(app.manual_workrefs_for_thread(&thread_id).is_empty());
    app.install_workrefs(None);
    let missing = render_app(&app, 140);
    assert!(missing.contains("UNAVAILABLE"), "{missing}");
    assert!(!missing.contains("BUG17"));
}

#[test]
fn compact_attention_and_delivery_hints_survive_drawer_in_both_languages() {
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        for width in [20, 40, 79, 80] {
            let mut app = app();
            app.language = language;
            app.terminal_drawer_open = true;
            let snapshot = render_app(&app, width);
            assert!(
                snapshot.contains(attention_reason_label(
                    &AttentionReason::ApprovalRequired,
                    language
                )),
                "width={width} language={language:?}: {snapshot}"
            );
            for key in ['b', 'r', 'w'] {
                assert!(
                    snapshot.contains(key),
                    "missing {key}, width={width}: {snapshot}"
                );
            }
            assert!(snapshot.contains(if language.is_simplified_chinese() {
                "待处理 1"
            } else {
                "1 need attention"
            }));
        }
    }
}

#[test]
fn unlocated_request_is_visible_in_both_languages_without_a_fake_thread_row() {
    let mut app = app();
    let original_count = app.threads.len();
    let unknown = ThreadId::new("not-on-current-target");
    reduce(
        &mut app,
        Action::InteractiveRequested(crate::conversation::InteractiveRequest {
            request_id: RpcRequestId::String("pending-remote".into()),
            thread_id: unknown.clone(),
            turn_id: "turn".into(),
            item_id: "item".into(),
            kind: InteractiveRequestKind::UserInput { questions: vec![] },
        }),
    );
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        app.language = language;
        for width in [60, 80] {
            let screen = render_app(&app, width);
            let label = if language.is_simplified_chinese() {
                "!1 请求待定位"
            } else {
                "!1 request unlocated"
            };
            assert!(screen.contains(label), "width={width}: {screen}");
            assert_eq!(app.threads.len(), original_count);
            assert!(!app.threads.iter().any(|thread| thread.id == unknown));
        }
    }
    reduce(
        &mut app,
        Action::InteractiveResolved {
            request_id: RpcRequestId::String("pending-remote".into()),
        },
    );
    assert_eq!(app.unlocated_pending_request_count(), 0);
    assert!(!render_app(&app, 80).contains("request unlocated"));
}

#[test]
fn pending_request_alone_sorts_first_and_renders_interaction_with_drawer() {
    let mut app = app();
    app.threads[2].attention.clear();
    app.selected = 0;
    let request = InteractiveRequest {
        request_id: RpcRequestId::Integer(81),
        thread_id: app.threads[2].id.clone(),
        turn_id: "pending-turn".into(),
        item_id: "pending-input".into(),
        kind: InteractiveRequestKind::UserInput { questions: vec![] },
    };
    assert!(reduce(&mut app, Action::InteractiveRequested(request)).is_empty());
    assert!(app.threads.iter().all(|thread| thread.attention.is_empty()));
    assert_eq!(app.visible_indices(), [2, 0, 1]);
    assert_eq!(
        app.selected, 0,
        "pending arrival must retain manual selection"
    );
    app.terminal_drawer_open = true;
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        app.language = language;
        for width in [20, 40, 79] {
            let snapshot = render_app(&app, width);
            let first_row = snapshot.lines().nth(2).expect("first registry row");
            let label = if language.is_simplified_chinese() {
                "交互"
            } else {
                "interac…"
            };
            assert!(first_row.contains(label), "width={width}: {snapshot}");
            assert!(!first_row.contains("WORKING"));
        }
    }
}

#[test]
fn a_snoozed_thread_without_attention_still_displays_its_runtime() {
    let mut app = app();
    app.threads.truncate(1);
    app.selected = 0;
    let mut local = crate::planning::WorkCardRecord::implicit_thread(&app.threads[0].id);
    local.overlay.snooze_until_unix_ms = Some(10_000);
    app.planning_snapshot.cards.push(local);
    reduce(&mut app, Action::ReconcilePlanning { now_unix_ms: 1 });
    for language in [UiLanguage::English, UiLanguage::SimplifiedChinese] {
        app.language = language;
        let snapshot = render_app(&app, 40);
        assert!(snapshot.contains(runtime_status_label(&RuntimeStatus::Working, language)));
        assert!(!snapshot.contains(if language.is_simplified_chinese() {
            "稍后"
        } else {
            "snoozed"
        }));
    }
}

#[test]
fn footer_delivery_keys_match_executable_routes() {
    let app = app();
    for (key, command, label) in [
        ('b', Command::Board, "b Board"),
        ('r', Command::Review, "r Review"),
        ('w', Command::Workspace, "w Workspace"),
    ] {
        assert_eq!(
            command_for_key(
                KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE),
                ViewKind::Registry
            ),
            Some(command)
        );
        assert!(registry_footer(&app, 100).to_string().contains(label));
    }
    assert!(render_app(&app, 100).contains("Alt+T terminal"));
}
