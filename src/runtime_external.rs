use anyhow::Result;
use std::path::Path;
use std::process::Stdio;

pub(crate) fn open_external_editor(cwd: &str, relative_path: &str) -> Result<()> {
    let editor =
        std::env::var_os("CODEX_TUI_EDITOR").unwrap_or_else(|| std::ffi::OsString::from("code"));
    if editor.to_string_lossy().trim().is_empty() {
        anyhow::bail!("CODEX_TUI_EDITOR is empty");
    }
    let path = Path::new(cwd).join(relative_path);
    if !path.exists() {
        anyhow::bail!("selected path does not exist: {}", path.display());
    }
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
    fn external_url_rejects_non_http_targets_before_spawning() {
        assert!(open_external_url("file:///tmp/secret").is_err());
        assert!(open_external_url("javascript:alert(1)").is_err());
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "runtime_external/tests.rs"]
mod process_tests;
