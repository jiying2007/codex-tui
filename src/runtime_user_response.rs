//! Admission is synchronous; the actor emits a separate, ticketed write receipt.
use codex_tui::{
    app::{Action, AppState, reduce},
    app_server::RegistryHandle,
    conversation::RpcRequestId,
    user_response::{UserResponseAnswers, UserResponseOutcome, UserResponseSubmission},
};
pub(crate) fn submit(
    app: &mut AppState,
    registry: Option<&RegistryHandle>,
    id: &RpcRequestId,
    answers: &UserResponseAnswers,
) {
    submit_with(app, id, answers, |submission| {
        registry
            .ok_or_else(|| "App Server unavailable; answer not enqueued".to_string())?
            .submit_user_input(submission)
            .map_err(|error| error.to_string())
    });
}
fn submit_with(
    app: &mut AppState,
    id: &RpcRequestId,
    answers: &UserResponseAnswers,
    admit: impl FnOnce(UserResponseSubmission) -> Result<(), String>,
) {
    let Some(submission) = app.user_response_submission(id, answers) else {
        reduce(
            app,
            Action::MutationNotice(
                "answer submission no longer matches its retained editor; not sent".into(),
            ),
        );
        return;
    };
    let ticket = submission.ticket;
    if let Err(error) = admit(submission) {
        app.finish_user_response(ticket, UserResponseOutcome::NotSent(error));
    }
}
#[cfg(test)]
mod tests;
