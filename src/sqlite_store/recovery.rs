//! Offline recovery image preservation. No SQLite handle opens the damaged live image.
use super::*;
use std::io::ErrorKind;

pub(super) fn ensure_distinct_source(source: &Path, live: &Path) -> Result<()> {
    let source = source.canonicalize().context("resolve recovery source")?;
    match live.canonicalize() {
        Ok(live) => anyhow::ensure!(
            source != live,
            "recovery source must differ from the live database"
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("resolve live database"),
    }
    Ok(())
}

pub(super) fn validate_recovery_database(path: &Path) -> Result<i64> {
    anyhow::ensure!(
        path.is_file(),
        "recovery database does not exist: {}",
        path.display()
    );
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open read-only recovery SQLite {}", path.display()))?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    anyhow::ensure!(
        version == DB_SCHEMA_VERSION,
        "unsupported recovery SQLite schema version {version}"
    );
    validate_operator_envelope(&conn)?;
    let integrity = quick_check(&conn)?;
    anyhow::ensure!(
        integrity == "ok",
        "recovery SQLite quick_check failed: {integrity}"
    );
    Ok(version)
}

/// The SQLite schema and the embedded operator envelope evolve independently.
/// Deserialize only the envelope version: unknown future fields must not be
/// silently discarded and overwritten by a current-version writer.
pub(super) fn validate_operator_envelope(conn: &Connection) -> Result<()> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Envelope {
        schema_version: u32,
    }
    let json: Option<String> = conn
        .query_row(
            "SELECT value_json FROM operator_state WHERE key=?1",
            [OPERATOR_STATE_KEY],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(json) = json {
        let envelope: Envelope = serde_json::from_str(&json).context("decode operator envelope")?;
        anyhow::ensure!(
            envelope.schema_version == 1,
            "unsupported LocalStore schemaVersion {}",
            envelope.schema_version
        );
    }
    Ok(())
}

fn sidecar(db: &Path, suffix: &str) -> Result<PathBuf> {
    let mut name = db
        .file_name()
        .context("SQLite path has no file name")?
        .to_os_string();
    name.push(suffix);
    Ok(db.with_file_name(name))
}

fn present_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.file_type().is_file(),
                "SQLite image component is not a regular file: {}",
                path.display()
            );
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("inspect SQLite image {}", path.display()))
        }
    }
}

fn image_paths(db: &Path) -> Result<Vec<PathBuf>> {
    ["", "-wal", "-shm", "-journal"]
        .into_iter()
        .map(|suffix| sidecar(db, suffix))
        .collect()
}

pub(super) fn move_sqlite_image(source: &Path, destination: &Path) -> Result<()> {
    move_image_with(source, destination, |from, to| fs::rename(from, to))
}

fn move_image_with<F>(source: &Path, destination: &Path, mut rename: F) -> Result<()>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    // Inspect the complete raw image before the first rename. Preserve rollback
    // journals too; directories and symlinks must not be reinterpreted as SQLite.
    let mut pairs = Vec::new();
    for (from, to) in image_paths(source)?
        .into_iter()
        .zip(image_paths(destination)?)
    {
        let exists = present_file(&from)?;
        anyhow::ensure!(
            !present_file(&to)?,
            "SQLite preservation destination already exists: {}",
            to.display()
        );
        if exists {
            pairs.push((from, to));
        }
    }
    for (index, (from, to)) in pairs.iter().enumerate() {
        if let Err(error) = rename(from, to) {
            let mut rollback_errors = Vec::new();
            for (original, moved) in pairs[..index].iter().rev() {
                // Never overwrite a path that appeared during the failed move.
                let result = (|| -> Result<()> {
                    anyhow::ensure!(
                        !present_file(original)?,
                        "rollback path occupied: {}",
                        original.display()
                    );
                    fs::rename(moved, original).context("rollback SQLite image move")
                })();
                if let Err(error) = result {
                    rollback_errors.push(format!("{error:#}"));
                }
            }
            anyhow::bail!(
                "preserve SQLite image {} -> {}: {error}; rollback failures: {rollback_errors:?}",
                from.display(),
                to.display()
            );
        }
    }
    Ok(())
}

pub(super) fn preserve_sqlite_image(db: &Path, label: &str) -> Result<Option<PathBuf>> {
    let components = image_paths(db)?;
    let present = components
        .iter()
        .map(|path| present_file(path))
        .collect::<Result<Vec<_>>>()?;
    if !present[0] {
        anyhow::ensure!(
            !present.iter().any(|value| *value),
            "orphan SQLite sidecars exist; offline recovery refused without modifying them"
        );
        return Ok(None);
    }
    let parent = db.parent().context("SQLite path has no parent")?;
    let reservation = tempfile::Builder::new()
        .prefix(&format!("state-v2.sqlite3.{label}."))
        .tempdir_in(parent)
        .context("reserve unique SQLite recovery image directory")?;
    let previous = reservation
        .path()
        .join(db.file_name().context("SQLite file name")?);
    // Retain the reservation even on incomplete rollback: never delete the only
    // remaining copy of a component during TempDir cleanup.
    let retained = reservation.keep();
    move_sqlite_image(db, &previous)
        .with_context(|| format!("recovery image retained at {}", retained.display()))?;
    Ok(Some(previous))
}

pub(super) fn restore_preserved_sqlite_image(db: &Path, preserved: &Path) -> Result<()> {
    // The caller already moved any failed candidate aside. An unexpected new
    // image is interference, not permission to replace another writer's state.
    move_sqlite_image(preserved, db)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sidecar_move_failure_rolls_back_the_entire_completed_prefix() {
        let root = tempfile::tempdir().unwrap();
        let live = root.path().join("live.db");
        let saved = root.path().join("saved.db");
        fs::write(&live, b"main").unwrap();
        fs::write(sidecar(&live, "-wal").unwrap(), b"wal").unwrap();
        let mut calls = 0;
        let error = move_image_with(&live, &saved, |from, to| {
            calls += 1;
            if calls == 2 {
                return Err(std::io::Error::new(ErrorKind::PermissionDenied, "injected"));
            }
            fs::rename(from, to)
        })
        .unwrap_err();
        assert!(error.to_string().contains("injected"));
        assert_eq!(fs::read(&live).unwrap(), b"main");
        assert_eq!(fs::read(sidecar(&live, "-wal").unwrap()).unwrap(), b"wal");
        assert!(!saved.exists());
    }
    #[test]
    fn existing_destination_never_gets_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let live = root.path().join("live.db");
        let saved = root.path().join("saved.db");
        fs::write(&live, b"live").unwrap();
        fs::write(&saved, b"saved").unwrap();
        assert!(move_sqlite_image(&live, &saved).is_err());
        assert_eq!(fs::read(live).unwrap(), b"live");
        assert_eq!(fs::read(saved).unwrap(), b"saved");
    }
    #[test]
    fn repeated_preservation_uses_distinct_reservations_and_keeps_journals() {
        let root = tempfile::tempdir().unwrap();
        let live = root.path().join("live.db");
        fs::write(&live, b"first").unwrap();
        fs::write(sidecar(&live, "-journal").unwrap(), b"raw-journal").unwrap();
        let first = preserve_sqlite_image(&live, "pre-restore")
            .unwrap()
            .unwrap();
        fs::write(&live, b"second").unwrap();
        let second = preserve_sqlite_image(&live, "pre-restore")
            .unwrap()
            .unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&first).unwrap(), b"first");
        assert_eq!(fs::read(second).unwrap(), b"second");
        assert_eq!(
            fs::read(sidecar(&first, "-journal").unwrap()).unwrap(),
            b"raw-journal"
        );
    }
}
