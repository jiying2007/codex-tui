//! Check the rendered frame, not just logical viewport indices.
use codex_tui::{
    app::{AppState, View},
    planning::{SavedView, SavedViewLayout, ScratchState, ScratchWork, reconcile_scratch_card},
    ui,
};
use ratatui::{Terminal, backend::TestBackend, style::Modifier};

fn fixture(layout: SavedViewLayout, grouped: bool, title: &str, workspace: &str) -> AppState {
    let mut app = AppState::new(vec![]);
    app.view = View::Board;
    app.planning_snapshot.saved_views.push(SavedView {
        id: "viewport-regression".into(),
        name: "Viewport regression".into(),
        source_scope: "all".into(),
        filter: String::new(),
        group_by: grouped.then(|| "workspace".into()),
        order_by: Some("title".into()),
        layout,
        visible_fields: vec!["stage".into(), "attention".into(), "workspace".into()],
    });
    app.planning_view_index = 4;
    app.work_cards = (0..100)
        .map(|index| {
            reconcile_scratch_card(&ScratchWork {
                id: format!("card-{index:03}"),
                title: format!("CARD{index:03} {title}"),
                note: None,
                workspace: Some(format!("group-{:03}/{workspace}", index / 4)),
                priority: None,
                state: ScratchState::Inbox,
                created_at_unix_ms: 1,
                updated_at_unix_ms: 1,
            })
        })
        .collect();
    app
}

#[test]
fn long_planning_rows_keep_first_middle_and_last_selection_visible() {
    for layout in [
        SavedViewLayout::Board,
        SavedViewLayout::List,
        SavedViewLayout::ReviewQueue,
    ] {
        for width in [80, 160] {
            for grouped in [false, true] {
                for (title, workspace) in [
                    ("long title ".repeat(40), "repo".into()),
                    ("short".into(), "工作区/很长的路径/".repeat(40)),
                    ("中文👩‍💻e\u{301}标题 ".repeat(40), "repo".into()),
                ] {
                    let mut app = fixture(layout, grouped, &title, &workspace);
                    for selected in [0, 50, 99] {
                        app.board_selected = selected;
                        let expected =
                            app.visible_planning_cards()[selected].title[..7].to_string();
                        let marker = format!("> {expected}");
                        let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
                        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
                        let buffer = terminal.backend().buffer();
                        let mut matches = 0;
                        for y in 0..40 {
                            let row = (0..width)
                                .map(|x| buffer[(x, y)].symbol())
                                .collect::<String>();
                            if let Some(start) = row.find(&marker) {
                                matches += 1;
                                // The marker is in the first Board column or List content,
                                // before any wide graphemes; terminal columns stay exact.
                                let x = row[..start].chars().count() as u16;
                                for offset in 0..marker.len() as u16 {
                                    assert!(
                                        buffer[(x + offset, y)]
                                            .modifier
                                            .contains(Modifier::REVERSED)
                                    );
                                }
                            }
                        }
                        assert_eq!(
                            matches, 1,
                            "{layout:?}, {width} columns, grouped={grouped}, selected={selected}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn planning_card_uses_one_terminal_row_and_truncates_wide_text() {
    let mut app = fixture(
        SavedViewLayout::Board,
        false,
        &"中文👩‍💻e\u{301} ".repeat(40),
        "repo",
    );
    app.board_selected = 50;
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let selected_y = (0..40)
        .find(|&y| buffer[(1, y)].symbol() == ">")
        .expect("selected card marker remains on screen");
    let row = (1..31)
        .map(|x| buffer[(x, selected_y)].symbol())
        .collect::<String>();
    assert!(row.contains("中") && row.contains("文") && row.contains('…'));
    assert!(matches!(buffer[(31, selected_y)].symbol(), "│" | "█"));
    assert!(
        !buffer[(1, selected_y + 1)]
            .modifier
            .contains(Modifier::REVERSED)
    );
    assert!(app.work_cards[50].title.ends_with("中文👩‍💻e\u{301} "));
}
