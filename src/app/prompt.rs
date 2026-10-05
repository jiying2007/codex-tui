//! Ephemeral submission tickets fence acknowledgements from newer draft edits.
//! One accepted send per thread; no automatic retries or persisted prompt copies.
use super::{Action, AppState, Effect, InputMode, local_text, reduce};
use crate::domain::ThreadId;
use std::collections::BTreeMap;

const MAX_PENDING: usize = 32;

#[derive(Clone, Debug)]
struct Pending {
    request_id: u64,
    edited: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct PendingSubmissions {
    next_id: u64,
    by_thread: BTreeMap<String, Pending>,
}

impl PendingSubmissions {
    pub(super) fn edited(&mut self, thread_id: &ThreadId) {
        if let Some(pending) = self.by_thread.get_mut(&thread_id.0) {
            pending.edited = true;
        }
    }

    fn begin(&mut self, thread_id: &ThreadId) -> Option<u64> {
        if self.by_thread.contains_key(&thread_id.0) || self.by_thread.len() >= MAX_PENDING {
            return None;
        }
        self.next_id = self.next_id.checked_add(1)?;
        self.by_thread.insert(
            thread_id.0.clone(),
            Pending {
                request_id: self.next_id,
                edited: false,
            },
        );
        Some(self.next_id)
    }

    fn finish(&mut self, thread_id: &ThreadId, request_id: u64) -> Option<Pending> {
        if self.by_thread.get(&thread_id.0)?.request_id != request_id {
            return None;
        }
        self.by_thread.remove(&thread_id.0)
    }
}

pub(super) fn submit(state: &mut AppState) -> Vec<Effect> {
    let Some(thread_id) = state.current_thread_id().cloned() else {
        return vec![];
    };
    let Some(conversation) = state.conversations.get(&thread_id.0) else {
        return vec![];
    };
    if conversation.loading || conversation.error.is_some() {
        return vec![];
    }
    let text = state
        .thread_ui
        .get(&thread_id.0)
        .map(|ui| ui.draft.trim().to_string())
        .unwrap_or_default();
    if text.is_empty() {
        return vec![];
    }
    let active_turn_id = conversation.active_turn_id().map(ToOwned::to_owned);
    let Some(request_id) = state.prompt_submissions.begin(&thread_id) else {
        state.mutation_notice = Some(
            local_text(
                state.language,
                "prompt already pending or send capacity reached; draft retained",
                "提示词仍在发送或队列已达上限；草稿已保留",
            )
            .into(),
        );
        return vec![];
    };
    state.input_mode = InputMode::Normal;
    vec![Effect::SubmitPrompt {
        thread_id,
        request_id,
        text,
        active_turn_id,
    }]
}

pub(super) fn acknowledge(
    state: &mut AppState,
    thread_id: ThreadId,
    request_id: u64,
) -> Vec<Effect> {
    let Some(pending) = state.prompt_submissions.finish(&thread_id, request_id) else {
        return vec![];
    };
    // The server actor loads authoritative history after this acknowledgement.
    // Until it arrives, the old idle/active-turn snapshot cannot authorize a send.
    if let Some(conversation) = state.conversations.get_mut(&thread_id.0) {
        conversation.loading = true;
        conversation.error = None;
    }
    if !pending.edited
        && let Some(ui) = state.thread_ui.get_mut(&thread_id.0)
    {
        ui.draft.clear();
        return vec![Effect::PersistOperatorState];
    }
    vec![]
}

pub(super) fn fail(state: &mut AppState, thread_id: ThreadId, request_id: u64, error: String) {
    if state
        .prompt_submissions
        .finish(&thread_id, request_id)
        .is_some()
    {
        // Never retry a possibly accepted turn. Preserve the draft and require an
        // explicit history refresh before the user can submit another prompt.
        reduce(state, Action::ConversationFailed { thread_id, error });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tickets_are_bounded_per_thread_and_never_reused() {
        let mut pending = PendingSubmissions::default();
        let first = ThreadId::new("0");
        let ticket = pending.begin(&first).unwrap();
        assert!(pending.begin(&first).is_none());
        for index in 1..MAX_PENDING {
            assert!(pending.begin(&ThreadId::new(index.to_string())).is_some());
        }
        assert!(pending.begin(&ThreadId::new("overflow")).is_none());
        assert!(pending.finish(&first, ticket + 1).is_none());
        assert!(pending.finish(&first, ticket).is_some());
        assert!(pending.begin(&first).unwrap() > ticket);
        pending.next_id = u64::MAX;
        pending.by_thread.clear();
        assert!(pending.begin(&first).is_none());
    }
}
