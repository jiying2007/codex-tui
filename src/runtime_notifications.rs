use anyhow::{Context, Result, anyhow};
use codex_tui::{
    app::AppState,
    notification::{
        NotificationMode, NotificationObservation, NotificationTracker,
    },
};
use std::{
    io::{self, Write},
    process::Stdio,
    time::Duration,
};
use tokio::{
    process::Command,
    sync::mpsc,
    task::JoinHandle,
    time::timeout,
};

const NOTIFICATION_QUEUE_CAPACITY: usize = 32;
const NOTIFICATION_NOTICE_CAPACITY: usize = 16;
const OS_NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
struct Delivery {
    title: String,
    body: String,
}

pub(crate) struct RuntimeNotifications {
    mode: NotificationMode,
    tracker: NotificationTracker,
    command_tx: Option<mpsc::Sender<Delivery>>,
    notice_rx: mpsc::Receiver<String>,
    task: Option<JoinHandle<()>>,
}

impl RuntimeNotifications {
    pub(crate) fn start(mode: NotificationMode) -> Self {
        let (notice_tx, notice_rx) = mpsc::channel(NOTIFICATION_NOTICE_CAPACITY);
        if mode == NotificationMode::Off {
            return Self {
                mode,
                tracker: NotificationTracker::default(),
                command_tx: None,
                notice_rx,
                task: None,
            };
        }

        let (command_tx, command_rx) = mpsc::channel(NOTIFICATION_QUEUE_CAPACITY);
        let task = tokio::spawn(run_actor(mode, command_rx, notice_tx));
        Self {
            mode,
            tracker: NotificationTracker::default(),
            command_tx: Some(command_tx),
            notice_rx,
            task: Some(task),
        }
    }

    pub(crate) fn seed(&mut self, app: &AppState) {
        let observation =
            NotificationObservation::from_projection(&app.threads, &app.work_cards, &app.goals);
        let _ = self.tracker.advance(observation);
    }

    pub(crate) fn observe(&mut self, app: &AppState) -> Result<usize> {
        let observation =
            NotificationObservation::from_projection(&app.threads, &app.work_cards, &app.goals);
        let events = self.tracker.advance(observation);
        if self.mode == NotificationMode::Off {
            return Ok(0);
        }

        let Some(tx) = self.command_tx.as_ref() else {
            return Err(anyhow!("notification dispatcher is unavailable"));
        };
        let mut dispatched = 0usize;
        for event in events {
            let (title, body) = event.text(app.language);
            tx.try_send(Delivery { title, body })
                .map_err(|error| match error {
                    mpsc::error::TrySendError::Full(_) => {
                        anyhow!("notification dispatcher queue is full")
                    }
                    mpsc::error::TrySendError::Closed(_) => {
                        anyhow!("notification dispatcher is unavailable")
                    }
                })?;
            dispatched = dispatched.saturating_add(1);
        }
        Ok(dispatched)
    }

    pub(crate) fn try_notice(&mut self) -> Option<String> {
        self.notice_rx.try_recv().ok()
    }
}

impl Drop for RuntimeNotifications {
    fn drop(&mut self) {
        self.command_tx.take();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn run_actor(
    mode: NotificationMode,
    mut command_rx: mpsc::Receiver<Delivery>,
    notice_tx: mpsc::Sender<String>,
) {
    while let Some(delivery) = command_rx.recv().await {
        let result = match mode {
            NotificationMode::Off => Ok(()),
            NotificationMode::Terminal => ring_terminal(),
            NotificationMode::Os => match deliver_os(&delivery).await {
                Ok(()) => Ok(()),
                Err(error) => {
                    let _ = ring_terminal();
                    Err(error)
                }
            },
        };
        if let Err(error) = result {
            let _ = notice_tx.try_send(format!(
                "notification delivery degraded; terminal bell fallback used: {error:#}"
            ));
        }
    }
}

fn ring_terminal() -> Result<()> {
    let mut stdout = io::stdout();
    stdout.write_all(b"\x07").context("write terminal bell")?;
    stdout.flush().context("flush terminal bell")
}

#[cfg(target_os = "linux")]
async fn deliver_os(delivery: &Delivery) -> Result<()> {
    run_notification_command(
        Command::new("notify-send")
            .arg("--app-name=codex-tui")
            .arg(&delivery.title)
            .arg(&delivery.body),
        "notify-send",
    )
    .await
}

#[cfg(target_os = "macos")]
async fn deliver_os(delivery: &Delivery) -> Result<()> {
    let mut command = Command::new("osascript");
    command
        .env("CODEX_TUI_NOTIFICATION_TITLE", &delivery.title)
        .env("CODEX_TUI_NOTIFICATION_BODY", &delivery.body)
        .arg("-e")
        .arg(
            "display notification (system attribute \"CODEX_TUI_NOTIFICATION_BODY\") \
             with title (system attribute \"CODEX_TUI_NOTIFICATION_TITLE\")",
        );
    run_notification_command(&mut command, "osascript").await
}

#[cfg(target_os = "windows")]
async fn deliver_os(delivery: &Delivery) -> Result<()> {
    let mut command = Command::new("powershell.exe");
    command
        .env("CODEX_TUI_NOTIFICATION_TITLE", &delivery.title)
        .env("CODEX_TUI_NOTIFICATION_BODY", &delivery.body)
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(
            "$ErrorActionPreference='Stop'; \
             [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType=WindowsRuntime] > $null; \
             [Windows.UI.Notifications.ToastNotification, Windows.UI.Notifications, ContentType=WindowsRuntime] > $null; \
             $xml=[Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
             $text=$xml.GetElementsByTagName('text'); \
             $null=$text.Item(0).AppendChild($xml.CreateTextNode($env:CODEX_TUI_NOTIFICATION_TITLE)); \
             $null=$text.Item(1).AppendChild($xml.CreateTextNode($env:CODEX_TUI_NOTIFICATION_BODY)); \
             $toast=[Windows.UI.Notifications.ToastNotification]::new($xml); \
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('codex-tui').Show($toast);",
        );
    run_notification_command(&mut command, "PowerShell toast").await
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
async fn deliver_os(_delivery: &Delivery) -> Result<()> {
    Err(anyhow!("OS notifications are unsupported on this platform"))
}

async fn run_notification_command(command: &mut Command, label: &str) -> Result<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = timeout(OS_NOTIFICATION_TIMEOUT, command.status())
        .await
        .with_context(|| format!("{label} timed out after 3s"))?
        .with_context(|| format!("spawn {label}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("{label} exited with {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_tui::{backend::CodexBackend, backend::FakeBackend};

    #[tokio::test]
    async fn off_mode_seeds_without_creating_a_dispatcher() {
        let mut runtime = RuntimeNotifications::start(NotificationMode::Off);
        let snapshot = FakeBackend::seeded().snapshot();
        let mut app = AppState::new(snapshot.threads);
        runtime.seed(&app);
        assert!(runtime.command_tx.is_none());

        app.work_cards.clear();
        assert_eq!(runtime.observe(&app).expect("observe"), 0);
        assert!(runtime.try_notice().is_none());
    }

    #[test]
    fn notification_channels_are_bounded() {
        let source = include_str!("runtime_notifications.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        assert!(production.contains("mpsc::channel(NOTIFICATION_QUEUE_CAPACITY)"));
        assert!(production.contains("mpsc::channel(NOTIFICATION_NOTICE_CAPACITY)"));
        assert!(production.contains("try_send"));
        assert!(production.contains("kill_on_drop(true)"));
        assert!(production.contains("OS_NOTIFICATION_TIMEOUT"));
    }
}
