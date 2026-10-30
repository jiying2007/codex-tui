//! First-connection retry only; never reconnect an established registry or replay writes.
use anyhow::Result;
use codex_tui::{
    app::{Action, AppState, InputMode, ViewKind, reduce},
    app_server::{self, StartedRegistry},
    app_server_target::ResolvedAppServerTarget,
    backend::BackendStatus,
    i18n::{UiLanguage, pick},
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::task::JoinHandle;

pub(crate) fn retry_hint(language: UiLanguage) -> &'static str {
    pick(
        language,
        "F5: retry startup in Mission Control",
        "F5：返回任务中心重试首次连接",
    )
}

fn eligible(app: &AppState, key: KeyEvent) -> bool {
    key.code == KeyCode::F(5)
        && key.kind == KeyEventKind::Press
        && key.modifiers == KeyModifiers::NONE
        && app.view_kind() == ViewKind::Registry
        && app.input_mode == InputMode::Normal
        && !app.should_quit
        && !app.backend_status.connected
        && app.backend_status.error.is_some()
        && app.threads.is_empty()
        && app.conversations.is_empty()
        && app.pending_requests.is_empty()
        && !app.request_details_open()
        && !app.terminal_drawer_open
        && !app.terminal_focused
        && !app.command_palette_open
        && !app.show_help
        && !app.context_open
        && !app.launch_menu_open
        && !app.thread_queue_open
        && !app.transcript_search_open
        && !app.goal_actions_open
        && app.saved_view_editor.is_none()
        && app.pending_operation.is_none()
        && app.pending_forge_operation.is_none()
        && app.pending_local_batch.is_none()
        && app.pending_launch_plan.is_none()
        && app.pending_queue_confirmation.is_none()
        && app.hot_slot_bind_pending.is_none()
}

// Tests inject the spawn boundary and cannot resolve or start a real backend.
fn try_retry_with<T, F>(
    app: &mut AppState,
    key: KeyEvent,
    target: Option<&ResolvedAppServerTarget>,
    ever_connected: bool,
    task: &mut Option<JoinHandle<Result<T>>>,
    start: F,
) -> bool
where
    F: FnOnce(ResolvedAppServerTarget) -> JoinHandle<Result<T>>,
{
    if ever_connected || task.is_some() || !eligible(app, key) {
        return false;
    }
    let Some(target) = target else {
        return false;
    };
    let target = target.clone();
    let status = BackendStatus::starting(target.status_source());
    *task = Some(start(target));
    reduce(app, Action::BackendStatus(status));
    true
}

pub(crate) fn try_retry(
    app: &mut AppState,
    key: KeyEvent,
    target: Option<&ResolvedAppServerTarget>,
    ever_connected: bool,
    task: &mut Option<JoinHandle<Result<StartedRegistry>>>,
) -> bool {
    try_retry_with(app, key, target, ever_connected, task, |target| {
        tokio::spawn(app_server::start_target(target))
    })
}

/// Await only a completed task: stalled startup cannot block input.
pub(crate) async fn take_finished<T>(
    task: &mut Option<JoinHandle<Result<T>>>,
) -> Option<Result<T>> {
    if !task.as_ref().is_some_and(JoinHandle::is_finished) {
        return None;
    }
    let task = task.take()?;
    Some(match task.await {
        Ok(result) => result,
        Err(error) => Err(anyhow::anyhow!("App Server connection task failed: {error}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_tui::app::View;
    use codex_tui::backend::{CodexBackend, FakeBackend};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn failed_app() -> AppState {
        let mut app = AppState::new(vec![]);
        app.backend_status = BackendStatus::starting("startup-test".into());
        app.backend_status.error = Some("synthetic first-connection failure".into());
        app
    }

    fn f5() -> KeyEvent {
        KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE)
    }

    async fn finish<T>(task: &Option<JoinHandle<Result<T>>>) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !task.as_ref().unwrap().is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[test]
    fn only_unmodified_single_f5_press_requests_startup_retry() {
        let app = failed_app();
        assert!(eligible(&app, f5()));
        for modifiers in [
            KeyModifiers::CONTROL,
            KeyModifiers::ALT,
            KeyModifiers::SHIFT,
            KeyModifiers::SUPER,
        ] {
            assert!(!eligible(&app, KeyEvent::new(KeyCode::F(5), modifiers)));
        }
        for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
            let key = KeyEvent::new_with_kind(KeyCode::F(5), KeyModifiers::NONE, kind);
            assert!(!eligible(&app, key));
        }
        assert!(!eligible(
            &app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        ));
    }

    #[test]
    fn startup_retry_never_steals_focus_or_reuses_a_populated_session() {
        let blockers: &[fn(&mut AppState)] = &[
            |a| a.show_help = true,
            |a| a.context_open = true,
            |a| a.command_palette_open = true,
            |a| a.launch_menu_open = true,
            |a| a.thread_queue_open = true,
            |a| a.transcript_search_open = true,
            |a| a.goal_actions_open = true,
            |a| a.terminal_drawer_open = true,
            |a| a.terminal_focused = true,
            |a| a.should_quit = true,
            |a| a.input_mode = InputMode::Composer,
            |a| a.view = View::Board,
            |a| a.backend_status.connected = true,
            |a| a.backend_status.error = None,
        ];
        for block in blockers {
            let mut app = failed_app();
            block(&mut app);
            assert!(!eligible(&app, f5()));
        }
        let mut app = failed_app();
        app.threads = FakeBackend::scaled(1).snapshot().threads;
        assert!(!eligible(&app, f5()));
    }

    #[tokio::test]
    async fn one_attempt_preserves_local_state_and_the_exact_target() {
        let mut app = failed_app();
        app.filter = "keep search".into();
        reduce(&mut app, Action::ToggleHostLocalFilter);
        let before = app.to_local_state();
        let target = ResolvedAppServerTarget::implicit_local(Some("unused-fixture".into()));
        let mut task = None;
        let accepted = try_retry_with(
            &mut app,
            f5(),
            Some(&target),
            false,
            &mut task,
            |received| {
                assert_eq!(received, target);
                tokio::spawn(std::future::pending::<Result<()>>())
            },
        );
        assert!(accepted);
        assert_eq!(app.to_local_state(), before);
        assert_eq!(app.filter, "keep search");
        assert!(!try_retry_with(
            &mut app,
            f5(),
            Some(&target),
            false,
            &mut task,
            |_| panic!("duplicate attempt"),
        ));
        task.take().unwrap().abort();
    }

    #[tokio::test]
    async fn fake_mode_or_previously_connected_session_cannot_retry() {
        let target = ResolvedAppServerTarget::implicit_local(None);
        for (configured, connected) in [(None, false), (Some(&target), true)] {
            let mut app = failed_app();
            let mut task: Option<JoinHandle<Result<()>>> = None;
            assert!(!try_retry_with(
                &mut app,
                f5(),
                configured,
                connected,
                &mut task,
                |_| panic!("forbidden backend start"),
            ));
            assert!(task.is_none());
        }
    }

    #[tokio::test]
    async fn pending_connection_poll_returns_without_waiting() {
        let mut task = Some(tokio::spawn(std::future::pending::<Result<()>>()));
        assert!(take_finished(&mut task).await.is_none());
        assert!(task.is_some());
        task.take().unwrap().abort();
    }

    #[tokio::test]
    async fn failed_attempt_does_not_schedule_an_automatic_retry() {
        let target = ResolvedAppServerTarget::implicit_local(None);
        let starts = Arc::new(AtomicUsize::new(0));
        let mut task = None;
        let mut app = failed_app();
        let counter = starts.clone();
        let accepted = try_retry_with(
            &mut app,
            f5(),
            Some(&target),
            false,
            &mut task,
            move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async { Err::<(), _>(anyhow::anyhow!("fixture unavailable")) })
            },
        );
        assert!(accepted);
        finish(&task).await;
        assert!(take_finished(&mut task).await.unwrap().is_err());
        for _ in 0..5 {
            assert!(take_finished(&mut task).await.is_none());
        }
        assert!(task.is_none());
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn completed_result_is_taken_once() {
        let mut task = Some(tokio::spawn(async { Ok::<_, anyhow::Error>(7u8) }));
        finish(&task).await;
        assert_eq!(take_finished(&mut task).await.unwrap().unwrap(), 7);
        assert!(take_finished(&mut task).await.is_none());
    }
}
