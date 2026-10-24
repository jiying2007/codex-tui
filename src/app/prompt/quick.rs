//! Open the visible thread composer before accepting or submitting prompt text.
use crate::app::{AppState, Effect, InputMode, View};
use crate::domain::ThreadId;
use crate::planning::SourceKind;

pub(in crate::app) fn open(state: &mut AppState) -> Vec<Effect> {
    let id = match &state.view {
        View::Registry => state.selected_thread_id(),
        View::Board => state.selected_planning_card().and_then(|card| {
            (card.anchor.kind == SourceKind::CodexThread)
                .then(|| ThreadId::new(card.anchor.value.clone()))
                .filter(|id| state.thread_by_id(id).is_some())
        }),
        View::Thread(id) | View::Review(id) | View::Workspace(id) | View::ManagedWorktrees(id) => {
            Some(id.clone())
        }
        View::Scratch(_) => None,
    };
    let Some(id) = id else { return vec![] };
    if !matches!(state.view, View::Thread(_)) {
        state.quick_prompt_return_view = Some(state.view.clone());
        if matches!(state.view, View::Registry) {
            state.previous_target = None;
        }
    }
    state.thread_ui.entry(id.0.clone()).or_default();
    state.prepare_conversation(&id);
    state.view = View::Thread(id.clone());
    state.input_mode = InputMode::Composer;
    vec![Effect::LoadConversation(id)]
}
