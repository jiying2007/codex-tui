//! Every event-loop outcome restores the terminal before attempting durable exit.
use anyhow::Result;

pub(crate) fn finish<T>(
    terminal: T,
    outcome: Result<()>,
    save: impl FnOnce() -> Result<()>,
) -> Result<()> {
    drop(terminal);
    combine(outcome, save())
}

pub(crate) fn combine(outcome: Result<()>, saved: Result<()>) -> Result<()> {
    match (outcome, saved) {
        (Ok(()), result) | (result, Ok(())) => result,
        (Err(runtime), Err(storage)) => {
            Err(runtime.context(format!("final state save also failed: {storage:#}")))
        }
    }
}

/// Installed before raw mode. Signal callbacks only wake the runtime; terminal
/// restoration and the existing bounded final save stay in ordinary Rust code.
/// Headless subcommands never install these interactive-only handlers.
pub(crate) struct ExitSignal {
    #[cfg(unix)]
    signals: [tokio::signal::unix::Signal; 3],
    #[cfg(windows)]
    interrupt: tokio::signal::windows::CtrlC,
}

impl ExitSignal {
    pub(crate) fn install() -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Ok(Self {
                signals: [
                    signal(SignalKind::terminate())?,
                    signal(SignalKind::hangup())?,
                    signal(SignalKind::interrupt())?,
                ],
            })
        }
        #[cfg(windows)]
        {
            Ok(Self {
                interrupt: tokio::signal::windows::ctrl_c()?,
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Ok(Self {})
        }
    }

    pub(crate) async fn recv(&mut self) {
        #[cfg(unix)]
        {
            let [terminate, hangup, interrupt] = &mut self.signals;
            tokio::select! {
                _ = terminate.recv() => {},
                _ = hangup.recv() => {},
                _ = interrupt.recv() => {},
            }
        }
        #[cfg(windows)]
        {
            self.interrupt.recv().await;
        }
        #[cfg(not(any(unix, windows)))]
        {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Terminal<'a>(&'a Cell<bool>);
    impl Drop for Terminal<'_> {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    #[test]
    fn event_loop_error_still_restores_then_flushes() {
        let restored = Cell::new(false);
        let saved = Cell::new(false);
        let result = finish(
            Terminal(&restored),
            Err(anyhow::anyhow!("injected input failure")),
            || {
                assert!(restored.get());
                saved.set(true);
                Ok(())
            },
        );
        assert!(saved.get());
        assert!(format!("{:#}", result.unwrap_err()).contains("injected input failure"));
    }
    #[test]
    fn concurrent_runtime_and_storage_failures_are_both_retained() {
        let error = combine(
            Err(anyhow::anyhow!("render failed")),
            Err(anyhow::anyhow!("disk full")),
        )
        .unwrap_err();
        let text = format!("{error:#}");
        assert!(text.contains("render failed") && text.contains("disk full"));
    }
    #[test]
    fn normal_exit_still_reports_final_save_failure() {
        assert!(finish((), Ok(()), || Err(anyhow::anyhow!("disk full"))).is_err());
    }
}
