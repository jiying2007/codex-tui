//! Keep the editor and its target until upstream accepts a queue mutation.
use super::*;

impl AppState {
    pub(super) fn install_thread_queue(&mut self, snapshot: ThreadQueueSnapshot) {
        if !self.thread_queue_open
            || self.current_thread_id() != Some(&snapshot.thread_id)
            || self.thread_by_id(&snapshot.thread_id).is_none()
        {
            return;
        }
        if let Err(error) = snapshot.validate_complete() {
            self.thread_queue_loading = false;
            self.thread_queue_error = Some(error.to_string());
            return;
        }
        let selected = self
            .thread_queue_snapshot
            .as_ref()
            .filter(|old| old.thread_id == snapshot.thread_id)
            .and_then(|old| old.submissions.get(self.thread_queue_selected))
            .map(|item| item.id.as_str());
        self.thread_queue_selected = selected
            .and_then(|id| snapshot.submissions.iter().position(|item| item.id == id))
            .unwrap_or_else(|| {
                self.thread_queue_selected
                    .min(snapshot.submissions.len().saturating_sub(1))
            });
        self.thread_queue_snapshot = Some(snapshot);
        self.thread_queue_loading = false;
        self.thread_queue_error = None;
    }
    pub(super) fn queue_mutation_pending(&self) -> bool {
        self.thread_queue_editor
            .as_ref()
            .is_some_and(|editor| editor.pending)
    }

    pub(super) fn queue_ack_loaded(&mut self, id: &ThreadId) {
        if self.thread_queue_open
            && !self.thread_queue_loading
            && self.thread_queue_error.is_none()
            && self
                .thread_queue_editor
                .as_ref()
                .is_some_and(|editor| editor.pending && &editor.thread_id == id)
        {
            self.thread_queue_editor = None;
            self.input_mode = InputMode::Normal;
            self.input_buffer.clear();
        }
    }

    pub(super) fn queue_ack_failed(&mut self, id: &ThreadId) {
        if let Some(editor) = self.thread_queue_editor.as_mut()
            && &editor.thread_id == id
        {
            editor.pending = false;
            // The original draft stays in input_buffer so the user can edit or retry.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    #[test]
    fn rejected_queue_submission_keeps_draft_and_prevents_duplicate_inflight_send() {
        let threads = FakeBackend::seeded().snapshot().threads;
        let thread_id = threads[0].id.clone();
        let mut app = AppState::new(threads);
        app.view = View::Thread(thread_id.clone());
        app.thread_queue_open = true;
        app.thread_queue_snapshot = Some(ThreadQueueSnapshot {
            thread_id: thread_id.clone(),
            submissions: vec![],
            next_cursor: None,
        });
        reduce(&mut app, Action::BeginThreadQueueAdd);
        app.input_buffer = "recoverable queue draft".into();
        let first = reduce(&mut app, Action::CommitInput);
        assert!(matches!(first.as_slice(), [Effect::MutateThreadQueue(_)]));
        assert!(app.queue_mutation_pending());
        assert_eq!(app.input_buffer, "recoverable queue draft");
        assert!(reduce(&mut app, Action::CommitInput).is_empty());
        assert!(reduce(&mut app, Action::CloseThreadQueue).is_empty());
        assert!(app.thread_queue_open);

        reduce(
            &mut app,
            Action::ThreadQueueFailed {
                thread_id: thread_id.clone(),
                error: "upstream rejected".into(),
            },
        );
        // Only a typed mutation outcome releases a pending draft.
        assert!(app.queue_mutation_pending());
        reduce(
            &mut app,
            Action::QueueMutationOutcome {
                thread_id: thread_id.clone(),
                committed: false,
            },
        );
        assert!(!app.queue_mutation_pending());
        assert_eq!(app.input_mode, InputMode::ThreadQueueAdd);
        assert_eq!(app.input_buffer, "recoverable queue draft");
        assert!(matches!(
            reduce(&mut app, Action::CommitInput).as_slice(),
            [Effect::MutateThreadQueue(_)]
        ));
        reduce(
            &mut app,
            Action::ThreadQueueLoaded(ThreadQueueSnapshot {
                thread_id: thread_id.clone(),
                submissions: vec![],
                next_cursor: None,
            }),
        );
        assert_eq!(app.input_mode, InputMode::ThreadQueueAdd);
        reduce(
            &mut app,
            Action::QueueMutationOutcome {
                thread_id,
                committed: true,
            },
        );
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.input_buffer.is_empty());
    }
}
