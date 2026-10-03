use crate::app::{Action, AppState, reduce};
use crate::app_server::RegistryHandle;
use crate::domain::ThreadId;
use crate::i18n::pick;

pub fn start_thread(app: &mut AppState, registry: Option<&RegistryHandle>, cwd: String) {
    match registry {
        Some(registry) => {
            if let Err(error) = registry.start_thread(cwd) {
                fail(app, "thread/start", error.to_string());
            }
        }
        None => fail_backend_unavailable(app, "thread/start"),
    }
}

pub fn fork_thread(
    app: &mut AppState,
    registry: Option<&RegistryHandle>,
    thread_id: ThreadId,
) {
    match registry {
        Some(registry) => {
            if let Err(error) = registry.fork_thread(thread_id) {
                fail(app, "thread/fork", error.to_string());
            }
        }
        None => fail_backend_unavailable(app, "thread/fork"),
    }
}

fn fail_backend_unavailable(app: &mut AppState, operation: &'static str) {
    fail(
        app,
        operation,
        pick(
            app.language,
            "conversation backend unavailable",
            "会话后端不可用",
        )
        .to_string(),
    );
}

fn fail(app: &mut AppState, operation: &'static str, error: String) {
    reduce(
        app,
        Action::ThreadLifecycleFailed {
            operation: operation.into(),
            error,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    #[test]
    fn unavailable_backend_fails_closed_without_mutating_thread_authority() {
        let threads = FakeBackend::seeded().snapshot().threads;
        let original = threads.len();
        let mut app = AppState::new(threads);
        start_thread(&mut app, None, "/repo".into());
        assert_eq!(app.threads.len(), original);
        assert!(app.mutation_notice.as_deref().is_some_and(|notice| {
            notice.contains("thread/start") && notice.contains("backend")
        }));
    }
}
