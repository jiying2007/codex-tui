//! Build a response only for the exact current request and compatible user decision.
use super::*;
use crate::conversation::InteractiveRequestKind;
pub(super) fn response(
    s: &UserResponseSubmission,
    pending: &PendingServerRequest,
) -> Result<Value> {
    let id = s.request.request_id.to_value();
    if s.request.thread_id.0.trim().is_empty()
        || s.request.turn_id.trim().is_empty()
        || s.request.item_id.trim().is_empty()
    {
        anyhow::bail!("empty interactive target");
    }
    let resolution = &s.resolution;
    let decision = match resolution {
        InteractiveResolution::Accept => "accept",
        InteractiveResolution::Decline => "decline",
        InteractiveResolution::Cancel => "cancel",
        InteractiveResolution::UserInput(_) => "answers",
    };
    let result = match (&s.request.kind, resolution) {
        (
            InteractiveRequestKind::UserInput { questions },
            InteractiveResolution::UserInput(answers),
        ) => {
            s.request.validate_user_input()?;
            if answers.len() != questions.len()
                || questions
                    .iter()
                    .any(|q| answers.get(&q.id).is_none_or(|a| a.is_empty()))
            {
                anyhow::bail!("answer keys do not match the observed form");
            }
            let answers = answers
                .iter()
                .map(|(id, answers)| (id.clone(), json!({"answers":answers})))
                .collect::<serde_json::Map<_, _>>();
            json!({"answers":answers})
        }
        (
            InteractiveRequestKind::UserInput { .. },
            InteractiveResolution::Decline | InteractiveResolution::Cancel,
        ) => {
            return Ok(
                json!({"id":id,"error":{"code":-32000,"message":"user input cancelled by user"}}),
            );
        }
        (InteractiveRequestKind::UserInput { .. }, _)
        | (_, InteractiveResolution::UserInput(_)) => {
            anyhow::bail!("response kind does not match interactive request");
        }
        (InteractiveRequestKind::PermissionsApproval { .. }, InteractiveResolution::Accept) => {
            let permissions = pending
                .params
                .get("permissions")
                .filter(|p| p.is_object())
                .context("permission request missing object permissions")?;
            json!({"permissions":permissions,"scope":"turn"})
        }
        (InteractiveRequestKind::PermissionsApproval { .. }, _) => {
            json!({"permissions":{},"scope":"turn"})
        }
        (_, _) => json!({"decision":decision}),
    };
    Ok(json!({"id":id,"result":result}))
}
