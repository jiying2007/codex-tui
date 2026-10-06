"""Apply the reviewable response-delivery candidate to one exact baseline.
No network calls, credentials, ref writes or publication are performed here.
"""
from pathlib import Path
import json
import sys

def put(name, text):
    path = Path(name)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")

def replace(name, old, new):
    path = Path(name)
    text = path.read_text(encoding="utf-8")
    if text.count(old) != 1:
        raise RuntimeError("non-unique patch anchor: %s: %r" % (name, old[:90]))
    path.write_text(text.replace(old, new), encoding="utf-8")

put('tests/user_response_retention.rs', r'''use codex_tui::{
    app::{Action, AppState, InputMode, View, reduce},
    backend::{CodexBackend, FakeBackend},
    conversation::{InteractiveRequest, InteractiveRequestKind, InteractiveResolution, RpcRequestId, UserInputQuestion},
    ui,
};
use ratatui::{Terminal, backend::TestBackend};
fn setup() -> (AppState, InteractiveRequest) {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    app.host_local_only = false;
    let request = InteractiveRequest {
        request_id: RpcRequestId::String("response-1".into()),
        thread_id: app.threads[0].id.clone(), turn_id: "turn".into(), item_id: "item".into(),
        kind: InteractiveRequestKind::UserInput { questions: vec![UserInputQuestion {
            id: "q".into(), header: "Header".into(), question: "Private answer?".into(),
            is_secret: true, options: vec![],
        }] },
    };
    app.view = View::Thread(request.thread_id.clone());
    reduce(&mut app, Action::InteractiveRequested(request.clone()));
    reduce(&mut app, Action::BeginUserInput);
    reduce(&mut app, Action::InputText("SYNTHETIC_PRIVATE_84".into()));
    (app, request)
}
#[test]
fn final_answer_is_retained_before_any_backend_admission() {
    let (mut app, _) = setup();
    assert_eq!(reduce(&mut app, Action::CommitInput).len(), 1);
    assert_eq!(app.input_mode, InputMode::UserInput);
    assert_eq!(app.input_buffer, "SYNTHETIC_PRIVATE_84");
}
#[test]
fn pending_answer_keeps_original_secret_screen() {
    let (mut app, _) = setup();
    reduce(&mut app, Action::CommitInput);
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal.draw(|f| ui::render(f, &app)).unwrap();
    let screen: String = terminal.backend().buffer().content.iter().map(|c| c.symbol()).collect();
    assert!(screen.contains("********"), "pending answer editor disappeared before receipt");
    assert!(!screen.contains("SYNTHETIC_PRIVATE_84"));
}
#[test]
fn explicit_close_does_not_authorize_duplicate_pending_answer() {
    let (mut app, _) = setup();
    reduce(&mut app, Action::CommitInput);
    reduce(&mut app, Action::CancelInput);
    reduce(&mut app, Action::BeginUserInput);
    reduce(&mut app, Action::InputText("second".into()));
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
}
#[test]
fn pending_answer_cannot_race_a_second_cancel_response() {
    let (mut app, _) = setup();
    reduce(&mut app, Action::CommitInput);
    assert!(reduce(&mut app, Action::ResolvePending(InteractiveResolution::Cancel)).is_empty());
}
#[test]
fn repeated_enter_without_a_receipt_never_submits_twice() {
    let (mut app, _) = setup();
    assert_eq!(reduce(&mut app, Action::CommitInput).len(), 1);
    assert!(reduce(&mut app, Action::CommitInput).is_empty());
}
''')
if sys.argv[1] == 'tests':
    raise SystemExit(0)
if sys.argv[1] != 'fix':
    raise SystemExit('expected tests or fix')

put('src/user_response.rs', r'''//! Local response bookkeeping. A completed transport write is not server acknowledgement.
use crate::conversation::{InteractiveRequest, RpcRequestId};
use std::collections::BTreeMap;
use std::fmt;
#[derive(Clone, PartialEq, Eq)]
pub struct UserResponseSubmission {
    pub ticket: u64,
    pub request: InteractiveRequest,
    pub answers: BTreeMap<String, Vec<String>>,
}
impl fmt::Debug for UserResponseSubmission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserResponseSubmission")
            .field("ticket", &self.ticket)
            .field("request_id", &self.request.request_id)
            .field("answers", &"[redacted]")
            .finish()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserResponseOutcome {
    /// No response bytes were submitted to the transport.
    NotSent(String),
    /// The local transport completed the write; no server acknowledgement is implied.
    Written,
    /// A write was attempted, or the actor stopped before a conclusive receipt.
    Unknown(String),
}
pub type UserResponseAnswers = BTreeMap<String, Vec<String>>;
pub type UserResponseId = RpcRequestId;
''')
replace('src/lib.rs', 'pub mod conversation;', 'pub mod conversation;\npub mod user_response;')
put('src/app/user_response.rs', r'''//! Bounded, transient attempts; explicit editor close never cancels accepted work.
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
            && matches!(action, Action::InputChar(_) | Action::InputText(_) | Action::InputBackspace)
            && let Some(editor) = self.user_input_editor.as_mut()
        {
            editor.revision = editor.revision.saturating_add(1);
        }
    }
    pub(super) fn user_response_blocked(&mut self, request_id: &RpcRequestId) -> bool {
        if self.user_responses.attempts.contains_key(request_id) {
            self.mutation_notice = Some(local_text(self.language,
                "answer is pending or delivery is unconfirmed; no duplicate response sent",
                "回答正在发送或送达尚未确认；未重复发送").into());
            true
        } else { false }
    }
    pub(super) fn begin_user_response(&mut self) -> Vec<Effect> {
        let Some(editor) = self.user_input_editor.as_ref() else { return vec![]; };
        let id = editor.request.request_id.clone();
        if self.user_response_blocked(&id) { return vec![]; }
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
        let submission = UserResponseSubmission { ticket, request: editor.request.clone(), answers: editor.answers.clone() };
        let effect = Effect::ResolveInteractive {
            request_id: id.clone(), resolution: InteractiveResolution::UserInput(editor.answers.clone()),
        };
        self.user_responses.next_ticket = ticket;
        self.user_responses.attempts.insert(id, Attempt { submission, revision: editor.revision, unknown: false });
        self.mutation_notice = Some(local_text(self.language,
            "sending answer; retained until matching transport receipt (not server acknowledgement)",
            "正在发送回答；匹配传输回执前保留内容（不等于服务端确认）").into());
        vec![effect]
    }
    pub fn user_response_submission(&self, id: &RpcRequestId, answers: &UserResponseAnswers) -> Option<UserResponseSubmission> {
        self.user_responses.attempts.get(id)
            .filter(|a| !a.unknown && &a.submission.answers == answers)
            .map(|a| a.submission.clone())
    }
    pub fn finish_user_response(&mut self, ticket: u64, outcome: UserResponseOutcome) {
        let Some(id) = self.user_responses.attempts.iter()
            .find(|(_, a)| a.submission.ticket == ticket).map(|(id, _)| id.clone()) else { return; };
        let mut attempt = self.user_responses.attempts.remove(&id).expect("matching attempt");
        match outcome {
            UserResponseOutcome::NotSent(error) => {
                if let Some(editor) = self.user_input_editor.as_mut()
                    && editor.submission == Some(ticket) { editor.submission = None; }
                self.mutation_notice = Some(format!("{}: {error}", local_text(self.language,
                    "answer not sent; input retained unless explicitly closed",
                    "回答未发送；未主动关闭的输入已保留")));
            }
            UserResponseOutcome::Unknown(error) => {
                attempt.unknown = true;
                self.user_responses.attempts.insert(id, attempt);
                self.mutation_notice = Some(format!("{}: {error}", local_text(self.language,
                    "answer delivery unconfirmed; input retained, no automatic retry; check backend state",
                    "回答送达未确认；已保留输入，不自动重试，请核对后端状态")));
            }
            UserResponseOutcome::Written => {
                self.pending_requests.retain(|r| r != &attempt.submission.request);
                self.rebuild_pending_request_threads();
                let unchanged = self.user_input_editor.as_ref().is_some_and(|e|
                    e.submission == Some(ticket) && e.request == attempt.submission.request
                    && e.revision == attempt.revision && e.revision != u64::MAX);
                if unchanged { self.clear_user_input_editor(); }
                self.mutation_notice = Some(local_text(self.language,
                    "answer written to transport; server acknowledgement not established; later input retained",
                    "回答已写入传输层；不代表服务端已确认，后续编辑仍保留").into());
                super::ensure_selection_visible(self);
            }
        }
    }
    pub fn user_response_actor_stopped(&mut self) -> bool {
        let tickets = self.user_responses.attempts.values().filter(|a| !a.unknown)
            .map(|a| a.submission.ticket).collect::<Vec<_>>();
        for ticket in &tickets {
            self.finish_user_response(*ticket, UserResponseOutcome::Unknown("App Server response actor stopped".into()));
        }
        !tickets.is_empty()
    }
    pub(super) fn forget_user_response(&mut self, id: &RpcRequestId) {
        self.user_responses.attempts.remove(id);
    }
}
''')
replace('src/app.rs', 'mod user_input;', 'mod user_input;\nmod user_response;')
replace('src/app.rs', '    user_input_editor: Option<user_input::UserInputEditor>,', '    user_input_editor: Option<user_input::UserInputEditor>,\n    user_responses: user_response::UserResponses,')
replace('src/app.rs', '            user_input_editor: None,', '            user_input_editor: None,\n            user_responses: Default::default(),')
replace('src/app.rs', 'pub fn reduce(state: &mut AppState, action: Action) -> Vec<Effect> {', 'pub fn reduce(state: &mut AppState, action: Action) -> Vec<Effect> {\n    state.observe_user_response_edit(&action);')
replace('src/app.rs', '        Action::InteractiveResolved { request_id } => {', '        Action::InteractiveResolved { request_id } => {\n            state.forget_user_response(&request_id);')
replace('src/app.rs', '        Action::ResolvePending(resolution) => {\n            if let Some(request) = state.current_pending_request().cloned() {', '        Action::ResolvePending(resolution) => {\n            if let Some(request) = state.current_pending_request().cloned() {\n                if state.user_response_blocked(&request.request_id) { return vec![]; }')
replace('src/app/user_input.rs', '    request: InteractiveRequest,\n    question_index: usize,\n    answers: BTreeMap<String, Vec<String>>,', '    pub(super) request: InteractiveRequest,\n    question_index: usize,\n    pub(super) answers: BTreeMap<String, Vec<String>>,\n    pub(super) revision: u64,\n    pub(super) submission: Option<u64>,')
replace('src/app/user_input.rs', '        self.user_input_editor = Some(UserInputEditor {\n            request,', '        if self.user_response_blocked(&request.request_id) { return; }\n        self.user_input_editor = Some(UserInputEditor {\n            revision: 0,\n            submission: None,\n            request,')
replace('src/app/user_input.rs', '        self.input_buffer.clear();\n        if editor.question_index + 1 < questions.len() {', '        if editor.question_index + 1 < questions.len() {\n            self.input_buffer.clear();')
replace('src/app/user_input.rs', '''        let Some(editor) = self.user_input_editor.take() else {
            return vec![];
        };
        self.input_mode = InputMode::Normal;
        vec![Effect::ResolveInteractive {
            request_id: editor.request.request_id,
            resolution: InteractiveResolution::UserInput(editor.answers),
        }]''', '        self.begin_user_response()')
replace('src/app/user_input.rs', 'InteractiveRequest, InteractiveRequestKind, InteractiveResolution, RpcRequestId,', 'InteractiveRequest, InteractiveRequestKind, RpcRequestId,')

put('src/app_server/user_response.rs', r'''//! Validate the observed form before I/O; quarantine any attempted write on failure.
use super::*;
use crate::user_response::{UserResponseOutcome, UserResponseSubmission};
use std::future::Future;
pub(super) async fn send(
    rpc: &mut RpcSession,
    pending: &mut BTreeMap<RpcRequestId, PendingServerRequest>,
    threads: &BTreeMap<String, ThreadSummary>,
    submission: UserResponseSubmission,
) -> UserResponseOutcome {
    send_with(pending, threads, submission, RPC_REQUEST_TIMEOUT,
        |message| async move { rpc.write_message(&message).await }).await
}
async fn send_with<F, Fut>(
    pending: &mut BTreeMap<RpcRequestId, PendingServerRequest>,
    threads: &BTreeMap<String, ThreadSummary>,
    submission: UserResponseSubmission,
    deadline: Duration,
    write: F,
) -> UserResponseOutcome
where F: FnOnce(Value) -> Fut, Fut: Future<Output = Result<()>> {
    let id = &submission.request.request_id;
    let Some(current) = pending.get(id) else {
        return UserResponseOutcome::NotSent("input request is no longer pending".into());
    };
    let message = json!({"id": id.to_value(), "method": current.method, "params": current.params});
    if parse_interactive_request(&message).ok().flatten().as_ref() != Some(&submission.request)
        || !threads.contains_key(&submission.request.thread_id.0)
    {
        return UserResponseOutcome::NotSent("input request changed or its thread disappeared".into());
    }
    let crate::conversation::InteractiveRequestKind::UserInput { questions } = &submission.request.kind else {
        return UserResponseOutcome::NotSent("not a user-input form".into());
    };
    if submission.request.validate_user_input().is_err()
        || submission.answers.len() != questions.len()
        || questions.iter().any(|q| submission.answers.get(&q.id).is_none_or(|a| a.is_empty()))
    {
        return UserResponseOutcome::NotSent("answer keys do not match the observed form".into());
    }
    let answers = submission.answers.into_iter().map(|(id, answers)| (id, json!({"answers": answers})))
        .collect::<serde_json::Map<_, _>>();
    let response = json!({"id": id.to_value(), "result": {"answers": answers}});
    // From this point a partial write is possible. Never leave the old request
    // retryable merely because flush/timeout failed; local UI also fences retries.
    pending.remove(id);
    match with_rpc_deadline("user-input response", deadline, write(response)).await {
        Ok(()) => UserResponseOutcome::Written,
        Err(error) => UserResponseOutcome::Unknown(error.to_string()),
    }
}
#[cfg(test)]
mod tests;
''')
replace('src/app_server.rs', 'mod lifecycle;', 'mod lifecycle;\nmod user_response;')
replace('src/app_server.rs', 'pub enum BackendCommand {\n', 'pub enum BackendCommand {\n    SubmitUserInput(crate::user_response::UserResponseSubmission),\n')
replace('src/app_server.rs', 'pub enum ConversationEvent {\n', 'pub enum ConversationEvent {\n    UserResponse { ticket: u64, outcome: crate::user_response::UserResponseOutcome },\n')
replace('src/app_server.rs', 'impl RegistryHandle {\n', '''impl RegistryHandle {
    pub fn response_actor_finished(&self) -> bool { self.task.is_finished() }
    pub fn submit_user_input(&self, submission: crate::user_response::UserResponseSubmission) -> Result<()> {
        self.send_command(BackendCommand::SubmitUserInput(submission))
    }
''')
replace('src/app_server.rs', '                    BackendCommand::LoadConversation(thread_id) => {', '''                    BackendCommand::SubmitUserInput(submission) => {
                        let ticket = submission.ticket;
                        let outcome = user_response::send(&mut rpc, &mut pending_requests, &threads, submission).await;
                        send_conversation_event(&conversation_tx, ConversationEvent::UserResponse { ticket, outcome }).await;
                    }
                    BackendCommand::LoadConversation(thread_id) => {''')
replace('src/app_server.rs', '''            InteractiveResolution::UserInput(answers) => {
                let answers = answers
                    .into_iter()
                    .map(|(question_id, answers)| (question_id, json!({"answers": answers})))
                    .collect::<serde_json::Map<_, _>>();
                rpc.respond_result(request_id.to_value(), json!({"answers": answers}))
                    .await?;
            }''', '''            InteractiveResolution::UserInput(_) => {
                anyhow::bail!("user input requires the checked submission path")
            }''')
put('src/runtime_user_response.rs', r'''//! Admission is synchronous; the actor emits a separate, ticketed write receipt.
use codex_tui::{app::{Action, AppState, reduce}, app_server::RegistryHandle,
    conversation::RpcRequestId, user_response::{UserResponseAnswers, UserResponseOutcome, UserResponseSubmission}};
pub(crate) fn submit(app: &mut AppState, registry: Option<&RegistryHandle>, id: &RpcRequestId, answers: &UserResponseAnswers) {
    submit_with(app, id, answers, |submission| registry.ok_or_else(|| "App Server unavailable; answer not enqueued".to_string())?
        .submit_user_input(submission).map_err(|error| error.to_string()));
}
fn submit_with(app: &mut AppState, id: &RpcRequestId, answers: &UserResponseAnswers,
    admit: impl FnOnce(UserResponseSubmission) -> Result<(), String>) {
    let Some(submission) = app.user_response_submission(id, answers) else {
        reduce(app, Action::MutationNotice("answer submission no longer matches its retained editor; not sent".into()));
        return;
    };
    let ticket = submission.ticket;
    if let Err(error) = admit(submission) { app.finish_user_response(ticket, UserResponseOutcome::NotSent(error)); }
}
#[cfg(test)]
mod tests;
''')
replace('src/main.rs', 'mod runtime_input;', 'mod runtime_input;\nmod runtime_user_response;')
replace('src/main.rs', '        | ConversationEvent::InteractiveResolved { .. }', '        | ConversationEvent::InteractiveResolved { .. }\n        | ConversationEvent::UserResponse { .. }')
replace('src/main.rs', '            ConversationEvent::InteractiveResolved { request_id } => {', '''            ConversationEvent::UserResponse { ticket, outcome } => {
                app.finish_user_response(ticket, outcome);
            }
            ConversationEvent::InteractiveResolved { request_id } => {''')
replace('src/main.rs', '''        let Some(event) = registry.try_recv_conversation() else {
            break;
        };''', '''        let Some(event) = registry.try_recv_conversation() else {
            if registry.response_actor_finished() { changes.any |= app.user_response_actor_stopped(); }
            break;
        };''')
replace('src/main.rs', '''            Effect::ResolveInteractive {
                request_id,
                resolution,
            } => {''', '''            Effect::ResolveInteractive {
                request_id,
                resolution,
            } => {
                if let codex_tui::conversation::InteractiveResolution::UserInput(answers) = &resolution {
                    runtime_user_response::submit(app, registry, &request_id, answers);
                    continue;
                }''')
# Keep the previous successful-send assertion, but wait for the actual write receipt.
replace('tests/user_input_intent.rs', '''    assert_eq!(answers["q2"], vec!["second-answer"]);
    assert_eq!(app.input_mode, InputMode::Normal);''', '''    assert_eq!(answers["q2"], vec!["second-answer"]);
    let submission = app.user_response_submission(request_id, answers).unwrap();
    assert_eq!(app.input_mode, InputMode::UserInput);
    app.finish_user_response(submission.ticket, codex_tui::user_response::UserResponseOutcome::Written);
    assert_eq!(app.input_mode, InputMode::Normal);''')

put('src/app_server/user_response/tests.rs', r'''use super::*;
use crate::backend::{CodexBackend, FakeBackend};
use crate::user_response::UserResponseSubmission;
use std::cell::Cell;
fn fixture() -> (BTreeMap<RpcRequestId, PendingServerRequest>, BTreeMap<String, ThreadSummary>, UserResponseSubmission) {
    let threads = by_id(FakeBackend::seeded().snapshot().threads);
    let thread = threads.keys().next().unwrap();
    let params = json!({"threadId": thread, "turnId": "turn", "itemId": "item", "questions": [{"id":"q", "question":"Answer?", "isSecret":true}]});
    let wire = json!({"id":"form", "method":"item/tool/requestUserInput", "params":params});
    let request = parse_interactive_request(&wire).unwrap().unwrap();
    let pending = BTreeMap::from([(request.request_id.clone(), PendingServerRequest { method:"item/tool/requestUserInput".into(), params })]);
    let submission = UserResponseSubmission { ticket:1, request, answers:BTreeMap::from([("q".into(), vec!["synthetic-only".into()])]) };
    (pending, threads, submission)
}
#[tokio::test]
async fn response_is_written_once_with_original_id_and_answers() {
    let (mut pending, threads, s) = fixture();
    let outcome = send_with(&mut pending, &threads, s.clone(), Duration::from_secs(1), |m| async move {
        assert_eq!(m, json!({"id":"form","result":{"answers":{"q":{"answers":["synthetic-only"]}}}})); Ok(())
    }).await;
    assert_eq!(outcome, UserResponseOutcome::Written);
    assert!(pending.is_empty());
    assert!(matches!(send_with(&mut pending, &threads, s, Duration::from_secs(1), |_| async { panic!("second write") }).await, UserResponseOutcome::NotSent(_)));
}
#[tokio::test]
async fn changed_form_or_removed_thread_is_refused_before_io() {
    for changed_thread in [false, true] {
        let (mut pending, mut threads, s) = fixture();
        if changed_thread { threads.clear(); } else { pending.get_mut(&s.request.request_id).unwrap().params["questions"][0]["question"] = json!("Different?"); }
        let outcome = send_with(&mut pending, &threads, s, Duration::from_secs(1), |_| async { panic!("unexpected write") }).await;
        assert!(matches!(outcome, UserResponseOutcome::NotSent(_)));
        assert_eq!(pending.len(), 1);
    }
}
#[tokio::test]
async fn mismatched_answer_keys_are_refused_without_consuming_request() {
    let (mut pending, threads, mut s) = fixture();
    s.answers.insert("other".into(), vec!["x".into()]);
    assert!(matches!(send_with(&mut pending, &threads, s, Duration::from_secs(1), |_| async { panic!("unexpected write") }).await, UserResponseOutcome::NotSent(_)));
    assert_eq!(pending.len(),1);
}
#[tokio::test]
async fn write_failure_is_unknown_and_not_retryable() {
    let (mut pending, threads, s) = fixture();
    let calls = Cell::new(0);
    let outcome = send_with(&mut pending, &threads, s.clone(), Duration::from_secs(1), |_| { calls.set(calls.get()+1); async { anyhow::bail!("injected broken pipe") } }).await;
    assert!(matches!(outcome, UserResponseOutcome::Unknown(_)));
    assert!(matches!(send_with(&mut pending, &threads, s, Duration::from_secs(1), |_| { calls.set(calls.get()+1); async { Ok(()) } }).await, UserResponseOutcome::NotSent(_)));
    assert_eq!(calls.get(),1);
}
#[tokio::test]
async fn stalled_write_is_bounded_and_not_restored_for_retry() {
    let (mut pending, threads, s) = fixture();
    let outcome = tokio::time::timeout(Duration::from_secs(1), send_with(&mut pending, &threads, s, Duration::from_millis(5), |_| std::future::pending::<Result<()>>())).await.unwrap();
    assert!(matches!(outcome, UserResponseOutcome::Unknown(_)));
    assert!(pending.is_empty());
}
#[test]
fn debug_does_not_expose_answers_and_actual_command_queue_is_bounded() {
    let (_, _, s) = fixture();
    assert!(!format!("{s:?}").contains("synthetic-only"));
    let (tx, _rx) = mpsc::channel(1);
    queue_backend_command(&tx, BackendCommand::SubmitUserInput(s.clone())).unwrap();
    assert!(queue_backend_command(&tx, BackendCommand::SubmitUserInput(s)).is_err());
}
''')
put('src/runtime_user_response/tests.rs', r'''use super::*;
use codex_tui::{app::{Effect, InputMode, View}, backend::{CodexBackend, FakeBackend},
    conversation::{InteractiveRequest, InteractiveRequestKind, InteractiveResolution, UserInputQuestion}};
fn ready() -> (AppState, RpcRequestId, UserResponseAnswers) {
    let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
    let request = InteractiveRequest { request_id:RpcRequestId::String("form".into()), thread_id:app.threads[0].id.clone(), turn_id:"t".into(), item_id:"i".into(),
        kind:InteractiveRequestKind::UserInput { questions:vec![UserInputQuestion { id:"q".into(), header:"H".into(), question:"Q?".into(), is_secret:true, options:vec![] }] } };
    app.view = View::Thread(request.thread_id.clone());
    reduce(&mut app, Action::InteractiveRequested(request));
    reduce(&mut app, Action::BeginUserInput);
    reduce(&mut app, Action::InputText("answer".into()));
    let effect = reduce(&mut app, Action::CommitInput).pop().unwrap();
    let Effect::ResolveInteractive {request_id, resolution:InteractiveResolution::UserInput(answers)} = effect else { panic!("response") };
    (app, request_id, answers)
}
#[test]
fn missing_backend_preserves_answer_and_allows_explicit_retry() {
    let (mut app, id, answers) = ready();
    submit(&mut app, None, &id, &answers);
    assert_eq!(app.input_mode,InputMode::UserInput);
    assert_eq!(app.input_buffer,"answer");
    assert!(app.user_response_submission(&id,&answers).is_none());
    assert_eq!(reduce(&mut app,Action::CommitInput).len(),1);
}
#[test]
fn actual_bounded_queue_refusal_is_not_a_write_receipt() {
    let (mut app, id, answers) = ready();
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    tx.try_send(app.user_response_submission(&id,&answers).unwrap()).unwrap();
    submit_with(&mut app,&id,&answers, |s| tx.try_send(s).map_err(|e|e.to_string()));
    assert_eq!(app.input_buffer,"answer");
    assert!(app.user_response_submission(&id,&answers).is_none());
    rx.try_recv().unwrap();
    reduce(&mut app,Action::CommitInput);
    submit_with(&mut app,&id,&answers, |s| tx.try_send(s).map_err(|e|e.to_string()));
    assert!(app.user_response_submission(&id,&answers).is_some());
    assert_eq!(app.input_mode,InputMode::UserInput);
}
#[test]
fn old_and_unrelated_receipts_cannot_clear_answer() {
    let (mut app, id, answers) = ready();
    let first=app.user_response_submission(&id,&answers).unwrap();
    app.finish_user_response(first.ticket,UserResponseOutcome::NotSent("full".into()));
    reduce(&mut app,Action::CommitInput);
    let second=app.user_response_submission(&id,&answers).unwrap();
    assert_ne!(first.ticket,second.ticket);
    app.finish_user_response(first.ticket,UserResponseOutcome::Written);
    assert_eq!(app.input_buffer,"answer");
    app.finish_user_response(second.ticket,UserResponseOutcome::Written);
    assert_eq!(app.input_mode,InputMode::Normal);
    assert!(app.input_buffer.is_empty());
}
#[test]
fn later_edit_and_edit_undo_survive_success() {
    for undo in [false,true] {
        let (mut app,id,answers)=ready();
        let ticket=app.user_response_submission(&id,&answers).unwrap().ticket;
        reduce(&mut app,Action::InputChar('!'));
        if undo {reduce(&mut app,Action::InputBackspace);}
        app.finish_user_response(ticket,UserResponseOutcome::Written);
        assert_eq!(app.input_mode,InputMode::UserInput);
        assert_eq!(app.input_buffer,if undo {"answer"} else {"answer!"});
    }
}
#[test]
fn uncertain_response_blocks_retry_but_keeps_answer_and_explicit_close() {
    let (mut app,id,answers)=ready();
    let ticket=app.user_response_submission(&id,&answers).unwrap().ticket;
    app.finish_user_response(ticket,UserResponseOutcome::Unknown("broken pipe".into()));
    assert!(reduce(&mut app,Action::CommitInput).is_empty());
    assert_eq!(app.input_buffer,"answer");
    reduce(&mut app,Action::CancelInput);
    assert_eq!(app.input_mode,InputMode::Normal);
    reduce(&mut app,Action::BeginUserInput);
    assert_eq!(app.input_mode,InputMode::Normal);
}
#[test]
fn disconnect_is_reported_once_and_never_retried() {
    let (mut app,_,_)=ready();
    assert!(app.user_response_actor_stopped());
    assert!(!app.user_response_actor_stopped());
    assert_eq!(app.input_buffer,"answer");
    assert!(reduce(&mut app,Action::CommitInput).is_empty());
}
#[test]
fn replaced_form_is_not_removed_by_old_success() {
    let (mut app,id,answers)=ready();
    let original=app.user_response_submission(&id,&answers).unwrap();
    let mut replacement=original.request.clone(); replacement.item_id="new-item".into();
    reduce(&mut app,Action::InteractiveRequested(replacement.clone()));
    app.finish_user_response(original.ticket,UserResponseOutcome::Written);
    assert!(app.pending_requests.contains(&replacement));
}
#[test]
fn explicit_close_is_not_resurrected_by_failure() {
    let (mut app,id,answers)=ready();
    let ticket=app.user_response_submission(&id,&answers).unwrap().ticket;
    reduce(&mut app,Action::CancelInput);
    app.finish_user_response(ticket,UserResponseOutcome::NotSent("full".into()));
    assert_eq!(app.input_mode,InputMode::Normal);
    assert!(app.input_buffer.is_empty());
}
''')
plan=Path('release/v1.4-plan.json'); data=json.loads(plan.read_text())
data['moduleRatchet'].update({'src/user_response.rs':90, 'src/app/user_response.rs':260, 'src/app_server/user_response.rs':120, 'src/app_server/user_response/tests.rs':200, 'src/runtime_user_response.rs':80, 'src/runtime_user_response/tests.rs':240})
plan.write_text(json.dumps(data,indent=2,ensure_ascii=False)+'\n')
put('docs/implementation/v1.4-user-response-delivery.md', '''# User answer admission and transport receipts

Frozen v1.4 defect correction from 380b81e950674fb88a064789d9bc4aa8b3c23323.

The final answer previously destroyed its editor before bounded command admission.
A missing backend, full queue or failed transport write could lose all answers.
This correction retains the original form and raw final input until a matching
transport receipt. The bounded transient ledger permits at most 64 outstanding
or uncertain forms. Repeated Enter and competing cancellation are refused; explicit
editor close does not cancel accepted work. Newer edits and edit/undo revisions
are not cleared by an old write receipt. No durable answer store is introduced.

NotSent is used only before transport I/O. Such refusal retains input for an
explicit user retry. Written means only local transport completion, never server
acknowledgement. Unknown covers attempted writes/timeouts/actor loss; those IDs
remain fenced against automatic or accidental retry until authoritative resolution
or process restart. Restart is not a claim that retry is safe; verify backend state.
The actor revalidates the full observed form and live thread before I/O and removes
an attempted request from its retryable map. Responses have the existing five-second
RPC deadline. Identity fences do not create server CAS or exactly-once delivery.

The old unchecked answer route now refuses answers rather than silently bypassing
this path. Approval and explicit pre-submission cancellation semantics are not
redesigned. Requests/answers are memory-only; this is not power-loss durability or
secure-memory zeroization. Synthetic tests never replace real account/Windows-SSH
qualification; stableReady and publicationAllowed remain false.

Validation uses actual reducers and TestBackend for baseline failures, actual bounded
channels for admission, the production actor write helper for changed forms, broken
pipes and deadlines, plus full existing tests and module gates. Before/after counts
and exact SHA/tree identities are retained by the PR verifier, not asserted here.
''')
Path('.audit/response_fix.py').unlink()
Path('.audit').rmdir()
Path('.github/workflows/audit-user-response.yml').unlink()
