//! Transient, UTF-8-safe composer cursor and viewport controls.
//! Draft bytes remain in ThreadUiState; cursor/view position never persists.
use super::{AppState, ThreadId};

fn byte_offset(draft: &str, position: usize) -> usize {
    draft
        .char_indices()
        .nth(position)
        .map_or(draft.len(), |(byte, _)| byte)
}

pub(super) fn open(state: &mut AppState, thread: &ThreadId) {
    state.composer_cursor = state.thread_ui.get(&thread.0)
        .map_or(0, |ui| ui.draft.chars().count());
    state.composer_scroll_back = 0;
}

pub(super) fn insert(state: &mut AppState, text: &str) -> bool {
    let Some(id) = state.current_thread_id().cloned() else { return false };
    state.prompt_submissions.edited(&id);
    let draft = &mut state.thread_ui.entry(id.0).or_default().draft;
    let cursor = state.composer_cursor.min(draft.chars().count());
    draft.insert_str(byte_offset(draft, cursor), text);
    state.composer_cursor = cursor.saturating_add(text.chars().count());
    state.composer_scroll_back = 0;
    true
}

pub(super) fn backspace(state: &mut AppState) -> bool {
    let Some(id) = state.current_thread_id().cloned() else { return false };
    let draft = &mut state.thread_ui.entry(id.0.clone()).or_default().draft;
    let cursor = state.composer_cursor.min(draft.chars().count());
    if cursor == 0 { return false; }
    state.prompt_submissions.edited(&id);
    draft.remove(byte_offset(draft, cursor - 1));
    state.composer_cursor = cursor - 1;
    state.composer_scroll_back = 0;
    true
}

pub(super) fn move_cursor(state: &mut AppState, delta: i32) {
    let len = state.current_thread_ui().map_or(0, |ui| ui.draft.chars().count());
    state.composer_cursor = state.composer_cursor.saturating_add_signed(delta as isize).min(len);
    state.composer_scroll_back = 0;
}

pub(super) fn boundary(state: &mut AppState, end: bool) {
    state.composer_cursor = if end {
        state.current_thread_ui().map_or(0, |ui| ui.draft.chars().count())
    } else { 0 };
    state.composer_scroll_back = 0;
}

pub(super) fn scroll(state: &mut AppState, delta: i32) {
    if delta >= 0 {
        state.composer_scroll_back = state.composer_scroll_back.saturating_add(delta as usize);
    } else {
        state.composer_scroll_back = state.composer_scroll_back.saturating_sub(delta.unsigned_abs() as usize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::View;

    #[test]
    fn unicode_cursor_insert_backspace_and_scroll_are_lossless() {
        let mut app = AppState::new(vec![]);
        let id = ThreadId::new("draft");
        app.view = View::Thread(id.clone());
        app.thread_ui.entry(id.0.clone()).or_default().draft = "你🙂好".into();
        open(&mut app, &id);
        assert_eq!(app.composer_cursor, 3);
        move_cursor(&mut app, -1);
        assert!(insert(&mut app, "中"));
        assert_eq!(app.current_thread_ui().unwrap().draft, "你🙂中好");
        assert!(backspace(&mut app));
        assert_eq!(app.current_thread_ui().unwrap().draft, "你🙂好");
        boundary(&mut app, false);
        assert!(!backspace(&mut app));
        scroll(&mut app, 10);
        scroll(&mut app, -3);
        assert_eq!(app.composer_scroll_back, 7);
        boundary(&mut app, true);
        assert_eq!(app.composer_cursor, 3);
    }
}
