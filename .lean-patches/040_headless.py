from pathlib import Path
from lean_patch_support import replace, between, write

write('src/headless/options.rs', r'''//! One pure parser for entry-point preflight and library dispatch.
use std::collections::BTreeSet;

pub(super) struct Options<'a> {
    pub json: bool,
    pub fake: bool,
    pub fixture_10k: bool,
    pub target: Option<&'a str>,
}

impl<'a> Options<'a> {
    pub fn parse(command: &str, words: &[&'a str]) -> Result<Self, String> {
        let mut parsed = Self { json: false, fake: false, fixture_10k: false, target: None };
        let mut seen = BTreeSet::new();
        let mut index = 0;
        while let Some(word) = words.get(index) {
            if !seen.insert(*word) { return Err(format!("duplicate option: {word}")); }
            match *word {
                "--json" => parsed.json = true,
                "--fake" => parsed.fake = true,
                "--fixture-10k" => parsed.fixture_10k = true,
                "--target" => {
                    if !matches!(command, "threads" | "work" | "status" | "attention" | "board") {
                        return Err("--target applies only to Codex Registry-derived snapshots; forge/worktrees are local repository queries".into());
                    }
                    index += 1;
                    let value = words.get(index).filter(|value| !value.trim().is_empty() && !value.starts_with('-'))
                        .ok_or("--target requires a non-option name")?;
                    parsed.target = Some(*value);
                }
                _ => return Err(format!("unknown headless option: {word}")),
            }
            index += 1;
        }
        if parsed.fake && parsed.fixture_10k { return Err("choose --fake or --fixture-10k, not both".into()); }
        if parsed.target.is_some() && (parsed.fake || parsed.fixture_10k) {
            return Err("fixture mode and --target cannot be combined".into());
        }
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_option_is_order_independent_and_never_a_fixture_or_forge_override() {
        for words in [vec!["--json", "--target", "remote"], vec!["--target", "remote", "--json"]] {
            let parsed = Options::parse("attention", &words).unwrap();
            assert!(parsed.json);
            assert_eq!(parsed.target, Some("remote"));
        }
        for words in [vec!["--target"], vec!["--target", "--json"], vec!["--target", "a", "--target", "b"],
            vec!["--fake", "--target", "remote"], vec!["--fixture-10k", "--target", "remote"], vec!["--json", "--json"]] {
            assert!(Options::parse("status", &words).is_err());
        }
        for command in ["forge", "worktrees"] { assert!(Options::parse(command, &["--target", "remote"]).is_err()); }
    }
}
''')
write('src/headless/coverage.rs', r'''use serde::Serialize;

/// Availability is per source. A healthy scoped read is not a complete engineering snapshot.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SnapshotCoverage {
    pub scope: &'static str,
    pub registry: &'static str,
    pub local_operator_state: &'static str,
    pub git: &'static str,
    pub goal: &'static str,
    pub forge: &'static str,
    pub all_sources_observed: bool,
}

impl SnapshotCoverage {
    pub fn registry(connected: bool, complete: bool) -> Self {
        Self {
            scope: "registry-local",
            registry: if !connected { "unavailable" } else if complete { "complete" } else { "partial" },
            local_operator_state: "not-collected", git: "not-collected", goal: "not-collected", forge: "not-collected",
            all_sources_observed: false,
        }
    }
}
''')
write('src/headless/card_index.rs', r'''use crate::planning::{SourceRef, WorkCardRecord};
use std::collections::BTreeMap;

pub(super) fn by_anchor(cards: &[WorkCardRecord]) -> BTreeMap<&SourceRef, &WorkCardRecord> {
    let mut index = BTreeMap::new();
    for card in cards { index.entry(&card.anchor).or_insert(card); }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ThreadId;
    #[test]
    fn one_index_preserves_the_first_match_and_resolves_sparse_cards() {
        let mut cards = (0..5_000).map(|id| WorkCardRecord::implicit_thread(&ThreadId::new(format!("t-{id}")))).collect::<Vec<_>>();
        let mut duplicate = cards[17].clone();
        duplicate.local_id = "must-not-win".into();
        cards.push(duplicate);
        let index = by_anchor(&cards);
        assert_eq!(index.len(), 5_000);
        assert_eq!(index.get(&cards[17].anchor).unwrap().local_id, cards[17].local_id);
        assert!(!index.contains_key(&SourceRef::codex_thread(&ThreadId::new("absent"))));
    }
}
''')
replace('src/headless.rs', 'mod surfaces;', 'mod card_index;\nmod coverage;\nmod options;\nmod surfaces;\n\npub fn validate_options(command: &str, words: &[&str]) -> Result<(), String> {\n    options::Options::parse(command, words).map(|_| ())\n}')
replace('src/headless.rs', '    store::{LocalStateV1, LocalStore},', '    store::{FileStore, LocalStateV1, LocalStore},\n    app_server_target::ResolvedAppServerTarget,')
replace('src/headless.rs', 'struct SourceStatus {\n    source: String,', 'struct SourceStatus {\n    coverage: coverage::SnapshotCoverage,\n    source: String,')
between('src/headless.rs', '    let flags = &args[1..];', '\n    match command {', '''    let words = args[1..].iter().map(String::as_str).collect::<Vec<_>>();
    let options = match options::Options::parse(command, &words) {
        Ok(options) => options,
        Err(error) => { eprintln!("{}", headless_field(&error)); return Ok(EXIT_USAGE); }
    };
    let format = if options.json { OutputFormat::Json } else { OutputFormat::Text };
    let fixture_10k = options.fixture_10k;
    let fake = options.fake || fixture_10k;
    let target = options.target;
    if !options.json && matches!(command, "threads" | "work" | "status" | "attention" | "board") {
        println!("scope: registry-local; Git/Goal/Forge not collected; empty attention does not prove all work is healthy");
    }
''')
# Add the target parameter throughout the existing read-only pipeline; keep local-only Forge commands explicit.
p = Path('src/headless.rs'); s = p.read_text()
s = s.replace('build_thread_snapshot(fake, fixture_10k)', 'build_thread_snapshot(fake, fixture_10k, target)')
s = s.replace('build_work_snapshot(fake, fixture_10k)', 'build_work_snapshot(fake, fixture_10k, target)')
s = s.replace('load_backend_snapshot(fake, fixture_10k)', 'load_backend_snapshot(fake, fixture_10k, target)')
s = s.replace('fake: bool, fixture_10k: bool) ->', 'fake: bool, fixture_10k: bool, target: Option<&str>) ->')
for command in ['status', 'attention', 'board']:
    s = s.replace('surfaces::run_%s(fake, fixture_10k, format)' % command, 'surfaces::run_%s(fake, fixture_10k, target, format)' % command)
s = s.replace('build_thread_snapshot(false, true)', 'build_thread_snapshot(false, true, None)')
s = s.replace('build_work_snapshot(true, false)', 'build_work_snapshot(true, false, None)')
s = s.replace('build_work_snapshot(false, true)', 'build_work_snapshot(false, true, None)')
s = s.replace('source: SourceStatus {\n', 'source: SourceStatus {\n                coverage: coverage::SnapshotCoverage::registry(false, false),\n')
s = s.replace('            SourceStatus {\n', '            SourceStatus {\n                coverage: coverage::SnapshotCoverage::registry(false, false),\n')
s = s.replace('    SourceStatus {\n        source: status.source.clone(),', '    SourceStatus {\n        coverage: coverage::SnapshotCoverage::registry(status.connected, status.registry_complete),\n        source: status.source.clone(),')
s = s.replace('degraded: !status.connected || status.error.is_some(),', 'degraded: !status.connected || !status.registry_complete || status.error.is_some(),')
s = s.replace('    let (mut threads, source) = match backend {', '    let (mut threads, mut source) = match backend {')
s = s.replace('    apply_local_operator_state(&mut threads, &local);', '''    source.coverage.local_operator_state = if fake { "fixture" } else if local_store_error.is_some() { "unavailable" } else { "observed" };
    apply_local_operator_state(&mut threads, &local);''')
s = s.replace('    let mut projections = Vec::with_capacity(threads.len() + planning.scratch.len());', '    let cards_by_anchor = card_index::by_anchor(&planning.cards);\n    let mut projections = Vec::with_capacity(threads.len() + planning.scratch.len());')
assert s.count('let local_card = planning.cards.iter().find(|card| card.anchor == anchor);') == 2
s = s.replace('let local_card = planning.cards.iter().find(|card| card.anchor == anchor);', 'let local_card = cards_by_anchor.get(&anchor).copied();')
s = s.replace('    app_server::probe(None).await', '''    let config = FileStore::discover()?.load_config()?;
    let resolved = ResolvedAppServerTarget::resolve(&config.app_server, target)?;
    app_server::probe_target(resolved).await''')
p.write_text(s)
p = Path('src/headless/surfaces.rs'); s = p.read_text()
for name in ['run_status', 'run_attention', 'run_board']:
    a = s.index('pub(super) async fn ' + name + '(')
    b = s.index(') -> Result<i32>', a)
    section = s[a:b]
    if 'fixture_10k: bool, format:' in section:
        section = section.replace('fixture_10k: bool, format:', 'fixture_10k: bool, target: Option<&str>, format:')
    else:
        section = section.replace('    fixture_10k: bool,\n', '    fixture_10k: bool,\n    target: Option<&str>,\n')
    s = s[:a] + section + s[b:]
s = s.replace('super::build_work_snapshot(fake, fixture_10k)', 'super::build_work_snapshot(fake, fixture_10k, target)')
p.write_text(s)
# Preflight uses the exact same pure validation function, before initialization.
replace('src/runtime_cli.rs', ') => headless_options(&words[2..]),', ') => codex_tui::headless::validate_options(words[1], &words[2..]),')
replace('src/runtime_cli.rs', 'Some("status") => headless_options(&words[1..]),', 'Some("status") => codex_tui::headless::validate_options("status", &words[1..]),')
replace('src/runtime_cli.rs', '            headless_options(&words[2..])', '            let command = match words[0] { "thread" => "threads", "worktree" => "worktrees", other => other };\n            codex_tui::headless::validate_options(command, &words[2..])')
between('src/runtime_cli.rs', 'fn headless_options(words:', 'fn options(', '')
p = Path('src/runtime_cli.rs'); s = p.read_text(); s = s.replace('Read-only aliases: status,', 'Registry-derived commands accept --target NAME; otherwise use the configured active target.\nGit/Goal/Forge are not collected in those scoped snapshots.\nRead-only aliases: status,'); p.write_text(s)
# Extend existing actual-binary usage tests, including side-effect-free refusals.
replace('tests/cli_contract.rs', '        vec!["headless", "threads", "--json", "--json"],', '''        vec!["headless", "threads", "--json", "--json"],
        vec!["headless", "threads", "--target"],
        vec!["attention", "list", "--target", "--help"],
        vec!["status", "--fake", "--target", "remote"],
        vec!["forge", "status", "--target", "remote"],
        vec!["worktree", "list", "--target", "remote"],''')
p = Path('tests/cli_contract.rs'); p.write_text(p.read_text() + r'''
#[test]
fn headless_json_declares_unobserved_sources_instead_of_claiming_full_health() {
    for command in ["threads", "work", "status", "attention", "board"] {
        let home = TempDir::new().unwrap();
        let output = invoke(&["headless", command, "--fake", "--json"], &home);
        assert!(output.status.success(), "{output:?}");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let coverage = &report["source"]["coverage"];
        assert_eq!(coverage["scope"], "registry-local");
        assert_eq!(coverage["allSourcesObserved"], false);
        for source in ["git", "goal", "forge"] { assert_eq!(coverage[source], "not-collected"); }
        assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
    }
}

#[test]
fn unknown_headless_target_is_blocked_and_never_falls_back_to_local() {
    let home = TempDir::new().unwrap();
    #[cfg(windows)]
    for directory in ["AppData/Roaming", "AppData/Local"] { std::fs::create_dir_all(home.path().join(directory)).unwrap(); }
    let output = invoke(&["thread", "list", "--target", "not-configured", "--json"], &home);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["source"]["coverage"]["registry"], "unavailable");
    assert!(report["source"]["error"].as_str().unwrap().contains("unknown App Server target"), "{report}");
}

#[cfg(target_os = "linux")]
#[test]
fn configured_headless_target_uses_the_same_resolver_as_interactive_mode() {
    let home = TempDir::new().unwrap();
    let directory = home.path().join("config/codex-tui");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("config.toml"), "[app_server]\nactive = 'configured-remote'\n").unwrap();
    let output = invoke(&["thread", "list", "--json"], &home);
    assert_eq!(output.status.code(), Some(3));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["source"]["error"].as_str().unwrap().contains("configured-remote"), "{report}");
}
''')
