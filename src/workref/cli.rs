//! Minimal manual CLI for G2/G3. No Feishu/TAPD API, Codex invocation or Git writes.
use super::*;
use crate::sqlite_store::SqliteStore;
use anyhow::{Context, Result};
use std::collections::BTreeMap;

const COMMANDS: &[&str] = &[
    "add",
    "session",
    "contract",
    "evidence",
    "handoff",
    "ack",
    "list",
    "show",
    "handoff-text",
];

enum Command {
    Add {
        key: String,
        item: WorkItem,
    },
    Session {
        key: String,
        alias: String,
        session: SessionRef,
    },
    Contract {
        key: String,
        contract: ContractRef,
    },
    Evidence {
        key: String,
        evidence: EvidenceRef,
    },
    Handoff {
        key: String,
        from: String,
        to: String,
        summary: String,
    },
    Ack {
        key: String,
        index: usize,
        receipt: String,
    },
    List {
        json: bool,
    },
    Show {
        key: String,
    },
    HandoffText {
        key: String,
        index: usize,
    },
}

impl Command {
    fn write(&self) -> bool {
        !matches!(
            self,
            Self::List { .. } | Self::Show { .. } | Self::HandoffText { .. }
        )
    }
}

fn fields(
    args: &[String],
    required: &[&str],
    optional: &[&str],
    allow_json: bool,
) -> Result<(BTreeMap<String, String>, bool), String> {
    let mut result = BTreeMap::new();
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        if flag == "--json" && allow_json {
            if json {
                return Err("duplicate --json".into());
            }
            json = true;
            index += 1;
            continue;
        }
        if !required.contains(&flag) && !optional.contains(&flag) {
            return Err(format!("unknown workref option: {flag}"));
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("{flag} requires a value"))?;
        if value.trim().is_empty() || value.starts_with("--") {
            return Err(format!("{flag} requires a nonempty value"));
        }
        if result.insert(flag.to_string(), value.clone()).is_some() {
            return Err(format!("duplicate option: {flag}"));
        }
        index += 2;
    }
    for key in required {
        if !result.contains_key(*key) {
            return Err(format!("missing workref option {key}"));
        }
    }
    Ok((result, json))
}

fn take(fields: &BTreeMap<String, String>, key: &str) -> String {
    fields.get(key).cloned().unwrap_or_default()
}

fn parse(args: &[String]) -> Result<Command, String> {
    let (verb, args) = args
        .split_first()
        .ok_or_else(|| format!("workref needs a subcommand: {}", COMMANDS.join(", ")))?;
    let cmd = match verb.as_str() {
        "add" => {
            let (m, _) = fields(
                args,
                &["--key", "--provider", "--space", "--type", "--id"],
                &["--url"],
                false,
            )?;
            let key = take(&m, "--key");
            key_valid(&key).map_err(|e| e.to_string())?;
            let item = WorkItem::new(
                take(&m, "--provider"),
                take(&m, "--space"),
                take(&m, "--type"),
                take(&m, "--id"),
                m.get("--url").cloned(),
            )
            .map_err(|e| e.to_string())?;
            Command::Add { key, item }
        }
        "session" => {
            let (m, _) = fields(
                args,
                &[
                    "--key", "--alias", "--target", "--home", "--thread", "--repo", "--role",
                ],
                &[],
                false,
            )?;
            let session = SessionRef {
                target_identity: take(&m, "--target"),
                codex_home: take(&m, "--home"),
                thread_id: take(&m, "--thread"),
                repository: take(&m, "--repo"),
                role: take(&m, "--role"),
            };
            session.validate().map_err(|e| e.to_string())?;
            Command::Session {
                key: take(&m, "--key"),
                alias: take(&m, "--alias"),
                session,
            }
        }
        "contract" => {
            let (m, _) = fields(
                args,
                &["--key", "--kind", "--source", "--revision"],
                &["--git-sha"],
                false,
            )?;
            let contract = ContractRef {
                kind: take(&m, "--kind"),
                source: take(&m, "--source"),
                revision: take(&m, "--revision"),
                git_sha: m.get("--git-sha").cloned(),
            };
            contract.validate().map_err(|e| e.to_string())?;
            Command::Contract {
                key: take(&m, "--key"),
                contract,
            }
        }
        "evidence" => {
            let (m, _) = fields(
                args,
                &[
                    "--key",
                    "--kind",
                    "--source",
                    "--revision",
                    "--result",
                    "--environment",
                ],
                &[],
                false,
            )?;
            let evidence = EvidenceRef {
                kind: take(&m, "--kind"),
                source: take(&m, "--source"),
                revision: take(&m, "--revision"),
                result: take(&m, "--result"),
                environment: take(&m, "--environment"),
            };
            evidence.validate().map_err(|e| e.to_string())?;
            Command::Evidence {
                key: take(&m, "--key"),
                evidence,
            }
        }
        "handoff" => {
            let (m, _) = fields(args, &["--key", "--from", "--to", "--summary"], &[], false)?;
            let summary = take(&m, "--summary");
            bounded("handoff summary", &summary, 800).map_err(|e| e.to_string())?;
            Command::Handoff {
                key: take(&m, "--key"),
                from: take(&m, "--from"),
                to: take(&m, "--to"),
                summary,
            }
        }
        "ack" => {
            let (m, _) = fields(args, &["--key", "--index", "--receipt"], &[], false)?;
            let index = take(&m, "--index")
                .parse::<usize>()
                .map_err(|_| "--index must be an unsigned integer")?;
            let receipt = take(&m, "--receipt");
            bounded("receipt", &receipt, 2048).map_err(|e| e.to_string())?;
            Command::Ack {
                key: take(&m, "--key"),
                index,
                receipt,
            }
        }
        "list" => {
            let (_, json) = fields(args, &[], &[], true)?;
            Command::List { json }
        }
        "show" => {
            let (m, _) = fields(args, &["--key"], &[], true)?;
            Command::Show {
                key: take(&m, "--key"),
            }
        }
        "handoff-text" => {
            let (m, _) = fields(args, &["--key", "--index"], &[], false)?;
            let index = take(&m, "--index")
                .parse::<usize>()
                .map_err(|_| "--index must be an unsigned integer")?;
            Command::HandoffText {
                key: take(&m, "--key"),
                index,
            }
        }
        _ => return Err(format!("unknown workref command: {verb}")),
    };
    let key = match &cmd {
        Command::Add { key, .. }
        | Command::Session { key, .. }
        | Command::Contract { key, .. }
        | Command::Evidence { key, .. }
        | Command::Handoff { key, .. }
        | Command::Ack { key, .. }
        | Command::Show { key }
        | Command::HandoffText { key, .. } => Some(key.as_str()),
        Command::List { .. } => None,
    };
    if let Some(key) = key {
        key_valid(key).map_err(|e| e.to_string())?;
    }
    Ok(cmd)
}

/// Used before any state discovery, backend probing or TTY access.
pub fn validate_cli(args: &[String]) -> Result<(), String> {
    parse(args).map(|_| ())
}

pub fn run_cli(args: &[String]) -> Result<i32> {
    let command = parse(args).map_err(anyhow::Error::msg)?;
    let store = SqliteStore::discover()?;
    store
        .health()
        .context("check local SQLite before G2/G3 reference operation")?;
    let _writer = if command.write() {
        Some(
            store
                .acquire_local_writer()
                .context("exclusive local workref writer")?,
        )
    } else {
        None
    };
    let mut ledger = store.load_workref_ledger()?;
    match command {
        Command::List { json } => {
            if json {
                println!("{}", serde_json::to_string_pretty(&ledger)?);
            } else {
                for (key, item) in &ledger.items {
                    println!(
                        "{}  {}:{}:{} [{}]  sessions={} contracts={} evidence={} handoffs={}",
                        crate::text::sanitize_inline(key),
                        item.provider,
                        crate::text::sanitize_inline(&item.space),
                        crate::text::sanitize_inline(&item.external_id),
                        item.kind,
                        item.sessions.len(),
                        item.contracts.len(),
                        item.evidence.len(),
                        item.handoffs.len()
                    );
                }
            }
            return Ok(0);
        }
        Command::Show { key } => {
            let item = ledger
                .items
                .get(&key)
                .ok_or_else(|| anyhow::anyhow!("unknown workref item"))?;
            println!("{}", serde_json::to_string_pretty(item)?);
            return Ok(0);
        }
        Command::HandoffText { key, index } => {
            let item = ledger
                .items
                .get(&key)
                .ok_or_else(|| anyhow::anyhow!("unknown workref item"))?;
            let handoff = item
                .handoffs
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("unknown handoff index"))?;
            println!(
                "WORKREF: {} · {} / {}",
                item.provider, item.space, item.external_id
            );
            println!("SOURCE: {} -> {}", handoff.from_alias, handoff.to_alias);
            println!(
                "SUMMARY: {}",
                crate::text::sanitize_inline(&handoff.summary)
            );
            for contract in &item.contracts {
                println!(
                    "CONTRACT: {} {} @ {}{}",
                    contract.kind,
                    crate::text::sanitize_inline(&contract.source),
                    crate::text::sanitize_inline(&contract.revision),
                    contract
                        .git_sha
                        .as_deref()
                        .map(|sha| format!(" git={sha}"))
                        .unwrap_or_default()
                );
            }
            println!(
                "HANDOFF STATUS: {} (manual copy is not delivery)",
                handoff.status
            );
            return Ok(0);
        }
        Command::Add { key, item } => ledger.add_item(key, item)?,
        Command::Session {
            key,
            alias,
            session,
        } => ledger.add_session(&key, alias, session)?,
        Command::Contract { key, contract } => ledger.add_contract(&key, contract)?,
        Command::Evidence { key, evidence } => ledger.add_evidence(&key, evidence)?,
        Command::Handoff {
            key,
            from,
            to,
            summary,
        } => {
            let index = ledger.prepare_handoff(&key, from, to, summary)?;
            println!(
                "Prepared handoff #{index}; manually copy with workref handoff-text. NOT delivered."
            );
        }
        Command::Ack {
            key,
            index,
            receipt,
        } => ledger.acknowledge_handoff(&key, index, receipt)?,
    }
    store.save_workref_ledger(&ledger)?;
    println!("Local reference stored; manual/unverified. No external workflow updated.");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_validation_is_strict_and_does_not_need_a_store() {
        let v = |parts: &[&str]| {
            validate_cli(
                &parts
                    .iter()
                    .map(|part| (*part).to_string())
                    .collect::<Vec<_>>(),
            )
        };
        assert!(v(&["list"]).is_ok());
        assert!(v(&["list", "--json"]).is_ok());
        assert!(
            v(&[
                "add",
                "--key",
                "BUG1",
                "--provider",
                "tapd",
                "--space",
                "app",
                "--type",
                "bug",
                "--id",
                "12",
                "--url",
                "https://www.tapd.cn/"
            ])
            .is_ok()
        );
        assert!(
            v(&[
                "add",
                "--key",
                "BUG1",
                "--provider",
                "tapd",
                "--space",
                "app",
                "--type",
                "bug",
                "--id",
                "12"
            ])
            .is_err()
        );
        assert!(
            v(&[
                "session",
                "--key",
                "BUG1",
                "--alias",
                "soc",
                "--target",
                "host-a",
                "--home",
                "/home/a",
                "--thread",
                "uuid",
                "--repo",
                "soc-app",
                "--role",
                "developer"
            ])
            .is_ok()
        );
        assert!(
            v(&[
                "handoff",
                "--key",
                "BUG1",
                "--from",
                "soc",
                "--to",
                "mcu",
                "--summary",
                "copy summary"
            ])
            .is_ok()
        );
        assert!(v(&["ack", "--key", "BUG1", "--index", "no", "--receipt", "r"]).is_err());
        assert!(v(&["show", "--key", "BUG1", "--json", "--json"]).is_err());
        assert!(v(&["list", "--target", "host-a"]).is_err());
    }
}
