//! Render the complete draft as reachable, width-aware physical rows.
//! The caret marker is presentation-only and never enters the submitted draft.
use crate::app::{AppState, InputMode};
use crate::conversation_viewport::WrappedRows;
use ratatui::layout::Rect;

pub(super) fn prepare(
    app: &AppState,
    text: String,
    draft: &str,
    area: Rect,
) -> (String, u16) {
    if app.input_mode != InputMode::Composer || area.width <= 2 || area.height <= 2 {
        return (text, 0);
    }
    let cursor = app.composer_cursor.min(draft.chars().count());
    let offset = draft
        .char_indices()
        .nth(cursor)
        .map_or(draft.len(), |(byte, _)| byte);
    let mut display = text;
    display.insert(offset, '▏');
    let width = area.width - 2;
    let mut rows = Vec::new();
    for line in display.split('\n') {
        let start = rows.len();
        for span in WrappedRows::new(line, width, 0) {
            rows.push(line[span].to_owned());
        }
        if rows.len() == start {
            rows.push(String::new());
        }
    }
    let caret_row = rows.iter().position(|row| row.contains('▏')).unwrap_or(0);
    let visible = usize::from(area.height - 2);
    let max_top = rows.len().saturating_sub(visible);
    let base_top = caret_row.saturating_sub(visible.saturating_sub(1));
    let top = base_top.saturating_sub(app.composer_scroll_back).min(max_top);
    (rows.join("\n"), u16::try_from(top).unwrap_or(u16::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_drafts_reveal_tail_and_remain_scrollable() {
        let mut app = AppState::new(vec![]);
        app.input_mode = InputMode::Composer;
        let draft = format!("{}TAIL_SENTINEL", "A".repeat(100));
        app.composer_cursor = draft.chars().count();
        let area = Rect::new(0, 0, 80, 4);
        let (full, top) = prepare(&app, format!("{draft}\nstatus"), &draft, area);
        assert!(top > 0);
        let visible = full.lines().skip(usize::from(top)).take(2).collect::<Vec<_>>();
        assert!(visible.join("\n").contains("TAIL_SENTINEL"));
        app.composer_scroll_back = 3;
        let (_, earlier) = prepare(&app, format!("{draft}\nstatus"), &draft, area);
        assert_eq!(earlier, 0);
    }

    #[test]
    fn unicode_draft_never_splits_utf8_codepoints() {
        let mut app = AppState::new(vec![]);
        app.input_mode = InputMode::Composer;
        let draft = "中文🙂".repeat(50);
        app.composer_cursor = draft.chars().count();
        let (display, offset) = prepare(&app, format!("{draft}\nstatus"), &draft, Rect::new(0, 0, 20, 10));
        assert!(display.contains("▏"));
        assert!(offset > 0);
    }
}
