use super::super::source::print_source;
use super::*;

pub(super) fn print_status(snapshot: &StatusSnapshot, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshot)?),
        OutputFormat::Text => {
            println!("schema: {}", snapshot.schema);
            println!("degraded: {}", snapshot.degraded);
            print_source(&snapshot.source);
            println!("local-store-scope: {}", snapshot.local_store_scope);
            println!("threads: {}", snapshot.thread_count);
            println!("work: {}", snapshot.work_count);
            println!("needs-you: {}", snapshot.needs_you_count);
            for (stage, count) in &snapshot.stages {
                println!("stage-{}: {count}", headless_field(stage));
            }
            if let Some(error) = &snapshot.local_store_error {
                println!("store-error: {}", headless_field(error));
            }
        }
    }
    Ok(())
}

pub(super) fn print_attention(snapshot: &AttentionSnapshot, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshot)?),
        OutputFormat::Text => {
            println!("schema: {}", snapshot.schema);
            println!("degraded: {}", snapshot.degraded);
            print_source(&snapshot.source);
            println!("local-store-scope: {}", snapshot.local_store_scope);
            if let Some(error) = &snapshot.local_store_error {
                println!("store-error: {}", headless_field(error));
            }
            println!("attention: {}", snapshot.items.len());
            for item in &snapshot.items {
                println!(
                    "{}\t{}\t{}\t{}",
                    headless_field(&item.local_id),
                    headless_field(&item.stage),
                    headless_field(&item.attention.join(",")),
                    headless_field(&item.title)
                );
            }
        }
    }
    Ok(())
}

pub(super) fn print_board(snapshot: &BoardSnapshot, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshot)?),
        OutputFormat::Text => {
            println!("schema: {}", snapshot.schema);
            println!("degraded: {}", snapshot.degraded);
            print_source(&snapshot.source);
            println!("local-store-scope: {}", snapshot.local_store_scope);
            if let Some(error) = &snapshot.local_store_error {
                println!("store-error: {}", headless_field(error));
            }
            for column in &snapshot.columns {
                println!("[{}] {}", headless_field(&column.stage), column.items.len());
                for item in &column.items {
                    println!(
                        "{}\t{}\t{}",
                        headless_field(&item.local_id),
                        headless_field(&item.attention.join(",")),
                        headless_field(&item.title)
                    );
                }
            }
        }
    }
    Ok(())
}

pub(super) fn print_forge(snapshot: &ForgeStatusSnapshot, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshot)?),
        OutputFormat::Text => {
            println!("schema: {}", snapshot.schema);
            println!("degraded: {}", snapshot.degraded);
            println!("scope: {}", snapshot.scope);
            println!("cwd: {}", headless_field(&snapshot.cwd));
            println!(
                "provider: {}",
                headless_field(snapshot.provider.as_deref().unwrap_or("unknown"))
            );
            println!(
                "authenticated: {}",
                snapshot
                    .authenticated
                    .map_or("unknown".into(), |value| value.to_string())
            );
            println!("freshness: {}", headless_field(&snapshot.freshness));
            for capability in &snapshot.capabilities {
                println!(
                    "capability-{}: {}",
                    headless_field(&capability.name),
                    headless_field(&capability.state)
                );
            }
            if let Some(error) = &snapshot.error {
                println!("error: {}", headless_field(error));
            }
        }
    }
    Ok(())
}

pub(super) fn print_worktrees(snapshot: &WorktreeSnapshot, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(snapshot)?),
        OutputFormat::Text => {
            println!("schema: {}", snapshot.schema);
            println!("degraded: {}", snapshot.degraded);
            println!("scope: {}", snapshot.scope);
            println!("worktrees: {}", snapshot.worktrees.len());
            for worktree in &snapshot.worktrees {
                println!(
                    "{}\t{}\t{}",
                    headless_field(worktree.branch.as_deref().unwrap_or("<detached>")),
                    if worktree.adopted {
                        "adopted"
                    } else {
                        "created"
                    },
                    headless_field(&worktree.canonical_path)
                );
            }
            if let Some(error) = &snapshot.local_store_error {
                println!("store-error: {}", headless_field(error));
            }
        }
    }
    Ok(())
}
