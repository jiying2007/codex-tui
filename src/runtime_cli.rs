//! Pure argument validation. This module must never discover configuration, touch
//! state, spawn a process, or enter a terminal (including the help/error paths).
use std::collections::BTreeSet;

const HELP: &str = "Usage: codex-tui [--fake | --target NAME]
       codex-tui --help | --version
       codex-tui doctor [codex|git|forge|store|presets|terminal]
       codex-tui doctor compat [--json]
       codex-tui doctor bundle [--output PATH]
       codex-tui headless <threads|work|status|attention|board|forge|worktrees> [--json] [--fake|--fixture-10k]
       codex-tui status | thread list | attention list | board list | forge status | worktree list
       codex-tui release <verify|benchmark|render-benchmark|interaction-benchmark|scale|failure-matrix> [OPTIONS]
       codex-tui soak [--rows N] [--cycles N] [--duration-seconds N] [--json]

Use --help after a command for its options. Doctor codex accepts --target NAME.
Exit codes: 0 success, 1 unexpected runtime error, 2 invalid usage, 3 degraded/blocked.
The default starts the interactive workbench. No headless mutation API exists.";
const DOCTOR_HELP: &str = "Usage: codex-tui doctor [codex|git|forge|store|presets|terminal]
       codex-tui doctor codex [--target NAME]
       codex-tui doctor compat [--json]
       codex-tui doctor bundle [--output PATH]

Failed requested diagnostics exit 3; unknown scopes/options exit 2.";
const HEADLESS_HELP: &str = "Usage: codex-tui headless <threads|work|status|attention|board|forge|worktrees> [--json] [--fake|--fixture-10k]
Read-only aliases: status, thread list, attention list, board list, forge status, worktree list.
Exit codes: 0 success, 2 invalid usage, 3 degraded.";
const RELEASE_HELP: &str = "Usage: codex-tui release verify --channel preview|stable --tag TAG --commit SHA [--evidence PATH] [--publish] [--json]
       codex-tui release benchmark [--warmup N] [--iterations N] [--source LABEL] [--json]
       codex-tui release render-benchmark [--warmup N] [--iterations N] [--source LABEL] [--json]
       codex-tui release interaction-benchmark [--warmup N] [--iterations N] [--source LABEL] [--json]
       codex-tui release scale [--rows N] [--warmup N] [--iterations N] [--source LABEL] [--json]
       codex-tui release failure-matrix [--json]
Reports do not grant stable publication authority.";
const SOAK_HELP: &str =
    "Usage: codex-tui soak [--rows N] [--cycles N] [--duration-seconds N] [--json]
Deterministic structural churn, not a real-environment duration soak certificate.";

/// Returns help text or a validated invocation to dispatch through existing handlers.
pub(crate) fn preflight(args: &[String]) -> Result<Option<&'static str>, String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    if matches!(words.as_slice(), ["--help" | "-h" | "help"]) {
        return Ok(Some(HELP));
    }
    if matches!(words.as_slice(), ["--version" | "version"]) {
        return Ok(None);
    }
    let first = words.first().copied();
    let help = match first {
        Some("doctor") => DOCTOR_HELP,
        Some("headless" | "status" | "thread" | "attention" | "board" | "forge" | "worktree") => {
            HEADLESS_HELP
        }
        Some("release") => RELEASE_HELP,
        Some("soak") => SOAK_HELP,
        _ => HELP,
    };
    // Help is permitted only in a syntactically valid route, not as an option value.
    let wants_help = words
        .last()
        .is_some_and(|word| matches!(*word, "--help" | "-h"));
    let input = if wants_help {
        &words[..words.len() - 1]
    } else {
        &words[..]
    };
    validate(input, wants_help)?;
    Ok(wants_help.then_some(help))
}

fn validate(words: &[&str], help: bool) -> Result<(), String> {
    match words.first().copied() {
        None => Ok(()),
        Some("doctor") => {
            let (scope, flags) = match words.get(1) {
                Some(scope) if !scope.starts_with('-') => (Some(*scope), &words[2..]),
                _ => (None, &words[1..]),
            };
            match scope {
                None | Some("codex") => options(flags, &[], &["--target"]),
                Some("git" | "forge" | "store" | "presets" | "terminal") => {
                    options(flags, &[], &[])
                }
                Some("compat") => options(flags, &["--json"], &[]),
                Some("bundle") => options(flags, &[], &["--output"]),
                Some(other) => Err(format!("unknown doctor scope: {other}")),
            }
        }
        Some("headless") => {
            if words.len() == 1 && help {
                return Ok(());
            }
            match words.get(1) {
                Some(
                    &("threads" | "work" | "status" | "attention" | "board" | "forge"
                    | "worktrees"),
                ) => headless_options(&words[2..]),
                _ => Err("headless requires a known read-only command".into()),
            }
        }
        Some("status") => headless_options(&words[1..]),
        Some("thread" | "attention" | "board" | "forge" | "worktree") => {
            if words.len() == 1 && help {
                return Ok(());
            }
            let verb = if words[0] == "forge" {
                "status"
            } else {
                "list"
            };
            if words.get(1) != Some(&verb) {
                return Err(format!("usage: codex-tui {} {verb}", words[0]));
            }
            headless_options(&words[2..])
        }
        Some("release") => {
            if words.len() == 1 && help {
                return Ok(());
            }
            let flags = words.get(2..).unwrap_or_default();
            match words.get(1) {
                Some(&"verify") => {
                    options(
                        flags,
                        &["--json", "--publish"],
                        &["--channel", "--tag", "--commit", "--evidence"],
                    )?;
                    if !help
                        && ["--channel", "--tag", "--commit"]
                            .iter()
                            .any(|flag| !flags.contains(flag))
                    {
                        return Err("release verify requires --channel, --tag and --commit".into());
                    }
                    if let Some(index) = flags.iter().position(|flag| *flag == "--channel")
                        && !matches!(flags[index + 1], "preview" | "stable")
                    {
                        return Err("--channel must be preview or stable".into());
                    }
                    Ok(())
                }
                Some(&("benchmark" | "render-benchmark" | "interaction-benchmark")) => options(
                    flags,
                    &["--json"],
                    &["--warmup", "--iterations", "--source"],
                ),
                Some(&"scale") => options(
                    flags,
                    &["--json"],
                    &["--rows", "--warmup", "--iterations", "--source"],
                ),
                Some(&"failure-matrix") => options(flags, &["--json"], &[]),
                _ => Err(
                    "release requires verify, benchmark, render-benchmark, interaction-benchmark, scale or failure-matrix"
                        .into(),
                ),
            }
        }
        Some("soak") => options(
            &words[1..],
            &["--json"],
            &["--rows", "--cycles", "--duration-seconds"],
        ),
        Some(word) if word.starts_with('-') => {
            options(words, &["--fake"], &["--target"])?;
            if words.contains(&"--fake") && words.contains(&"--target") {
                return Err("--fake and --target cannot be combined".into());
            }
            Ok(())
        }
        Some(other) => Err(format!("unknown command: {other}")),
    }
}

fn headless_options(words: &[&str]) -> Result<(), String> {
    options(words, &["--json", "--fake", "--fixture-10k"], &[])?;
    if words.contains(&"--fake") && words.contains(&"--fixture-10k") {
        return Err("choose --fake or --fixture-10k, not both".into());
    }
    Ok(())
}

fn options(words: &[&str], switches: &[&str], values: &[&str]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    let mut index = 0;
    while let Some(word) = words.get(index) {
        if !seen.insert(*word) {
            return Err(format!("duplicate option: {word}"));
        }
        if values.contains(word) {
            index += 1;
            let value = words
                .get(index)
                .ok_or_else(|| format!("{word} requires a value"))?;
            if value.trim().is_empty() || value.starts_with('-') {
                return Err(format!("{word} requires a non-option value"));
            }
            if matches!(
                *word,
                "--rows" | "--cycles" | "--warmup" | "--iterations" | "--duration-seconds"
            ) && value.parse::<usize>().ok().filter(|n| *n > 0).is_none()
            {
                return Err(format!("{word} requires a positive integer"));
            }
        } else if !switches.contains(word) {
            return Err(format!("unknown option or argument: {word}"));
        }
        index += 1;
    }
    Ok(())
}
