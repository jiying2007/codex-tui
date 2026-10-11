use anyhow::{Context, Result, anyhow};
use codex_tui::{
    app::AppState,
    notification::{NotificationMode, NotificationObservation, NotificationTracker},
};
use std::{
    collections::VecDeque,
    io::{self, Write},
    process::Stdio,
    time::Duration,
};
use tokio::{process::Command, sync::mpsc, task::JoinHandle, time::timeout};

const NOTIFICATION_QUEUE_CAPACITY: usize = 32;
const NOTIFICATION_PENDING_CAPACITY: usize = 128;
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
    pending: VecDeque<Delivery>,
    dispatcher_notice_emitted: bool,
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
                pending: VecDeque::new(),
                dispatcher_notice_emitted: false,
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
            pending: VecDeque::new(),
            dispatcher_notice_emitted: false,
            notice_rx,
            task: Some(task),
        }
    }

    pub(crate) fn seed(&mut self, app: &AppState) {
        if self.mode == NotificationMode::Off {
            return;
        }
        let observation =
            NotificationObservation::from_projection(&app.threads, &app.work_cards, &app.goals);
        let _ = self.tracker.advance(observation);
    }

    pub(crate) fn observe(&mut self, app: &AppState) -> Result<usize> {
        if self.mode == NotificationMode::Off {
            return Ok(0);
        }
        anyhow::ensure!(
            self.command_tx.is_some(),
            "notification dispatcher is unavailable"
        );
        let mut dispatched = self.flush_pending()?;
        let observation =
            NotificationObservation::from_projection(&app.threads, &app.work_cards, &app.goals);
        let events = self.tracker.advance(observation);

        let mut overflow = 0usize;
        for event in events {
            if self.pending.len() == NOTIFICATION_PENDING_CAPACITY {
                overflow = overflow.saturating_add(1);
                continue;
            }
            let (title, body) = event.text(app.language);
            self.pending.push_back(Delivery { title, body });
        }
        dispatched = dispatched.saturating_add(self.flush_pending()?);
        anyhow::ensure!(
            overflow == 0,
            "notification backlog exceeded {NOTIFICATION_PENDING_CAPACITY} pending events; {overflow} additional events were not queued; inspect Mission Control Needs You"
        );
        Ok(dispatched)
    }

    fn flush_pending(&mut self) -> Result<usize> {
        if self.mode == NotificationMode::Off || self.pending.is_empty() {
            return Ok(0);
        }
        let Some(tx) = self.command_tx.as_ref() else {
            return Err(anyhow!("notification dispatcher is unavailable"));
        };
        let mut dispatched = 0usize;
        while let Some(delivery) = self.pending.pop_front() {
            match tx.try_send(delivery) {
                Ok(()) => dispatched = dispatched.saturating_add(1),
                Err(mpsc::error::TrySendError::Full(delivery)) => {
                    self.pending.push_front(delivery);
                    break;
                }
                Err(mpsc::error::TrySendError::Closed(delivery)) => {
                    self.pending.push_front(delivery);
                    return Err(anyhow!("notification dispatcher is unavailable"));
                }
            }
        }
        Ok(dispatched)
    }

    pub(crate) fn try_notice(&mut self) -> Option<String> {
        // Called by the existing 16ms TUI loop even without a fresh planning
        // projection, so temporarily full channels cannot strand prior edges.
        if let Err(error) = self.flush_pending()
            && !self.dispatcher_notice_emitted
        {
            self.dispatcher_notice_emitted = true;
            return Some(format!("notification dispatcher unavailable: {error:#}"));
        }
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
fn linux_notification_command(delivery: &Delivery) -> Command {
    let mut command = Command::new("notify-send");
    command
        .arg("--app-name=codex-tui")
        .arg("--")
        .arg(&delivery.title)
        .arg(&delivery.body);
    command
}

#[cfg(target_os = "linux")]
async fn deliver_os(delivery: &Delivery) -> Result<()> {
    let mut command = linux_notification_command(delivery);
    run_notification_command(&mut command, "notify-send").await
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
    use codex_tui::{
        backend::{CodexBackend, FakeBackend},
        domain::RuntimeStatus,
    };
    use std::collections::BTreeSet;

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

    #[cfg(target_os = "linux")]
    #[test]
    fn notify_send_treats_option_looking_body_as_data() {
        let delivery = Delivery {
            title: "codex-tui test".into(),
            body: "--icon=spoofed".into(),
        };
        let command = linux_notification_command(&delivery);
        let args = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            vec![
                "--app-name=codex-tui",
                "--",
                "codex-tui test",
                "--icon=spoofed",
            ]
        );
    }

    #[tokio::test]
    async fn temporary_queue_pressure_retries_all_completion_edges_once() {
        let mut runtime = RuntimeNotifications::start(NotificationMode::Terminal);
        runtime.task.take().unwrap().abort();
        let (tx, mut rx) = mpsc::channel(1);
        runtime.command_tx = Some(tx);

        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        for thread in &mut app.threads {
            thread.runtime = RuntimeStatus::Working;
        }
        runtime.seed(&app);
        for thread in &mut app.threads {
            thread.runtime = RuntimeStatus::Ready;
        }
        let count = app.threads.len();
        assert_eq!(runtime.observe(&app).unwrap(), 1);
        assert_eq!(runtime.pending.len(), count - 1);
        assert_eq!(runtime.observe(&app).unwrap(), 0);

        let mut seen = BTreeSet::new();
        for _ in 0..count {
            let delivery = rx.try_recv().expect("queued completion");
            assert!(seen.insert(delivery.body), "duplicate completion delivered");
            assert!(runtime.try_notice().is_none());
        }
        assert_eq!(seen.len(), count);
        assert!(runtime.pending.is_empty());
    }

    #[tokio::test]
    async fn saturated_pending_buffer_is_bounded_and_reports_missing_alerts() {
        let mut runtime = RuntimeNotifications::start(NotificationMode::Terminal);
        runtime.task.take().unwrap().abort();
        let (tx, _rx) = mpsc::channel(1);
        runtime.command_tx = Some(tx);

        let mut app = AppState::new(FakeBackend::scaled(160).snapshot().threads);
        for thread in &mut app.threads {
            thread.runtime = RuntimeStatus::Working;
        }
        runtime.seed(&app);
        for thread in &mut app.threads {
            thread.runtime = RuntimeStatus::Ready;
        }
        let error = runtime.observe(&app).expect_err("must report overflow");
        assert!(error.to_string().contains("additional events were not queued"));
        assert!(runtime.pending.len() <= NOTIFICATION_PENDING_CAPACITY);
        assert!(!runtime.pending.is_empty());
    }

    #[tokio::test]
    async fn closed_channel_keeps_unsent_edge_and_reports_degradation_once() {
        let mut runtime = RuntimeNotifications::start(NotificationMode::Terminal);
        runtime.task.take().unwrap().abort();
        let (tx, rx) = mpsc::channel(1);
        runtime.command_tx = Some(tx);
        drop(rx);

        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        for thread in &mut app.threads {
            thread.runtime = RuntimeStatus::Working;
        }
        runtime.seed(&app);
        for thread in &mut app.threads {
            thread.runtime = RuntimeStatus::Ready;
        }
        assert!(runtime.observe(&app).is_err());
        let pending = runtime.pending.len();
        assert!(pending > 0);
        assert!(runtime.try_notice().unwrap().contains("unavailable"));
        assert!(runtime.try_notice().is_none());
        assert_eq!(runtime.pending.len(), pending);
    }

    #[test]
    fn notification_channels_are_bounded() {
        let source = include_str!("runtime_notifications.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        assert!(production.contains("mpsc::channel(NOTIFICATION_QUEUE_CAPACITY)"));
        assert!(production.contains("NOTIFICATION_PENDING_CAPACITY"));
        assert!(production.contains("self.pending.push_front(delivery)"));
        assert!(production.contains("mpsc::channel(NOTIFICATION_NOTICE_CAPACITY)"));
        assert!(production.contains("try_send"));
        assert!(production.contains("kill_on_drop(true)"));
        assert!(production.contains("OS_NOTIFICATION_TIMEOUT"));
    }
}

#[cfg(test)]
mod disabled_tracking_tests {
    use super::*;
    use codex_tui::{
        backend::{CodexBackend, FakeBackend},
        domain::RuntimeStatus,
    };

    fn working() -> AppState {
        let mut app = AppState::new(FakeBackend::seeded().snapshot().threads);
        for thread in &mut app.threads {
            thread.runtime = RuntimeStatus::Working;
        }
        app
    }

    fn assert_untracked(runtime: &mut RuntimeNotifications, mut app: AppState) {
        for thread in &mut app.threads {
            thread.runtime = RuntimeStatus::Ready;
        }
        let observation = NotificationObservation::from_projection(&app.threads, &[], &app.goals);
        assert!(
            runtime.tracker.advance(observation).is_empty(),
            "disabled mode retained and compared the entire previous registry"
        );
    }

    #[tokio::test]
    async fn off_seed_does_not_retain_registry_projections() {
        let mut runtime = RuntimeNotifications::start(NotificationMode::Off);
        let app = working();
        runtime.seed(&app);
        assert_untracked(&mut runtime, app);
    }

    #[tokio::test]
    async fn off_observe_does_not_retain_registry_projections() {
        let mut runtime = RuntimeNotifications::start(NotificationMode::Off);
        let app = working();
        assert_eq!(runtime.observe(&app).unwrap(), 0);
        assert_untracked(&mut runtime, app);
    }
}
