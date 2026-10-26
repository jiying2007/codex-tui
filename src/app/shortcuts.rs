//! Visible feedback for shortcut binding and unmapped slots.
use super::{AppState, local_text};

pub(super) fn begin_bind(state: &mut AppState) {
    state.hot_slot_bind_pending = state.selected_local_target();
    let message = if state.hot_slot_bind_pending.is_some() {
        local_text(
            state.language,
            "shortcut binding: press 1-9 to save, Esc to cancel",
            "快捷槽绑定中：按 1–9 保存，Esc 取消",
        )
    } else {
        local_text(
            state.language,
            "select a local thread or Board item before binding a shortcut",
            "先选择本机会话或看板项目再绑定快捷槽",
        )
    };
    state.mutation_notice = Some(message.into());
}

pub(super) fn cancelled(state: &mut AppState) {
    state.mutation_notice = Some(local_text(
        state.language, "shortcut binding cancelled", "快捷槽绑定已取消"
    ).into());
}

pub(super) fn empty_slot(state: &mut AppState, slot: u8) {
    state.mutation_notice = Some(format!(
        "{} {slot}: {}",
        local_text(state.language, "shortcut slot", "快捷槽"),
        local_text(
            state.language,
            "unassigned; select a task and press = to bind",
            "尚未绑定；选择任务后按 = 绑定",
        )
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Action, reduce};
    use crate::backend::{CodexBackend, FakeBackend};

    #[test]
    fn shortcut_feedback_is_visible_and_cancellable() {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        reduce(&mut app, Action::UseHotSlot(9));
        let notice = app.mutation_notice.as_deref().unwrap();
        assert!(notice.contains("unassigned"));
        reduce(&mut app, Action::BeginHotSlotBind);
        assert!(app.hot_slot_bind_pending.is_some());
        assert!(app.mutation_notice.as_deref().unwrap().contains("Esc"));
        reduce(&mut app, Action::Back);
        assert!(app.hot_slot_bind_pending.is_none());
    }

    #[test]
    fn binding_without_selection_reports_missing_target() {
        let mut app = AppState::new(vec![]);
        reduce(&mut app, Action::BeginHotSlotBind);
        assert!(app.hot_slot_bind_pending.is_none());
        assert!(app.mutation_notice.as_deref().unwrap().contains("select"));
    }
}
