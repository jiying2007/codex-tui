//! Queue rejection and unavailable-backend failures use the same submission ticket.
use codex_tui::{
    app::{Action, AppState, reduce},
    app_server::RegistryHandle,
    domain::ThreadId,
    i18n::pick,
};

pub(crate) fn submit(
    app: &mut AppState,
    registry: Option<&RegistryHandle>,
    thread_id: ThreadId,
    request_id: u64,
    text: String,
    active_turn_id: Option<String>,
) {
    let result = match registry {
        Some(registry) => {
            registry.submit_prompt(thread_id.clone(), request_id, text, active_turn_id)
        }
        None => Err(anyhow::anyhow!(pick(
            app.language,
            "conversation backend unavailable",
            "会话后端不可用"
        ))),
    };
    if let Err(error) = result {
        reduce(
            app,
            Action::PromptFailed {
                thread_id,
                request_id,
                error: error.to_string(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_tui::{
        app::{Effect, InputMode},
        backend::{CodexBackend, FakeBackend},
        conversation::ConversationPage,
    };

    #[test]
    fn queue_rejection_retains_draft_and_resolves_only_its_ticket() {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        reduce(&mut app, Action::QuickPrompt);
        let id = app.current_thread_id().unwrap().clone();
        let page = ConversationPage {
            thread_id: id.clone(),
            title: None,
            turns: vec![],
            items: vec![],
            next_turn_cursor: None,
            next_item_cursor: None,
        };
        reduce(&mut app, Action::ConversationLoaded(page.clone()));
        reduce(&mut app, Action::SetDraft("not sent".into()));
        let effects = reduce(&mut app, Action::CommitInput);
        let [
            Effect::SubmitPrompt {
                request_id,
                text,
                active_turn_id,
                ..
            },
        ] = effects.as_slice()
        else {
            panic!("submit");
        };
        let old = *request_id;
        submit(
            &mut app,
            None,
            id.clone(),
            old,
            text.clone(),
            active_turn_id.clone(),
        );
        assert_eq!(app.thread_ui[&id.0].draft, "not sent");
        assert!(app.conversations[&id.0].error.is_some());
        reduce(&mut app, Action::ConversationLoaded(page));
        app.input_mode = InputMode::Composer;
        let effects = reduce(&mut app, Action::CommitInput);
        assert!(
            matches!(effects.as_slice(), [Effect::SubmitPrompt { request_id, .. }] if *request_id != old)
        );
    }
}
