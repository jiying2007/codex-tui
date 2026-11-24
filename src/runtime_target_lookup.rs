//! One cancellable user-triggered lookup, independent of the live registry.
use codex_tui::{
    app::{Action, AppState, Effect, LookupPhase, TargetLookupAction, reduce},
    app_server_target::AppServerConfig,
    headless::target_lookup_pages,
    i18n::pick,
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::task::JoinHandle;

pub(crate) fn handle_key(app: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    let plain = key.kind == KeyEventKind::Press && key.modifiers.is_empty();
    let action = match key.code {
        KeyCode::Esc => TargetLookupAction::Close,
        KeyCode::Enter if plain => TargetLookupAction::Submit,
        KeyCode::Up => TargetLookupAction::Move(-1),
        KeyCode::Down => TargetLookupAction::Move(1),
        KeyCode::PageUp => TargetLookupAction::Scroll(-8),
        KeyCode::PageDown => TargetLookupAction::Scroll(8),
        KeyCode::Backspace => TargetLookupAction::Backspace,
        KeyCode::Char(ch)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
        {
            TargetLookupAction::Input(ch.to_string())
        }
        _ => return vec![],
    };
    reduce(app, Action::TargetLookup(action))
}

pub(crate) struct LookupWorker {
    config: Option<AppServerConfig>,
    task: Option<(u64, JoinHandle<Vec<String>>)>,
}

impl LookupWorker {
    pub(crate) fn new(config: Option<AppServerConfig>) -> Self {
        Self { config, task: None }
    }

    pub(crate) async fn advance(&mut self, app: &mut AppState) -> bool {
        if let Some((generation, task)) = &self.task {
            if app.target_lookup.as_ref().map(|panel| panel.generation) != Some(*generation) {
                task.abort();
            }
            if !task.is_finished() {
                return false;
            }
        }
        if let Some((generation, task)) = self.task.take() {
            let pages = task.await.unwrap_or_else(|_| {
                vec![pick(app.language, "Lookup unavailable", "检索不可用").into()]
            });
            if app.target_lookup.as_ref().map(|panel| panel.generation) == Some(generation) {
                reduce(
                    app,
                    Action::TargetLookup(TargetLookupAction::Loaded { generation, pages }),
                );
                return true;
            }
        }
        let Some(panel) = app.target_lookup.as_mut() else {
            return false;
        };
        if panel.phase != LookupPhase::Requested {
            return false;
        }
        let Some(config) = self.config.clone() else {
            panel.phase = LookupPhase::Results(vec![
                pick(
                    app.language,
                    "Lookup unavailable in fixture mode",
                    "模拟模式不连接目标",
                )
                .into(),
            ]);
            return true;
        };
        panel.phase = LookupPhase::Running;
        let generation = panel.generation;
        let query = panel.query.clone();
        let language = app.language;
        self.task = Some((
            generation,
            tokio::spawn(async move { target_lookup_pages(&config, &query, language).await }),
        ));
        true
    }
}

impl Drop for LookupWorker {
    fn drop(&mut self) {
        if let Some((_, task)) = &self.task {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_input::{handle_key as runtime_key, handle_paste};
    use codex_tui::app::ViewKind;

    #[tokio::test]
    async fn palette_lookup_traps_keys_and_fixture_worker_never_connects() {
        let mut app = AppState::new(vec![]);
        let mut worker = LookupWorker::new(None);
        runtime_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
        );
        handle_paste(&mut app, "Search all targets".into());
        runtime_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.target_lookup.is_some());
        assert!(!worker.advance(&mut app).await);
        handle_paste(&mut app, "audio".into());
        runtime_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(worker.advance(&mut app).await);
        assert!(worker.task.is_none());
        let effects = runtime_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        );
        assert!(effects.is_empty());
        assert!(!app.should_quit);
        assert_eq!(app.view_kind(), ViewKind::Registry);
        runtime_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.target_lookup.is_none());
        assert!(!worker.advance(&mut app).await);
    }
}
