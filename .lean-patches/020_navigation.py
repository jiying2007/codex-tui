from lean_patch_support import replace, write

write('src/app/mission.rs', r'''//! Three daily entry points reuse existing projections and navigation ownership.
use super::*;

impl AppState {
    pub(super) fn open_today(&mut self) {
        self.view = View::Registry;
        self.input_mode = InputMode::Normal;
        self.board_return_view = None;
        self.selected = self.visible_indices().first().copied().unwrap_or(0);
    }

    pub(super) fn open_mission_saved_view(&mut self, id: &str) {
        let Some(index) = self.planning_views().iter().position(|view| view.id == id) else { return; };
        if !matches!(self.view, View::Board) {
            self.board_return_view = Some(self.view.clone());
        }
        self.view = View::Board;
        self.planning_view_index = index;
        self.board_stage_index = 0;
        self.board_selected = 0;
        self.input_mode = InputMode::Normal;
    }

    pub(super) fn open_delivery(&mut self) -> Vec<Effect> {
        let url = self.selected_local_target()
            .filter(|target| target.kind == SourceKind::CodexThread)
            .and_then(|target| {
                let id = ThreadId::new(target.value);
                let thread = self.thread_by_id(&id)?;
                let forge = self.forge_observation(&id)?;
                if forge.cwd != thread.metadata.cwd || forge.identity.is_none() { return None; }
                // A cached Git result from a former cwd must not select a different change request.
                if self.git_context(&id).is_some_and(|git| git.cwd != thread.metadata.cwd) { return None; }
                self.review_external_url(&id)
            });
        match url {
            Some(url) => vec![Effect::OpenExternalUrl { url }],
            None => {
                self.mutation_notice = Some(local_text(self.language,
                    "delivery metadata unavailable for this exact selection; open Review or run doctor forge",
                    "当前精确选择没有可用交付信息；请打开评审或运行 doctor forge").into());
                vec![]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};
    #[test]
    fn daily_routes_reuse_the_existing_views_without_write_effects() {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        assert!(reduce(&mut app, Action::OpenProjects).iter().all(|effect| !matches!(effect, Effect::OpenExternalUrl { .. })));
        assert_eq!(app.view, View::Board);
        assert_eq!(app.active_saved_view().id, "builtin:projects");
        assert_eq!(app.active_saved_view().layout, SavedViewLayout::List);
        reduce(&mut app, Action::OpenReviewQueue);
        assert_eq!(app.active_saved_view().id, "builtin:review");
        reduce(&mut app, Action::OpenToday);
        assert_eq!(app.view, View::Registry);
    }
    #[test]
    fn missing_delivery_identity_never_opens_an_unrelated_link() {
        let mut app = AppState::new(vec![]);
        assert!(reduce(&mut app, Action::OpenDelivery).is_empty());
        assert!(app.mutation_notice.as_deref().unwrap().contains("unavailable"));
    }
}
''')
replace('src/app.rs', 'mod context_menu;', 'mod mission;\nmod context_menu;')
replace('src/command.rs', '    QuitOrInterrupt,', '    Today,\n    Projects,\n    ReviewQueue,\n    Delivery,\n    QuitOrInterrupt,')
replace('src/command.rs', '        Some(match (self, simplified_chinese) {', '''        Some(match (self, simplified_chinese) {
            (Self::Today, false) => "Today / Mission Control",
            (Self::Today, true) => "今日 / 任务中心",
            (Self::Projects, false) => "Projects",
            (Self::Projects, true) => "项目",
            (Self::ReviewQueue, false) => "Review queue",
            (Self::ReviewQueue, true) => "交付评审队列",
            (Self::Delivery, false) => "Open delivery in browser",
            (Self::Delivery, true) => "在浏览器打开交付",''')
replace('src/app/types.rs', 'pub enum Action {', 'pub enum Action {\n    OpenToday,\n    OpenProjects,\n    OpenReviewQueue,\n    OpenDelivery,')
replace('src/app/reducer.rs', '    match action {\n        Action::ReplaceThreads', '''    match action {
        Action::OpenToday => state.open_today(),
        Action::OpenProjects => state.open_mission_saved_view("builtin:projects"),
        Action::OpenReviewQueue => state.open_mission_saved_view("builtin:review"),
        Action::OpenDelivery => return state.open_delivery(),
        Action::ReplaceThreads''')
replace('src/runtime_commands.rs', '    let action = match command {', '''    let action = match command {
        Command::Today => Action::OpenToday,
        Command::Projects => Action::OpenProjects,
        Command::ReviewQueue => Action::OpenReviewQueue,
        Command::Delivery => Action::OpenDelivery,''')
replace('src/keymap.rs', 'pub const HELP_BINDINGS: &[HelpBinding] = &[', '''pub const HELP_BINDINGS: &[HelpBinding] = &[
    HelpBinding { surface: "Daily", scope: HelpScope::AllViews, token: "F1", code: KeyCode::F(1), modifiers: KeyModifiers::NONE, command: Command::Today },
    HelpBinding { surface: "Daily", scope: HelpScope::AllViews, token: "F2", code: KeyCode::F(2), modifiers: KeyModifiers::NONE, command: Command::Projects },
    HelpBinding { surface: "Daily", scope: HelpScope::AllViews, token: "F3", code: KeyCode::F(3), modifiers: KeyModifiers::NONE, command: Command::ReviewQueue },
    HelpBinding { surface: "Registry", scope: HelpScope::View(ViewKind::Registry), token: "o", code: KeyCode::Char('o'), modifiers: KeyModifiers::NONE, command: Command::Delivery },
    HelpBinding { surface: "Board", scope: HelpScope::View(ViewKind::Board), token: "o", code: KeyCode::Char('o'), modifiers: KeyModifiers::NONE, command: Command::Delivery },''')
replace('src/keymap.rs', '    match (view, key.code) {', '''    match (view, key.code) {
        (_, KeyCode::F(1)) if key.modifiers.is_empty() => Some(Command::Today),
        (_, KeyCode::F(2)) if key.modifiers.is_empty() => Some(Command::Projects),
        (_, KeyCode::F(3)) if key.modifiers.is_empty() => Some(Command::ReviewQueue),
        (ViewKind::Registry | ViewKind::Board, KeyCode::Char('o')) => Some(Command::Delivery),''')
replace('src/app/palette.rs', '        let mut choices = vec![Command::Search, Command::TranscriptSearch];', '        let mut choices = vec![Command::Today, Command::Projects, Command::ReviewQueue, Command::Search, Command::TranscriptSearch];')
replace('src/app/palette.rs', '            choices.push(Command::QuickPrompt);', '            choices.push(Command::QuickPrompt);\n            choices.push(Command::Delivery);')
replace('src/app/palette.rs', '                            Command::Search\n                                | Command::NextAttention', '                            Command::Today\n                                | Command::Projects\n                                | Command::ReviewQueue\n                                | Command::Delivery\n                                | Command::Search\n                                | Command::NextAttention')
replace('src/app/palette_intent.rs', '                        Command::Help\n', '                        Command::Today\n                            | Command::Projects\n                            | Command::ReviewQueue\n                            | Command::Help\n')
replace('src/app/palette_intent.rs', '                            Command::Review\n', '                            Command::Delivery\n                                | Command::Review\n')
# Append the new built-in rather than shifting saved-view identities or existing indices.
p = __import__('pathlib').Path('src/planning.rs')
s = p.read_text()
a = s.index('pub fn builtin_saved_views()')
b = s.index('\npub fn apply_saved_view', a)
part = s[a:b]
assert part.count('    ]\n}') == 1
part = part.replace('    ]\n}', '''        SavedView {
            id: "builtin:projects".into(),
            name: "Projects".into(),
            source_scope: "all".into(),
            filter: "-stage:done".into(),
            group_by: Some("workspace".into()),
            order_by: Some("workspace".into()),
            layout: SavedViewLayout::List,
            visible_fields: vec!["attention".into(), "stage".into(), "workspace".into()],
        },
    ]
}''')
p.write_text(s[:a] + part + s[b:])
replace('src/ui.rs', '        "builtin:forge" => "Forge 工作",', '        "builtin:forge" => "Forge 工作",\n        "builtin:projects" => "项目",')
write('tests/lean_navigation.rs', r'''use codex_tui::{app::{Action, AppState, View, reduce}, command::Command, keymap::command_for_key};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
#[test]
fn daily_shortcuts_and_palette_are_available_without_selected_threads() {
    let mut app = AppState::new(vec![]);
    for (key, command) in [(1, Command::Today), (2, Command::Projects), (3, Command::ReviewQueue)] {
        assert_eq!(command_for_key(KeyEvent::new(KeyCode::F(key), KeyModifiers::NONE), app.view_kind()), Some(command));
    }
    reduce(&mut app, Action::OpenCommandPalette);
    let choices = app.command_palette_choices();
    for command in [Command::Today, Command::Projects, Command::ReviewQueue] { assert!(choices.contains(&command)); }
    reduce(&mut app, Action::CloseCommandPalette);
    reduce(&mut app, Action::OpenProjects);
    assert_eq!(app.view, View::Board);
    assert_eq!(app.active_saved_view().id, "builtin:projects");
    reduce(&mut app, Action::OpenReviewQueue);
    assert_eq!(app.active_saved_view().id, "builtin:review");
    reduce(&mut app, Action::OpenToday);
    assert_eq!(app.view, View::Registry);
}
#[test]
fn daily_shortcuts_do_not_replace_hot_slots_or_drawer_focus_release() {
    let app = AppState::new(vec![]);
    assert_ne!(command_for_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE), app.view_kind()), Some(Command::Today));
    assert_ne!(command_for_key(KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE), app.view_kind()), Some(Command::Today));
}
''')
