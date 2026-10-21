use super::{AppState, Block, Frame, Line, Paragraph, Rect, render_item_scrollbar, tr};

pub(super) fn render(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let Some(details) = app.active_request_details() else {
        return;
    };
    let footer_height = area.height.saturating_sub(1).min(1);
    let content = Rect {
        height: area.height.saturating_sub(footer_height),
        ..area
    };
    let block = if content.height >= 3 && content.width >= 3 {
        Block::bordered().title(tr(app, " Request details · local ", " 请求详情 · 本地 "))
    } else {
        Block::default()
    };
    let inner = block.inner(content);
    let (lines, top, total) = details.visible(inner.width, inner.height);
    let visible = lines.len();
    frame.render_widget(
        Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>()).block(block),
        content,
    );
    render_item_scrollbar(frame, content, total, top, visible);
    frame.render_widget(
        Paragraph::new(tr(
            app,
            "Esc back · ↑/↓ PgUp/PgDn scroll · Home/End",
            "Esc 返回 · ↑/↓ PgUp/PgDn 滚动 · Home/End",
        )),
        Rect {
            y: area.y.saturating_add(content.height),
            height: footer_height,
            ..area
        },
    );
}
