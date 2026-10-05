//! Freeze question identity and privacy for the lifetime of one answer editor.
use super::{AppState, Effect, InputMode, local_text};
use crate::conversation::{
    InteractiveRequest, InteractiveRequestKind, InteractiveResolution, RpcRequestId,
    UserInputQuestion,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub(super) struct UserInputEditor {
    request: InteractiveRequest,
    question_index: usize,
    answers: BTreeMap<String, Vec<String>>,
}

impl AppState {
    pub fn current_user_input_question(&self) -> Option<&UserInputQuestion> {
        let editor = self.user_input_editor.as_ref()?;
        let InteractiveRequestKind::UserInput { questions } = &editor.request.kind else {
            return None;
        };
        questions.get(editor.question_index)
    }

    fn user_input_is_current(&self) -> bool {
        self.user_input_editor.as_ref().is_some_and(|editor| {
            self.current_thread_id() == Some(&editor.request.thread_id)
                && self
                    .thread_index_by_id
                    .contains_key(&editor.request.thread_id.0)
                && self
                    .pending_requests
                    .iter()
                    .any(|request| request == &editor.request)
        })
    }

    pub(super) fn notice_changed_user_input(&mut self) {
        if self.input_mode == InputMode::UserInput && !self.user_input_is_current() {
            self.mutation_notice = Some(local_text(self.language,
                "input request changed or disappeared; original answers retained, cancel and reopen before replying",
                "输入请求已改变或失效；已保留原回答，请取消并重新打开后作答").into());
        }
    }

    pub(super) fn begin_user_input(&mut self) {
        // Repeated begin is not permission to discard an unfinished answer.
        if self.input_mode == InputMode::UserInput && self.user_input_editor.is_some() {
            self.notice_changed_user_input();
            return;
        }
        let Some(request) = self.current_pending_request().cloned() else {
            return;
        };
        if !matches!(request.kind, InteractiveRequestKind::UserInput { .. }) {
            return;
        }
        if request.validate_user_input().is_err()
            || !self.thread_index_by_id.contains_key(&request.thread_id.0)
        {
            self.mutation_notice = Some(
                local_text(
                    self.language,
                    "input request is invalid; no answer editor opened",
                    "输入请求无效；未打开回答编辑器",
                )
                .into(),
            );
            return;
        }
        self.user_input_editor = Some(UserInputEditor {
            request,
            question_index: 0,
            answers: BTreeMap::new(),
        });
        self.input_buffer.clear();
        self.input_mode = InputMode::UserInput;
    }

    pub(super) fn commit_user_input(&mut self) -> Vec<Effect> {
        if !self.user_input_is_current() {
            self.notice_changed_user_input();
            return vec![];
        }
        let Some(question) = self.current_user_input_question() else {
            return vec![];
        };
        let input = self.input_buffer.trim();
        if input.is_empty() {
            return vec![];
        }
        let answers = if question.options.is_empty() {
            vec![input.to_string()]
        } else {
            input
                .split(',')
                .map(str::trim)
                .filter(|answer| !answer.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        };
        if answers.is_empty() {
            return vec![];
        }
        let question_id = question.id.clone();
        let Some(editor) = self.user_input_editor.as_mut() else {
            return vec![];
        };
        let InteractiveRequestKind::UserInput { questions } = &editor.request.kind else {
            return vec![];
        };
        editor.answers.insert(question_id, answers);
        self.input_buffer.clear();
        if editor.question_index + 1 < questions.len() {
            editor.question_index += 1;
            return vec![];
        }
        let Some(editor) = self.user_input_editor.take() else {
            return vec![];
        };
        self.input_mode = InputMode::Normal;
        vec![Effect::ResolveInteractive {
            request_id: editor.request.request_id,
            resolution: InteractiveResolution::UserInput(editor.answers),
        }]
    }

    pub(super) fn resolve_user_input_editor(&mut self, request_id: &RpcRequestId) {
        if self
            .user_input_editor
            .as_ref()
            .is_some_and(|editor| &editor.request.request_id == request_id)
        {
            self.clear_user_input_editor();
        }
    }

    pub(super) fn clear_user_input_editor(&mut self) {
        self.user_input_editor = None;
        // A late resolution must never erase another kind of editor.
        if self.input_mode == InputMode::UserInput {
            self.input_buffer.clear();
            self.input_mode = InputMode::Normal;
        }
    }
}
