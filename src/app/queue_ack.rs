//! Keep the editor and its target until upstream accepts a queue mutation.
use super::*;

impl AppState {
    pub(super) fn queue_mutation_pending(&self) -> bool {
        self.thread_queue_editor.as_ref().is_some_and(|editor| editor.pending)
    }

    pub(super) fn queue_ack_loaded(&mut self, id: &ThreadId) {
        if self.thread_queue_open && !self.thread_queue_loading && self.thread_queue_error.is_none()
            && self.thread_queue_editor.as_ref().is_some_and(|editor| editor.pending && &editor.thread_id == id)
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
