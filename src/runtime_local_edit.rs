//! Bind local editor receipts to ordered storage work, including failed admission.
use crate::{
    apply_planning_store_result,
    runtime_store::RuntimeStore,
    runtime_store_worker::{StoreEvent, StoreWorker},
};
use codex_tui::{
    app::{Action, AppState, reduce},
    planning::PlanningSnapshot,
};

pub(crate) fn submit<F>(app: &mut AppState, worker: &mut StoreWorker, ticket: Option<u64>, job: F)
where
    F: FnOnce(&mut RuntimeStore) -> Result<PlanningSnapshot, String> + Send + 'static,
{
    if let Err(error) = worker.planning_with_ticket(job, None, ticket) {
        if let Some(ticket) = ticket {
            app.finish_local_edit_write(
                ticket,
                Err(format!("operation was not accepted: {error}")),
            );
        } else {
            reduce(app, Action::MutationNotice(error));
        }
    }
}

pub(crate) fn apply_event(app: &mut AppState, event: StoreEvent) -> bool {
    match event {
        StoreEvent::Operator(error) => {
            if let Some(error) = error {
                reduce(app, Action::PlanningStoreDegraded(Some(error)));
            }
        }
        StoreEvent::Planning(result, notice, ticket) => {
            let succeeded = result.is_ok();
            if let Some(ticket) = ticket {
                app.finish_local_edit_write(
                    ticket,
                    result.as_ref().map(|_| ()).map_err(Clone::clone),
                );
            }
            apply_planning_store_result(app, result);
            if succeeded && let Some(notice) = notice {
                reduce(app, Action::MutationNotice(notice));
            }
            return succeeded;
        }
        StoreEvent::Stopped => {
            let error = "local state worker stopped; write outcome is unconfirmed".to_string();
            if let Some(ticket) = app.pending_local_edit_ticket() {
                app.finish_local_edit_write(ticket, Err(error.clone()));
            }
            reduce(app, Action::PlanningStoreDegraded(Some(error)));
        }
        StoreEvent::Search(Ok(results)) => {
            reduce(app, Action::TranscriptSearchLoaded(results));
        }
        StoreEvent::WorkrefLinked { key, result } => match result {
            Ok((ledger, alias)) => {
                app.install_workrefs(Some(ledger));
                let notice = if app.language.is_simplified_chinese() {
                    format!("已关联事项 {key} · 会话别名 {alias} · 手工/未核验；未修改飞书/TAPD")
                } else {
                    format!("Linked {key} · session alias {alias} · manual/unverified; Feishu/TAPD unchanged")
                };
                reduce(app, Action::MutationNotice(notice));
            }
            Err(error) => reduce(app, Action::MutationNotice(error)),
        },
        StoreEvent::Search(Err(error)) | StoreEvent::Notice(Some(error)) => {
            reduce(app, Action::MutationNotice(error));
        }
        StoreEvent::Notice(None) => {}
    }
    false
}

#[cfg(test)]
#[path = "runtime_local_edit/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "runtime_local_edit/route_tests.rs"]
mod route_tests;
