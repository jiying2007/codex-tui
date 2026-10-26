use super::*;
use crate::backend::{CodexBackend, FakeBackend};
use ratatui::{Terminal, backend::TestBackend};

fn screen(app: &AppState) -> String {
    let mut terminal = Terminal::new(TestBackend::new(20, 10)).expect("tty");
    terminal.draw(|frame| render(frame, app)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut screen = String::new();
    for y in 0..buffer.area.height {
        let mut x = 0;
        while x < buffer.area.width {
            let symbol = buffer[(x, y)].symbol();
            screen.push_str(symbol);
            x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
        }
        screen.push('\n');
    }
    screen
}

#[test]
fn twenty_column_help_reaches_both_ends_in_both_languages() {
    for (language, first, last) in [
        (UiLanguage::English, "Global:", "only."),
        (UiLanguage::SimplifiedChinese, "全局:", "态。"),
    ] {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        app.language = language;
        app.show_help = true;
        app.help_top = 0;
        assert!(screen(&app).contains(first));
        app.help_top = 4096;
        let final_page = screen(&app);
        assert!(final_page.contains(last), "{language:?}: {final_page}");
    }
}
