//! Resolve actor termination through the same reducer path as normal replies.
//! Unknown send outcomes must not be retried or left occupying prompt capacity.
use super::*;

impl AppState {
    pub fn settle_stopped_actor(&mut self, effects: &mut Vec<Effect>) -> bool {
        let responses_changed = self.user_response_actor_stopped();
        let mut actions = Vec::new();
        for (id, pending) in &self.prompt_submissions.by_thread {
            actions.push(Action::PromptFailed {
                thread_id: ThreadId::new(id.clone()),
                request_id: pending.request_id,
                error: "App Server actor stopped; prompt delivery outcome unknown".into(),
            });
        }
        for id in &self.prompt_submissions.awaiting_history {
            actions.push(Action::ConversationFailed {
                thread_id: ThreadId::new(id.clone()),
                error: "App Server actor stopped; prompt history unavailable".into(),
            });
        }
        let prompts_changed = !actions.is_empty();
        for action in actions {
            effects.extend(reduce(self, action));
        }
        responses_changed || prompts_changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_exit_settles_unknown_prompt_without_retry_or_draft_loss() {
        let mut app = AppState::new(vec![]);
        let thread = ThreadId::new("pending");
        let ticket = app.prompt_submissions.begin(&thread).expect("ticket");
        app.thread_ui.entry(thread.0.clone()).or_default().draft = "keep draft".into();
        let mut effects = vec![];
        assert!(app.settle_stopped_actor(&mut effects));
        assert!(app.prompt_submissions.by_thread.is_empty());
        assert_eq!(app.thread_ui.get(&thread.0).unwrap().draft, "keep draft");
        assert!(app.conversations.get(&thread.0).unwrap().error.is_some());
        assert!(!app.settle_stopped_actor(&mut effects));
        let next = app.prompt_submissions.begin(&thread).expect("capacity released");
        assert_ne!(next, ticket);
    }
    #[test]
    fn actor_exit_closes_acknowledged_but_unread_history() {
        let mut app = AppState::new(vec![]);
        let thread = ThreadId::new("accepted");
        let pending_history = &mut app.prompt_submissions.awaiting_history;
        pending_history.insert(thread.0.clone());
        let mut effects = vec![];
        assert!(app.settle_stopped_actor(&mut effects));
        assert!(app.prompt_submissions.awaiting_history.is_empty());
        assert!(app.conversations.get(&thread.0).unwrap().error.is_some());
    }
}
