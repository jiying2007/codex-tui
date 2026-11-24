//! Pure argument validation. This module must never discover configuration, touch
//! state, spawn a process, or enter a terminal (including the help/error paths).
use codex_tui::headless::validate_options as headless_options;
use std::collections::BTreeSet;

const HELP: &str = "Usage: codex-tui [--fake | --target NAME]
       codex-tui --help | --version
       codex-tui doctor [codex|git|forge|store|presets|terminal]
       codex-tui doctor compat [--json]
       codex-tui doctor bundle [--offline] [--output PATH]
       codex-tui headless <threads|work|status|attention|board|forge|worktrees> [--json] [--fake|--fixture-10k|--target NAME]
       codex-tui headless threads --all-targets --query TEXT [--json]
       codex-tui status | thread list | attention list | board list | forge status | worktree list
       codex-tui release <verify|benchmark|render-benchmark|interaction-benchmark|scale|failure-matrix> [OPTIONS]
       codex-tui soak [--rows N] [--cycles N] [--duration-seconds N] [--json]
       codex-tui workref <add|session|contract|evidence|handoff|ack|find|show|list|handoff-text> [OPTIONS]

Use --help after a command for its options. Doctor codex accepts --target NAME.
Exit codes: 0 success, 1 unexpected runtime error, 2 invalid usage, 3 degraded/blocked.
The default starts the interactive workbench. No headless mutation API exists.";
const DOCTOR_HELP: &str = "Usage: codex-tui doctor [codex|git|forge|store|presets|terminal]
       codex-tui doctor codex [--target NAME]
       codex-tui doctor compat [--json]
       codex-tui doctor bundle [--offline] [--output PATH]

Failed requested diagnostics exit 3; unknown scopes/options exit 2.";
const HEADLESS_HELP: &str = "Usage: codex-tui headless <threads|work|status|attention|board|forge|worktrees> [--json] [--fake|--fixture-10k|--target NAME]
Codex views use configured active target or --target NAME; forge/worktrees reject --target.
Explicit metadata lookup: threads --all-targets --query TEXT (no --target or fixtures).
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
const WORKREF_HELP: &str = "Usage: codex-tui workref add --key KEY --provider feishu-project|tapd|local-draft --space SPACE --type requirement|bug|technical --id EXTERNAL_ID [--url ORIGINAL_HTTPS_URL]
       codex-tui workref session --key KEY --alias NAME --target EXACT_TARGET_ID --home CODEX_HOME --thread THREAD_ID --repo REPOSITORY --role ROLE
       codex-tui workref contract --key KEY --kind KIND --source AUTHORITY --revision VERSION [--git-sha FULL_40_HEX_SHA]
       codex-tui workref evidence --key KEY --kind KIND --source EVIDENCE_REF --revision VERSION --result passed|failed|partial|unknown --environment ENV
       codex-tui workref handoff --key KEY --from SESSION_ALIAS --to SESSION_ALIAS --summary TEXT
       codex-tui workref ack --key KEY --index N --receipt EXPLICIT_RECEIPT
       codex-tui workref find --target EXACT_TARGET_ID --home CODEX_HOME --thread THREAD_ID [--json]
       codex-tui workref list [--json] | show --key KEY [--json] | handoff-text --key KEY --index N

All refs are MANUAL/UNVERIFIED until checked against their authoritative source.
No upstream API, automatic delivery, Codex call or external workflow change.
A handoff is only prepared; copy it manually, and only record an acknowledgement when received.";

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
        Some("workref") => WORKREF_HELP,
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
    validate_limits(input)?;
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
                Some("bundle") => options(flags, &["--offline"], &["--output"]),
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
                ) => headless_options(words[1], &words[2..]),
                _ => Err("headless requires a known read-only command".into()),
            }
        }
        Some("status") => headless_options("status", &words[1..]),
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
            let command = match words[0] {
                "thread" => "threads",
                "worktree" => "worktrees",
                other => other,
            };
            headless_options(command, &words[2..])
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
        Some("workref") => {
            if words.len() <= 2 && help {
                return Ok(());
            }
            codex_tui::workref::cli::validate_cli(
                &words[1..]
                    .iter()
                    .map(|word| (*word).to_string())
                    .collect::<Vec<_>>(),
            )
        }
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

fn validate_limits(words: &[&str]) -> Result<(), String> {
    for pair in words.windows(2) {
        let maximum = match (words.first().copied(), words.get(1).copied(), pair[0]) {
            (Some("soak"), _, "--rows") => codex_tui::soak::MAX_ROWS,
            (Some("soak"), _, "--cycles") => codex_tui::soak::MAX_CYCLES,
            (Some("soak"), _, "--duration-seconds") => {
                codex_tui::soak::MAX_DURATION_SECONDS as usize
            }
            (Some("release"), Some("scale"), "--rows") => codex_tui::scale_evidence::MAX_ROWS,
            _ => continue,
        };
        if pair[1].parse::<usize>().is_ok_and(|value| value > maximum) {
            return Err(format!("{} must be <= {maximum}", pair[0]));
        }
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
