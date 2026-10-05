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
