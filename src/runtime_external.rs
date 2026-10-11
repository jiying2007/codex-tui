use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;

fn selected_editor_target(worktree_root: &str, relative_path: &str) -> Result<PathBuf> {
    let selected = Path::new(relative_path);
    anyhow::ensure!(
        !relative_path.is_empty()
            && selected
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "selected Git change must be a worktree-relative path"
    );
    let root = std::fs::canonicalize(worktree_root)
        .with_context(|| format!("resolve worktree root {worktree_root:?}"))?;
    anyhow::ensure!(
        root.is_dir(),
        "selected Git worktree root is not a directory"
    );
    let target = std::fs::canonicalize(root.join(selected))
        .with_context(|| format!("resolve changed file {relative_path:?}"))?;
    anyhow::ensure!(
        target != root && target.starts_with(&root),
        "selected Git change escapes worktree root"
    );
    Ok(target)
}

pub(crate) fn open_external_editor(worktree_root: &str, relative_path: &str) -> Result<()> {
    let editor =
        std::env::var_os("CODEX_TUI_EDITOR").unwrap_or_else(|| std::ffi::OsString::from("code"));
    if editor.to_string_lossy().trim().is_empty() {
        anyhow::bail!("CODEX_TUI_EDITOR is empty");
    }
    let path = selected_editor_target(worktree_root, relative_path)?;
    let mut command = std::process::Command::new(editor);
    command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    codex_tui::detached_process::spawn(&mut command)
        .map(|_| ())
        .map_err(Into::into)
}

pub(crate) fn open_external_url(url: &str) -> Result<()> {
    anyhow::ensure!(
        url.starts_with("https://") || url.starts_with("http://"),
        "external target must be an HTTP(S) URL"
    );

    let mut command = if let Some(browser) = std::env::var_os("CODEX_TUI_BROWSER") {
        anyhow::ensure!(
            !browser.to_string_lossy().trim().is_empty(),
            "CODEX_TUI_BROWSER is empty"
        );
        let mut command = std::process::Command::new(browser);
        command.arg(url);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = std::process::Command::new("rundll32.exe");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    codex_tui::detached_process::spawn(&mut command)
        .map(|_| ())
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_editor_path_is_scoped_to_the_real_worktree_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(repo.join("src")).expect("create subdir");
        let file = repo.join("src").join("changed.rs");
        std::fs::write(&file, "pub fn changed() {}").expect("write changed file");
        std::fs::write(temp.path().join("outside.rs"), "secret").expect("outside");

        assert_eq!(
            selected_editor_target(repo.to_str().unwrap(), "src/changed.rs").unwrap(),
            file.canonicalize().unwrap()
        );
        for invalid in [
            "../outside.rs",
            "src/../../outside.rs",
            "./src/changed.rs",
            "",
        ] {
            assert!(
                selected_editor_target(repo.to_str().unwrap(), invalid).is_err(),
                "unexpected editor target: {invalid:?}"
            );
        }
        assert!(selected_editor_target(repo.to_str().unwrap(), file.to_str().unwrap()).is_err());
        assert!(selected_editor_target(repo.to_str().unwrap(), "absent.rs").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn selected_editor_path_rejects_symlink_escape_but_accepts_in_tree_links() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        std::fs::create_dir(&repo).expect("create repo");
        let inner = repo.join("inner.rs");
        let outside = temp.path().join("private.rs");
        std::fs::write(&inner, "inside").unwrap();
        std::fs::write(&outside, "outside").unwrap();
        symlink(&outside, repo.join("escape.rs")).unwrap();
        symlink(&inner, repo.join("inner-link.rs")).unwrap();
        assert!(selected_editor_target(repo.to_str().unwrap(), "escape.rs").is_err());
        assert_eq!(
            selected_editor_target(repo.to_str().unwrap(), "inner-link.rs").unwrap(),
            inner.canonicalize().unwrap()
        );
    }

    #[test]
    fn external_url_rejects_non_http_targets_before_spawning() {
        assert!(open_external_url("file:///tmp/secret").is_err());
        assert!(open_external_url("javascript:alert(1)").is_err());
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "runtime_external/tests.rs"]
mod process_tests;
