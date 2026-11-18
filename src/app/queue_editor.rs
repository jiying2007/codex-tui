//! Queue drafts are tied to the observed upstream item, never a moving row index.
use super::{AppState, Effect, InputMode, local_text};
use crate::{
    domain::ThreadId,
    operation::now_unix_ms,
    thread_queue::{QueuedSubmission, ThreadQueueMutation, ThreadQueueSnapshot},
};

#[derive(Clone, Debug)]
pub(super) struct QueueEditor {
    pub(super) thread_id: ThreadId,
    original: Option<QueuedSubmission>,
    pub(super) pending: bool,
}
impl AppState {
    pub(super) fn begin_queue_input(&mut self, edit: bool) {
        if !self.thread_queue_open || self.queue_mutation_pending() {
            return;
        }
        let Some(thread_id) = self
            .current_thread_id()
            .filter(|id| self.thread_by_id(id).is_some())
            .cloned()
        else {
            return;
        };
        let original = if edit {
            let selected = self
                .ready_queue_snapshot()
                .filter(|snapshot| snapshot.thread_id == thread_id)
                .and_then(|snapshot| snapshot.submissions.get(self.thread_queue_selected));
            let Some(item) = selected.filter(|item| item.editable_text.is_some()) else {
                self.mutation_notice = Some(
                    local_text(
                        self.language,
                        "this queued item is unavailable or cannot be text-edited safely",
                        "该队列项不可用，或无法安全地按文本编辑",
                    )
                    .into(),
                );
                return;
            };
            Some(item.clone())
        } else {
            None
        };
        self.input_buffer = original
            .as_ref()
            .and_then(|item| item.editable_text.clone())
            .unwrap_or_default();
        self.thread_queue_editor = Some(QueueEditor {
            thread_id,
            original,
            pending: false,
        });
        self.input_mode = if edit {
            InputMode::ThreadQueueEdit
        } else {
            InputMode::ThreadQueueAdd
        };
        self.thread_queue_error = None;
    }
    pub(super) fn commit_queue_input(&mut self) -> Vec<Effect> {
        if self.thread_queue_loading {
            return vec![];
        }
        let text = self.input_buffer.trim().to_string();
        if text.is_empty() {
            return vec![];
        }
        let valid = self.thread_queue_editor.as_ref().is_some_and(|editor| {
            self.thread_queue_open
                && self.current_thread_id() == Some(&editor.thread_id)
                && self.thread_by_id(&editor.thread_id).is_some()
                && match (&editor.original, self.input_mode) {
                    (None, InputMode::ThreadQueueAdd) => true,
                    (Some(original), InputMode::ThreadQueueEdit) => self
                        .ready_queue_snapshot()
                        .filter(|snapshot| snapshot.thread_id == editor.thread_id)
                        .and_then(|snapshot| {
                            snapshot
                                .submissions
                                .iter()
                                .find(|item| item.id == original.id)
                        })
                        .is_some_and(|current| current == original),
                    _ => false,
                }
        });
        if !valid {
            self.thread_queue_error = Some(
                local_text(
                    self.language,
                    "queue target changed or disappeared; draft retained, no update sent; cancel and reopen to review",
                    "队列目标已变化或消失；已保留草稿，未发送更新，请取消并重新打开确认",
                )
                .into(),
            );
            return vec![];
        }
        let Some(editor) = self.thread_queue_editor.as_ref() else {
            return vec![];
        };
        let mutation = match &editor.original {
            None => ThreadQueueMutation::add(editor.thread_id.clone(), text, now_unix_ms()),
            Some(original) => {
                ThreadQueueMutation::update(editor.thread_id.clone(), original.id.clone(), text)
            }
        };
        match mutation {
            Ok(mutation) => {
                self.thread_queue_editor
                    .as_mut()
                    .expect("validated editor")
                    .pending = true;
                self.thread_queue_loading = true;
                self.thread_queue_error = None;
                vec![Effect::MutateThreadQueue(mutation)]
            }
            Err(error) => {
                self.thread_queue_error = Some(error.to_string());
                vec![]
            }
        }
    }
}
