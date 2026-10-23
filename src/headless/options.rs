use super::OutputFormat;
use std::collections::BTreeSet;

pub(super) struct HeadlessOptions {
    pub(super) format: OutputFormat,
    pub(super) fake: bool,
    pub(super) fixture_10k: bool,
    pub(super) target: Option<String>,
}

/// Shared pure validation for binary preflight and direct library callers.
pub fn validate_options(command: &str, words: &[&str]) -> Result<(), String> {
    HeadlessOptions::parse(command, words).map(|_| ())
}

impl HeadlessOptions {
    pub(super) fn parse(command: &str, words: &[&str]) -> Result<Self, String> {
        if !matches!(
            command,
            "threads" | "work" | "status" | "attention" | "board" | "forge" | "worktrees"
        ) {
            return Err(format!("unknown headless command: {command}"));
        }
        let mut options = Self {
            format: OutputFormat::Text,
            fake: false,
            fixture_10k: false,
            target: None,
        };
        let mut seen = BTreeSet::new();
        let mut index = 0;
        while let Some(word) = words.get(index) {
            if !seen.insert(*word) {
                return Err(format!("duplicate option: {word}"));
            }
            match *word {
                "--json" => options.format = OutputFormat::Json,
                "--fake" => options.fake = true,
                "--fixture-10k" => options.fixture_10k = true,
                "--target" => {
                    if matches!(command, "forge" | "worktrees") {
                        return Err(format!("{command} does not accept --target"));
                    }
                    index += 1;
                    let value = words.get(index).ok_or("--target requires a value")?;
                    if value.trim().is_empty() || value.starts_with('-') {
                        return Err("--target requires a non-option value".into());
                    }
                    options.target = Some((*value).into());
                }
                _ => return Err(format!("unknown headless option: {word}")),
            }
            index += 1;
        }
        if options.fake && options.fixture_10k {
            return Err("choose --fake or --fixture-10k, not both".into());
        }
        options.fake |= options.fixture_10k;
        if options.fake && options.target.is_some() {
            return Err("--target cannot be combined with fixture input".into());
        }
        Ok(options)
    }

    #[cfg(test)]
    pub(super) fn fixture(scaled: bool) -> Self {
        Self {
            format: OutputFormat::Json,
            fake: true,
            fixture_10k: scaled,
            target: None,
        }
    }
}
