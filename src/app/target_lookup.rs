//! Transient read-only locator; a result cannot become a mutation route.
use super::{AppState, View};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LookupPhase {
    Editing,
    Requested,
    Running,
    Results(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetLookupPanel {
    pub generation: u64,
    pub query: String,
    pub phase: LookupPhase,
    pub selected: usize,
    pub scroll: u16,
    pub invalid: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetLookupAction {
    Open,
    Input(String),
    Backspace,
    Submit,
    Move(i32),
    Scroll(i32),
    Close,
    Loaded { generation: u64, pages: Vec<String> },
}

impl AppState {
    pub(super) fn reduce_target_lookup(&mut self, action: TargetLookupAction) {
        if action == TargetLookupAction::Open {
            if matches!(self.view, View::Registry)
                && self.backend_status.source != "fake"
                && self.recent_alert_controls_available()
            {
                self.target_lookup_generation = self.target_lookup_generation.wrapping_add(1);
                self.target_lookup = Some(TargetLookupPanel {
                    generation: self.target_lookup_generation,
                    query: String::new(),
                    phase: LookupPhase::Editing,
                    selected: 0,
                    scroll: 0,
                    invalid: false,
                });
            }
            return;
        }
        if action == TargetLookupAction::Close {
            self.target_lookup = None;
            return;
        }
        let Some(panel) = self.target_lookup.as_mut() else {
            return;
        };
        match action {
            TargetLookupAction::Input(text) if panel.phase == LookupPhase::Editing => {
                for ch in text.chars().filter(|ch| !ch.is_control()) {
                    if panel.query.len() + ch.len_utf8() > 256 {
                        break;
                    }
                    panel.query.push(ch);
                }
                panel.invalid = false;
            }
            TargetLookupAction::Backspace if panel.phase == LookupPhase::Editing => {
                panel.query.pop();
                panel.invalid = false;
            }
            TargetLookupAction::Submit if panel.phase == LookupPhase::Editing => {
                if crate::headless::validate_options(
                    "threads",
                    &["--all-targets", "--query", &panel.query],
                )
                .is_ok()
                {
                    panel.phase = LookupPhase::Requested;
                } else {
                    panel.invalid = true;
                }
            }
            TargetLookupAction::Move(delta) => {
                if let LookupPhase::Results(pages) = &panel.phase {
                    panel.selected =
                        super::reducer::wrap_selection(panel.selected, delta, pages.len());
                    panel.scroll = 0;
                }
            }
            TargetLookupAction::Scroll(delta) => {
                if let LookupPhase::Results(pages) = &panel.phase {
                    let max = pages.get(panel.selected).map_or(0, |page| {
                        page.chars().count().min(usize::from(u16::MAX)) as u16
                    });
                    panel.scroll = i32::from(panel.scroll)
                        .saturating_add(delta)
                        .clamp(0, i32::from(max)) as u16;
                }
            }
            TargetLookupAction::Loaded { generation, pages }
                if generation == panel.generation && panel.phase == LookupPhase::Running =>
            {
                panel.phase = LookupPhase::Results(pages);
                panel.selected = 0;
                panel.scroll = 0;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Action, reduce};

    #[test]
    fn explicit_query_is_bounded_and_stale_result_cannot_replace_panel() {
        let mut app = AppState::new(vec![]);
        let act = |app: &mut AppState, action| reduce(app, Action::TargetLookup(action));
        act(&mut app, TargetLookupAction::Open);
        act(&mut app, TargetLookupAction::Submit);
        assert!(app.target_lookup.as_ref().unwrap().invalid);
        act(&mut app, TargetLookupAction::Input("中".repeat(100)));
        assert!(app.target_lookup.as_ref().unwrap().query.len() <= 256);
        act(&mut app, TargetLookupAction::Submit);
        let first = app.target_lookup.as_mut().unwrap();
        assert_eq!(first.phase, LookupPhase::Requested);
        first.phase = LookupPhase::Running;
        let generation = first.generation;
        act(&mut app, TargetLookupAction::Close);
        act(&mut app, TargetLookupAction::Open);
        act(
            &mut app,
            TargetLookupAction::Loaded {
                generation,
                pages: vec!["stale".into()],
            },
        );
        let second = app.target_lookup.as_ref().unwrap();
        assert_ne!(second.generation, generation);
        assert_eq!(second.phase, LookupPhase::Editing);
        assert!(matches!(app.view, View::Registry));
        assert!(!app.recent_alert_controls_available());
    }
}
