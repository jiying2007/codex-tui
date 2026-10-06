//! Bounded, transient attempts; explicit editor close never cancels accepted work.
use super::{Action, AppState, Effect, InputMode, local_text};
use crate::conversation::{InteractiveRequest, InteractiveResolution, RpcRequestId};
use crate::user_response::{UserResponseOutcome, UserResponseSubmission};
use std::collections::BTreeMap;
const LIMIT: usize = 64;
#[derive(Clone, Debug, Default)]
pub(super) struct UserResponses {
    next_ticket: u64,
    attempts: BTreeMap<RpcRequestId, Attempt>,
}
#[derive(Clone, Debug)]
struct Attempt {
    submission: UserResponseSubmission,
    revision: Option<u64>,
    unknown: bool,
}
impl AppState {
    pub(super) fn observe_user_response_edit(&mut self, action: &Action) {
        if self.input_mode == InputMode::UserInput
            && matches!(
                action,
                Action::InputChar(_) | Action::InputText(_) | Action::InputBackspace
            )
            && let Some(editor) = self.user_input_editor.as_mut()
        {
            editor.revision = editor.revision.saturating_add(1);
        }
    }
    pub(super) fn user_response_blocked(&mut self, request_id: &RpcRequestId) -> bool {
        if self.user_responses.attempts.contains_key(request_id) {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "response is pending or delivery is unconfirmed; no duplicate response sent",
                    "响应正在发送或送达尚未确认；未重复发送",
                )
                .into(),
            );
            true
        } else {
            false
        }
    }
    pub(super) fn begin_user_response(&mut self) -> Vec<Effect> {
        let Some(editor) = self.user_input_editor.as_ref() else {
            return vec![];
        };
        self.begin_interactive_response(
            editor.request.clone(),
            InteractiveResolution::UserInput(editor.answers.clone()),
        )
    }
    pub(super) fn begin_interactive_response(
        &mut self,
        request: InteractiveRequest,
        resolution: InteractiveResolution,
    ) -> Vec<Effect> {
        if !self.thread_index_by_id.contains_key(&request.thread_id.0)
            || !self.pending_requests.contains(&request)
            || request.thread_id.0.trim().is_empty()
            || request.turn_id.trim().is_empty()
            || request.item_id.trim().is_empty()
        {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "interactive request is no longer current; no response sent",
                    "交互请求已失效；未发送响应",
                )
                .into(),
            );
            return vec![];
        }
        let id = request.request_id.clone();
        if self.user_response_blocked(&id) {
            return vec![];
        }
        let Some(ticket) = self.user_responses.next_ticket.checked_add(1) else {
            self.mutation_notice = Some("response identity exhausted; input retained".into());
            return vec![];
        };
        if self.user_responses.attempts.len() >= LIMIT {
            self.mutation_notice = Some(local_text(self.language,
                "too many unconfirmed responses; input retained, check the backend before restarting",
                "未确认响应过多；已保留输入，请核对后端状态后再重启").into());
            return vec![];
        }
        let revision = self
            .user_input_editor
            .as_mut()
            .filter(|editor| editor.request == request)
            .map(|editor| {
                editor.submission = Some(ticket);
                editor.revision
            });
        let submission = UserResponseSubmission {
            ticket,
            request,
            resolution: resolution.clone(),
        };
        let effect = Effect::ResolveInteractive {
            request_id: id.clone(),
            resolution,
        };
        self.user_responses.next_ticket = ticket;
        self.user_responses.attempts.insert(
            id,
            Attempt {
                submission,
                revision,
                unknown: false,
            },
        );
        self.mutation_notice = Some(local_text(self.language,
            "sending response; input retained until matching transport receipt (not server acknowledgement)",
            "正在发送响应；匹配传输回执前保留内容（不等于服务端确认）").into());
        vec![effect]
    }
    pub fn user_response_submission(
        &self,
        id: &RpcRequestId,
        resolution: &InteractiveResolution,
    ) -> Option<UserResponseSubmission> {
        self.user_responses
            .attempts
            .get(id)
            .filter(|a| !a.unknown && &a.submission.resolution == resolution)
            .map(|a| a.submission.clone())
    }
    pub fn finish_user_response(&mut self, ticket: u64, outcome: UserResponseOutcome) {
        let Some(id) = self
            .user_responses
            .attempts
            .iter()
            .find(|(_, a)| a.submission.ticket == ticket)
            .map(|(id, _)| id.clone())
        else {
            return;
        };
        let mut attempt = self
            .user_responses
            .attempts
            .remove(&id)
            .expect("matching attempt");
        // A delayed duplicate refusal cannot turn an uncertain attempt into a safe retry.
        if attempt.unknown && matches!(outcome, UserResponseOutcome::NotSent(_)) {
            self.user_responses.attempts.insert(id, attempt);
            return;
        }
        match outcome {
            UserResponseOutcome::NotSent(error) => {
                if let Some(editor) = self.user_input_editor.as_mut()
                    && editor.submission == Some(ticket)
                {
                    editor.submission = None;
                }
                self.mutation_notice = Some(format!(
                    "{}: {error}",
                    local_text(
                        self.language,
                        "response not sent; input retained unless explicitly closed",
                        "响应未发送；未主动关闭的输入已保留"
                    )
                ));
            }
            UserResponseOutcome::Unknown(error) => {
                attempt.unknown = true;
                self.user_responses.attempts.insert(id, attempt);
                self.mutation_notice = Some(format!(
                    "{}: {error}",
                    local_text(
                        self.language,
                        "response delivery unconfirmed; input retained, no automatic retry; check backend state",
                        "响应送达未确认；已保留输入，不自动重试，请核对后端状态"
                    )
                ));
            }
            UserResponseOutcome::Written => {
                self.pending_requests
                    .retain(|r| r != &attempt.submission.request);
                self.rebuild_pending_request_threads();
                let unchanged = self.user_input_editor.as_ref().is_some_and(|e| {
                    e.submission == Some(ticket)
                        && e.request == attempt.submission.request
                        && Some(e.revision) == attempt.revision
                        && e.revision != u64::MAX
                });
                if unchanged {
                    self.clear_user_input_editor();
                }
                self.mutation_notice = Some(local_text(self.language,
                    "response written to transport; server acknowledgement not established; later input retained",
                    "响应已写入传输层；不代表服务端已确认，后续编辑仍保留").into());
                super::ensure_selection_visible(self);
            }
        }
    }
    pub fn user_response_actor_stopped(&mut self) -> bool {
        let tickets = self
            .user_responses
            .attempts
            .values()
            .filter(|a| !a.unknown)
            .map(|a| a.submission.ticket)
            .collect::<Vec<_>>();
        for ticket in &tickets {
            self.finish_user_response(
                *ticket,
                UserResponseOutcome::Unknown("App Server response actor stopped".into()),
            );
        }
        !tickets.is_empty()
    }
    pub(super) fn forget_user_response(&mut self, id: &RpcRequestId) {
        self.user_responses.attempts.remove(id);
    }
}

#[cfg(test)]
mod tests;
