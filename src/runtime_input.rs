use codex_tui::{
    app::{Action, AppState, Effect, InputMode, ViewKind, reduce},
    command::Command,
    conversation::{InteractiveRequestKind, InteractiveResolution},
    goal::GoalStatus,
    keymap::command_for_key,
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub(crate) fn handle_paste(app: &mut AppState, text: String) -> Vec<Effect> {
    if text.is_empty() {
        return vec![];
    }
    if app.terminal_focused {
        return vec![Effect::TerminalPaste(text)];
    }
    if app.command_palette_open {
        return reduce(app, Action::CommandPaletteInputText(text));
    }
    if app.input_mode != InputMode::Normal {
        return reduce(app, Action::InputText(text));
    }
    vec![]
}

fn is_terminal_release_key(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::F(6))
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char(']'))
        || key.code == KeyCode::Char('\u{001d}')
}

pub(crate) fn handle_key(app: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return vec![];
    }

    if app.terminal_focused {
        if is_terminal_release_key(key) {
            return reduce(app, Action::SetTerminalFocus(false));
        }
        if key.modifiers.contains(KeyModifiers::SHIFT) && key.code == KeyCode::PageUp {
            return reduce(app, Action::TerminalScroll(10));
        }
        if key.modifiers.contains(KeyModifiers::SHIFT) && key.code == KeyCode::PageDown {
            return reduce(app, Action::TerminalScroll(-10));
        }
        return terminal_key_bytes(key)
            .map(|bytes| vec![Effect::TerminalInput(bytes)])
            .unwrap_or_default();
    }

    if app.command_palette_open {
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('k') {
            return reduce(app, Action::CloseCommandPalette);
        }
        return match key.code {
            KeyCode::Esc => reduce(app, Action::CloseCommandPalette),
            KeyCode::Down => reduce(app, Action::MoveCommandPalette(1)),
            KeyCode::Up => reduce(app, Action::MoveCommandPalette(-1)),
            KeyCode::Backspace => reduce(app, Action::CommandPaletteBackspace),
            KeyCode::Enter => {
                let choice = app.command_palette_choice();
                reduce(app, Action::CloseCommandPalette);
                choice.map_or_else(Vec::new, |choice| handle_palette_choice(app, choice))
            }
            KeyCode::Char(character)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                reduce(app, Action::CommandPaletteInputChar(character))
            }
            _ => vec![],
        };
    }

    if app.launch_menu_open {
        return match key.code {
            KeyCode::Esc => reduce(app, Action::CloseLaunchPresets),
            KeyCode::Char('j') | KeyCode::Down => reduce(app, Action::MoveLaunchPreset(1)),
            KeyCode::Char('k') | KeyCode::Up => reduce(app, Action::MoveLaunchPreset(-1)),
            KeyCode::Enter => reduce(app, Action::SelectLaunchPreset),
            _ => vec![],
        };
    }

    if app.context_open {
        return match key.code {
            KeyCode::Esc => reduce(app, Action::CloseContext),
            KeyCode::Char('j') | KeyCode::Down => reduce(app, Action::MoveContext(1)),
            KeyCode::Char('k') | KeyCode::Up => reduce(app, Action::MoveContext(-1)),
            KeyCode::Enter => reduce(app, Action::ExecuteContext),
            _ => vec![],
        };
    }

    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
        return handle_command(app, Command::QuitOrInterrupt);
    }

    if app.input_mode != InputMode::Normal {
        let action = match key.code {
            KeyCode::Esc => Action::CancelInput,
            KeyCode::Enter => Action::CommitInput,
            KeyCode::Backspace => Action::InputBackspace,
            KeyCode::Char(character)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                Action::InputChar(character)
            }
            _ => return vec![],
        };
        return reduce(app, action);
    }

    if app.pending_launch_plan.is_some() {
        return match key.code {
            KeyCode::Char('y') => reduce(app, Action::ConfirmPendingOperation),
            KeyCode::Char('c') | KeyCode::Esc => reduce(app, Action::CancelPendingOperation),
            _ => vec![],
        };
    }

    if app.pending_local_batch.is_some() {
        return match key.code {
            KeyCode::Char('y') => reduce(app, Action::ConfirmPendingOperation),
            KeyCode::Char('c') | KeyCode::Esc => reduce(app, Action::CancelPendingOperation),
            _ => vec![],
        };
    }

    if app.pending_forge_operation.is_some() {
        return match key.code {
            KeyCode::Char('y') => reduce(app, Action::ConfirmPendingOperation),
            KeyCode::Char('c') | KeyCode::Esc => reduce(app, Action::CancelPendingOperation),
            _ => vec![],
        };
    }

    if app.goal_actions_open {
        let action = match key.code {
            KeyCode::Esc => Some(Action::CloseGoalActions),
            KeyCode::Enter | KeyCode::Char('e') => Some(Action::BeginGoalObjective),
            KeyCode::Char('p') => Some(Action::SetGoalStatus(GoalStatus::Paused)),
            KeyCode::Char('r') => Some(Action::SetGoalStatus(GoalStatus::Active)),
            KeyCode::Char('c') => Some(Action::ClearGoal),
            _ => None,
        };
        if let Some(action) = action {
            return reduce(app, action);
        }
        return vec![];
    }

    if app.view_kind() == ViewKind::Thread
        && let Some(request) = app.current_pending_request()
    {
        let action = match (&request.kind, key.code) {
            (InteractiveRequestKind::UserInput { .. }, KeyCode::Enter | KeyCode::Char('i')) => {
                Some(Action::BeginUserInput)
            }
            (InteractiveRequestKind::UserInput { .. }, KeyCode::Char('n')) => {
                Some(Action::ResolvePending(InteractiveResolution::Decline))
            }
            (InteractiveRequestKind::UserInput { .. }, KeyCode::Char('c')) => {
                Some(Action::ResolvePending(InteractiveResolution::Cancel))
            }
            (_, KeyCode::Char('y')) => Some(Action::ResolvePending(InteractiveResolution::Accept)),
            (_, KeyCode::Char('n')) => Some(Action::ResolvePending(InteractiveResolution::Decline)),
            (_, KeyCode::Char('c')) => Some(Action::ResolvePending(InteractiveResolution::Cancel)),
            _ => None,
        };
        if let Some(action) = action {
            return reduce(app, action);
        }
    }

    let Some(command) = command_for_key(key, app.view_kind()) else {
        return vec![];
    };
    handle_command(app, command)
}

fn terminal_key_bytes(key: KeyEvent) -> Option<Vec<u8>> {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return None;
    }

    let mut bytes = match key.code {
        KeyCode::Char(character) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if character.is_ascii() {
                let value = character as u8;
                if (b'@'..=b'_').contains(&value.to_ascii_uppercase()) {
                    vec![value.to_ascii_uppercase() & 0x1f]
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        KeyCode::Char(character) => {
            let mut buffer = [0_u8; 4];
            character.encode_utf8(&mut buffer).as_bytes().to_vec()
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        _ => return None,
    };

    if key.modifiers.contains(KeyModifiers::ALT) {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn handle_palette_choice(app: &mut AppState, command: Command) -> Vec<Effect> {
    handle_command(app, command)
}

pub(crate) fn action_for_command(app: &AppState, command: Command) -> Option<Action> {
    let action = match command {
        Command::QuitOrInterrupt => match app.view_kind() {
            ViewKind::Registry => Action::Quit,
            ViewKind::Thread => Action::InterruptCurrent,
            ViewKind::Review
            | ViewKind::Workspace
            | ViewKind::ManagedWorktrees
            | ViewKind::Board
            | ViewKind::Scratch => Action::Back,
        },
        Command::Back => Action::Back,
        Command::Help => Action::ToggleHelp,
        Command::Search => Action::BeginSearch,
        Command::ToggleHostLocalFilter => Action::ToggleHostLocalFilter,
        Command::ToggleRepoBackedFilter => Action::ToggleRepoBackedFilter,
        Command::ToggleAllHistory => Action::ToggleAllHistory,
        Command::Next => match app.view_kind() {
            ViewKind::Review => Action::MoveReview(1),
            ViewKind::ManagedWorktrees => Action::MoveManagedWorktree(1),
            ViewKind::Board => Action::MovePlanningSelection(1),
            _ => Action::MoveSelection(1),
        },
        Command::Previous => match app.view_kind() {
            ViewKind::Review => Action::MoveReview(-1),
            ViewKind::ManagedWorktrees => Action::MoveManagedWorktree(-1),
            ViewKind::Board => Action::MovePlanningSelection(-1),
            _ => Action::MoveSelection(-1),
        },
        Command::Open => {
            if app.view_kind() == ViewKind::Board {
                Action::OpenPlanningSelected
            } else {
                Action::OpenSelected
            }
        }
        Command::NextAttention => Action::NextAttention,
        Command::QuickPrompt => Action::QuickPrompt,
        Command::MarkUnread => Action::MarkUnread,
        Command::TogglePin => Action::TogglePin,
        Command::EditAlias => Action::BeginAlias,
        Command::AcknowledgeAttention => Action::AcknowledgeAttention,
        Command::ApprovePending => Action::ResolvePending(InteractiveResolution::Accept),
        Command::DeclinePending => Action::ResolvePending(InteractiveResolution::Decline),
        Command::CancelPending => Action::ResolvePending(InteractiveResolution::Cancel),
        Command::AnswerPending => Action::BeginUserInput,
        Command::Board => Action::OpenBoard,
        Command::BoardLeft => Action::MoveBoardColumn(-1),
        Command::BoardRight => Action::MoveBoardColumn(1),
        Command::CycleSavedView => Action::CycleSavedView(1),
        Command::Review => Action::OpenReview,
        Command::Workspace => Action::OpenWorkspace,
        Command::ManagedWorktrees => Action::OpenManagedWorktrees,
        Command::CreateWorktree => Action::BeginCreateWorktree,
        Command::AdoptWorktree => Action::BeginAdoptCurrentWorktree,
        Command::RemoveWorktree => Action::BeginRemoveManagedWorktree,
        Command::DeleteBranch => Action::BeginDeleteBranch,
        Command::ConfirmOperation => Action::ConfirmPendingOperation,
        Command::CancelOperation => Action::CancelPendingOperation,
        Command::New => Action::BeginScratch,
        Command::Snooze => Action::BeginSnooze,
        Command::BeginHotSlotBind => Action::BeginHotSlotBind,
        Command::PageUp => {
            if app.view_kind() == ViewKind::Review {
                Action::ScrollReviewBy(-10)
            } else {
                Action::ScrollBy(-5)
            }
        }
        Command::PageDown => {
            if app.view_kind() == ViewKind::Review {
                Action::ScrollReviewBy(10)
            } else {
                Action::ScrollBy(5)
            }
        }
        Command::ToggleWordDiff => Action::ToggleReviewWordDiff,
        Command::ExternalEditor => Action::OpenReviewExternalEditor,
        Command::TerminalDrawer => Action::ToggleTerminalDrawer,
        Command::CloseTerminalDrawer => Action::CloseTerminalDrawer,
        Command::HotSlot(slot) => Action::UseHotSlot(slot),
        Command::ContextActions => Action::OpenContext,
        Command::Goal => Action::OpenGoalActions,
        Command::CommandPalette => Action::OpenCommandPalette,
        Command::OpenExternal => Action::OpenReviewExternal,
    };
    Some(action)
}

pub(crate) fn handle_command(app: &mut AppState, command: Command) -> Vec<Effect> {
    let Some(action) = action_for_command(app, command) else {
        return vec![];
    };
    reduce(app, action)
}
