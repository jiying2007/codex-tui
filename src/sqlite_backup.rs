//! Consistent, bounded online backups. Never copy a live SQLite main file:
//! committed data may exist only in its WAL even after a checkpoint attempt.
use anyhow::{Context, Result};
use rusqlite::{
    Connection,
    backup::{Backup, StepResult},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

const BACKUP_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn snapshot(source: &Connection, destination: &Path) -> Result<()> {
    snapshot_with_timeout(source, destination, BACKUP_TIMEOUT)
}

fn snapshot_with_timeout(source: &Connection, destination: &Path, timeout: Duration) -> Result<()> {
    let mut target = Connection::open(destination).context("open staged SQLite backup")?;
    // Retry on our own deadline rather than stacking SQLite's busy wait per page.
    target.busy_timeout(Duration::ZERO)?;
    source.busy_timeout(Duration::ZERO)?;
    let started = Instant::now();
    {
        let backup = Backup::new(source, &mut target).context("start consistent SQLite backup")?;
        loop {
            match backup.step(128).context("copy SQLite backup pages")? {
                StepResult::Done => break,
                StepResult::More => {}
                StepResult::Busy | StepResult::Locked => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                _ => anyhow::bail!("unexpected SQLite backup step result"),
            }
            anyhow::ensure!(
                started.elapsed() < timeout,
                "SQLite backup deadline exceeded; source remains unchanged"
            );
        }
    }
    // Publish a self-contained database; do not leave a destination WAL behind.
    target.execute_batch("PRAGMA journal_mode = DELETE;")?;
    target
        .close()
        .map_err(|(_, error)| error)
        .context("close staged SQLite backup")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exclusive_writer_has_a_bounded_backup_deadline() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.db");
        let writer = Connection::open(&path).unwrap();
        writer
            .execute_batch("CREATE TABLE data(n); INSERT INTO data VALUES(1);")
            .unwrap();
        let source = Connection::open(&path).unwrap();
        writer
            .execute_batch("BEGIN EXCLUSIVE; INSERT INTO data VALUES(2);")
            .unwrap();
        let started = Instant::now();
        let error = snapshot_with_timeout(
            &source,
            &root.path().join("backup.db"),
            Duration::from_millis(25),
        )
        .unwrap_err();
        assert!(error.to_string().contains("deadline"), "{error:#}");
        assert!(started.elapsed() < Duration::from_secs(1));
        writer.execute_batch("ROLLBACK").unwrap();
        let count: i64 = writer
            .query_row("SELECT count(*) FROM data", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
