//! One bounded original item, not an executable mutation aimed at a moving row.
use super::{AppState, Effect, local_text};
use crate::{
    domain::ThreadId,
    thread_queue::{QueuedSubmission, ThreadQueueMutation, ThreadQueueSnapshot},
};

#[derive(Clone, Debug)]
pub struct QueueConfirmation {
    thread_id: ThreadId,
    original: QueuedSubmission,
    start: bool,
}
impl QueueConfirmation {
    pub const fn label(&self) -> &'static str {
        if self.start { "start" } else { "delete" }
    }
    pub fn item_id(&self) -> &str {
        &self.original.id
    }
    pub fn summary(&self) -> &str {
        &self.original.summary
    }
}

impl AppState {
    pub(super) fn ready_queue_snapshot(&self) -> Option<&ThreadQueueSnapshot> {
        let snapshot = self.thread_queue_snapshot.as_ref()?;
        (self.thread_queue_open
            && !self.thread_queue_loading
            && self.thread_queue_error.is_none()
            && self.current_thread_id() == Some(&snapshot.thread_id)
            && self.thread_by_id(&snapshot.thread_id).is_some()
            && snapshot.validate_complete().is_ok())
        .then_some(snapshot)
    }

    pub(super) fn begin_queue_confirmation(&mut self, start: bool) {
        self.pending_queue_confirmation = self.ready_queue_snapshot().and_then(|snapshot| {
            snapshot
                .submissions
                .get(self.thread_queue_selected)
                .map(|item| QueueConfirmation {
                    thread_id: snapshot.thread_id.clone(),
                    original: item.clone(),
                    start,
                })
        });
        if self.pending_queue_confirmation.is_none() {
            self.refuse_queue_mutation();
        }
    }

    pub(super) fn confirm_queue_operation(&mut self) -> Vec<Effect> {
        let Some(pending) = self.pending_queue_confirmation.take() else {
            return vec![];
        };
        let valid = self.ready_queue_snapshot().is_some_and(|snapshot| {
            snapshot.thread_id == pending.thread_id
                && snapshot
                    .submissions
                    .iter()
                    .find(|item| item.id == pending.original.id)
                    .is_some_and(|current| {
                        current.client_user_message_id == pending.original.client_user_message_id
                            && current.input == pending.original.input
                    })
        });
        if !valid {
            self.refuse_queue_mutation();
            return vec![];
        }
        let mutation = if pending.start {
            ThreadQueueMutation::start(pending.thread_id, pending.original.id)
        } else {
            ThreadQueueMutation::delete(pending.thread_id, pending.original.id)
        };
        match mutation {
            Ok(mutation) => {
                self.thread_queue_loading = true;
                vec![Effect::MutateThreadQueue(mutation)]
            }
            Err(error) => {
                self.thread_queue_error = Some(error.to_string());
                vec![]
            }
        }
    }

    pub(super) fn refuse_queue_mutation(&mut self) {
        self.thread_queue_error = Some(local_text(
            self.language,
            "queue target changed, disappeared or is not ready; nothing sent; refresh and confirm again",
            "队列目标已变化、消失或尚未就绪；未发送请求，请刷新后重新确认",
        ).into());
    }
}
