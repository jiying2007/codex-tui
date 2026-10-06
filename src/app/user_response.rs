//! Bounded, transient attempts; explicit editor close never cancels accepted work.
use super::{Action, AppState, Effect, InputMode, local_text};
use crate::conversation::{InteractiveResolution, RpcRequestId};
use crate::user_response::{UserResponseAnswers, UserResponseOutcome, UserResponseSubmission};
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
    revision: u64,
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
                    "answer is pending or delivery is unconfirmed; no duplicate response sent",
                    "回答正在发送或送达尚未确认；未重复发送",
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
        let id = editor.request.request_id.clone();
        if self.user_response_blocked(&id) {
            return vec![];
        }
        let Some(ticket) = self.user_responses.next_ticket.checked_add(1) else {
            self.mutation_notice = Some("response identity exhausted; answer retained".into());
            return vec![];
        };
        if self.user_responses.attempts.len() >= LIMIT {
            self.mutation_notice = Some(local_text(self.language,
                "too many unconfirmed answers; answer retained, check the backend before restarting",
                "未确认回答过多；已保留回答，请核对后端状态后再重启").into());
            return vec![];
        }
        let editor = self.user_input_editor.as_mut().expect("unchanged editor");
        editor.submission = Some(ticket);
        let submission = UserResponseSubmission {
            ticket,
            request: editor.request.clone(),
            answers: editor.answers.clone(),
        };
        let effect = Effect::ResolveInteractive {
            request_id: id.clone(),
            resolution: InteractiveResolution::UserInput(editor.answers.clone()),
        };
        self.user_responses.next_ticket = ticket;
        self.user_responses.attempts.insert(
            id,
            Attempt {
                submission,
                revision: editor.revision,
                unknown: false,
            },
        );
        self.mutation_notice = Some(local_text(self.language,
            "sending answer; retained until matching transport receipt (not server acknowledgement)",
            "正在发送回答；匹配传输回执前保留内容（不等于服务端确认）").into());
        vec![effect]
    }
    pub fn user_response_submission(
        &self,
        id: &RpcRequestId,
        answers: &UserResponseAnswers,
    ) -> Option<UserResponseSubmission> {
        self.user_responses
            .attempts
            .get(id)
            .filter(|a| !a.unknown && &a.submission.answers == answers)
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
                        "answer not sent; input retained unless explicitly closed",
                        "回答未发送；未主动关闭的输入已保留"
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
                        "answer delivery unconfirmed; input retained, no automatic retry; check backend state",
                        "回答送达未确认；已保留输入，不自动重试，请核对后端状态"
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
                        && e.revision == attempt.revision
                        && e.revision != u64::MAX
                });
                if unchanged {
                    self.clear_user_input_editor();
                }
                self.mutation_notice = Some(local_text(self.language,
                    "answer written to transport; server acknowledgement not established; later input retained",
                    "回答已写入传输层；不代表服务端已确认，后续编辑仍保留").into());
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
mod tests {
    use super::*;
    use crate::{
        app::View,
        backend::{CodexBackend, FakeBackend},
        conversation::{InteractiveRequest, InteractiveRequestKind, UserInputQuestion},
    };
    fn editor() -> AppState {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        let request = InteractiveRequest {
            request_id: RpcRequestId::String("r".into()),
            thread_id: app.threads[0].id.clone(),
            turn_id: "t".into(),
            item_id: "i".into(),
            kind: InteractiveRequestKind::UserInput {
                questions: vec![UserInputQuestion {
                    id: "q".into(),
                    header: "H".into(),
                    question: "Q".into(),
                    is_secret: false,
                    options: vec![],
                }],
            },
        };
        app.view = View::Thread(request.thread_id.clone());
        crate::app::reduce(&mut app, Action::InteractiveRequested(request));
        crate::app::reduce(&mut app, Action::BeginUserInput);
        crate::app::reduce(&mut app, Action::InputText("retained".into()));
        app
    }
    #[test]
    fn ticket_exhaustion_does_not_wrap_or_drop_answer() {
        let mut app = editor();
        app.user_responses.next_ticket = u64::MAX;
        assert!(crate::app::reduce(&mut app, Action::CommitInput).is_empty());
        assert_eq!(app.input_buffer, "retained");
        assert!(app.user_responses.attempts.is_empty());
    }
    #[test]
    fn ledger_capacity_never_evicts_uncertain_entries() {
        let mut app = editor();
        crate::app::reduce(&mut app, Action::CommitInput);
        let mut attempt = app.user_responses.attempts.values().next().unwrap().clone();
        attempt.unknown = true;
        app.user_responses.attempts.clear();
        for i in 0..LIMIT {
            app.user_responses.attempts.insert(
                RpcRequestId::String(format!("uncertain-{i}")),
                attempt.clone(),
            );
        }
        assert!(crate::app::reduce(&mut app, Action::CommitInput).is_empty());
        assert_eq!(app.user_responses.attempts.len(), LIMIT);
        assert_eq!(app.input_buffer, "retained");
    }
}
