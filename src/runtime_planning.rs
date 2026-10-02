use crate::runtime_store::RuntimeStore;
use codex_tui::app::{Action, AppState, Effect, reduce};
use codex_tui::planning::PlanningSnapshot;

pub(crate) fn apply_planning_effect(
    app: &mut AppState,
    store: &mut RuntimeStore,
    effect: Effect,
) -> bool {
    match effect {
        Effect::PersistOperatorState => {
            if let Some(error) = store.persist_operator_state(&app.to_local_state()) {
                reduce(app, Action::PlanningStoreDegraded(Some(error)));
            }
        }
        Effect::PersistOperatorStateDeferred => {
            store.defer_operator_state();
        }
        Effect::CreateScratch { title, workspace } => {
            apply_store_result(app, store.create_scratch(title, workspace));
        }
        Effect::SnoozeWorkCard {
            anchor,
            duration_ms,
        } => {
            apply_store_result(app, store.snooze_work_card(anchor, duration_ms));
        }
        Effect::SaveSourceNote { owner, text } => {
            apply_store_result(app, store.save_source_note(owner, text));
        }
        Effect::UpdateScratchNote { scratch_id, note } => {
            apply_store_result(app, store.update_scratch_note(scratch_id, note));
        }
        Effect::CreateBookmark { source, label } => {
            apply_store_result(app, store.create_bookmark(source, label));
        }
        Effect::UpdateScratchState { scratch_id, state } => {
            apply_store_result(app, store.update_scratch_state(scratch_id, state));
        }
        Effect::DeleteScratch { scratch_id } => {
            apply_store_result(app, store.delete_scratch(scratch_id));
        }
        Effect::SaveSavedView { view } => {
            apply_store_result(app, store.save_view(view));
        }
        Effect::DeleteSavedView { view_id } => {
            apply_store_result(app, store.delete_view(view_id));
        }
        Effect::SetHotSlot { slot, target } => {
            apply_store_result(app, store.set_hot_slot(slot, target));
        }
        Effect::ApplyLocalBatch(plan) => {
            let preview = plan.preview();
            match store.apply_local_batch(&plan) {
                Ok(snapshot) => {
                    apply_store_result(app, Ok(snapshot));
                    reduce(
                        app,
                        Action::MutationNotice(format!(
                            "{} · {preview}",
                            super::runtime_text(
                                app.language,
                                "local batch applied",
                                "本地批量操作已应用",
                            )
                        )),
                    );
                }
                Err(error) => apply_store_result(app, Err(error)),
            }
        }
        _ => return false,
    }
    true
}

pub(crate) fn apply_store_result(app: &mut AppState, result: Result<PlanningSnapshot, String>) {
    match result {
        Ok(snapshot) => {
            reduce(app, Action::PlanningSnapshotLoaded(snapshot));
            reduce(
                app,
                Action::ReconcilePlanning {
                    now_unix_ms: super::now_unix_ms(),
                },
            );
            reduce(app, Action::PlanningStoreDegraded(None));
        }
        Err(error) => {
            reduce(app, Action::PlanningStoreDegraded(Some(error)));
        }
    }
}
