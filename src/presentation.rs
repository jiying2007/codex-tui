use serde::{Deserialize, Serialize};
use std::time::Duration;

const QUIET_BACKGROUND_REDRAW_MS: u64 = 100;
const SCREEN_READER_BACKGROUND_REDRAW_MS: u64 = 500;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresentationMode {
    #[default]
    Normal,
    Quiet,
    ScreenReader,
}

impl PresentationMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Quiet => "quiet",
            Self::ScreenReader => "screen-reader",
        }
    }

    pub const fn background_redraw_interval_ms(self) -> u64 {
        match self {
            Self::Normal => 0,
            Self::Quiet => QUIET_BACKGROUND_REDRAW_MS,
            Self::ScreenReader => SCREEN_READER_BACKGROUND_REDRAW_MS,
        }
    }

    pub fn background_redraw_interval(self) -> Duration {
        Duration::from_millis(self.background_redraw_interval_ms())
    }

    pub fn should_render(self, elapsed: Duration, urgent: bool) -> bool {
        urgent || elapsed >= self.background_redraw_interval()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_preserves_immediate_background_rendering() {
        assert!(PresentationMode::Normal.should_render(Duration::ZERO, false));
    }

    #[test]
    fn quiet_coalesces_only_background_redraws() {
        let mode = PresentationMode::Quiet;
        assert!(!mode.should_render(Duration::from_millis(99), false));
        assert!(mode.should_render(Duration::from_millis(100), false));
        assert!(mode.should_render(Duration::ZERO, true));
    }

    #[test]
    fn screen_reader_has_the_strongest_background_coalescing() {
        let mode = PresentationMode::ScreenReader;
        assert!(!mode.should_render(Duration::from_millis(499), false));
        assert!(mode.should_render(Duration::from_millis(500), false));
        assert!(mode.should_render(Duration::ZERO, true));
    }
}
