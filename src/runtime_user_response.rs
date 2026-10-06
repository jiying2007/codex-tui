//! Admission is synchronous; the actor emits a separate, ticketed write receipt.
use codex_tui::{
    app::{Action, AppState, reduce},
    app_server::RegistryHandle,
    conversation::{InteractiveResolution, RpcRequestId},
    user_response::{UserResponseOutcome, UserResponseSubmission},
};
pub(crate) fn submit(
    app: &mut AppState,
    registry: Option<&RegistryHandle>,
    id: &RpcRequestId,
    resolution: &InteractiveResolution,
) {
    submit_with(app, id, resolution, |submission| {
        registry
            .ok_or_else(|| "App Server unavailable; response not enqueued".to_string())?
            .submit_user_response(submission)
            .map_err(|error| error.to_string())
    });
}
fn submit_with(
    app: &mut AppState,
    id: &RpcRequestId,
    resolution: &InteractiveResolution,
    admit: impl FnOnce(UserResponseSubmission) -> Result<(), String>,
) {
    let Some(submission) = app.user_response_submission(id, resolution) else {
        reduce(
            app,
            Action::MutationNotice(
                "response submission no longer matches its retained intent; not sent".into(),
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

#[cfg(test)]
mod approval_tests;
