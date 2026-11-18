use super::*;
use crate::domain::{classify_cwd, classify_cwd_without_io};

impl AppState {
    // WebSocket/Unix-socket peers may report a cwd with the same local spelling.
    pub(super) fn app_server_target_is_remote(&self) -> bool {
        let source = &self.backend_status.source;
        source.starts_with("codex-app-server[target=") && !source.ends_with(" transport=stdio]")
    }

    pub fn cwd_locality(&self, cwd: &str) -> CwdLocality {
        if self.app_server_target_is_remote() {
            return CwdLocality::ForeignUnix;
        }
        self.cwd_localities
            .get(cwd)
            .copied()
            .unwrap_or_else(|| classify_cwd(cwd))
    }

    pub fn cwd_locality_for_display(&self, cwd: &str) -> Option<CwdLocality> {
        if self.app_server_target_is_remote() {
            return Some(CwdLocality::ForeignUnix);
        }
        self.cwd_localities
            .get(cwd)
            .copied()
            .or_else(|| classify_cwd_without_io(cwd))
    }

    pub(super) fn refresh_cwd_locality(&mut self, cwd: &str) -> CwdLocality {
        // Re-probe the filesystem: a cached result can outlive the directory.
        let locality = if self.app_server_target_is_remote() {
            CwdLocality::ForeignUnix
        } else {
            classify_cwd(cwd)
        };
        self.cwd_localities.insert(cwd.to_string(), locality);
        locality
    }

    pub(super) fn reconcile_cwd_locality_cache(&mut self, fill_missing: bool) {
        let current_cwds = self
            .threads
            .iter()
            .map(|thread| thread.metadata.cwd.clone())
            .collect::<BTreeSet<_>>();
        self.cwd_localities
            .retain(|cwd, _| current_cwds.contains(cwd));

        if fill_missing {
            let remote = self.app_server_target_is_remote();
            for cwd in current_cwds {
                self.cwd_localities.entry(cwd.clone()).or_insert_with(|| {
                    if remote {
                        CwdLocality::ForeignUnix
                    } else {
                        classify_cwd(&cwd)
                    }
                });
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    #[test]
    fn remote_peer_cannot_authorize_local_shell_or_git_with_matching_path() {
        let threads = FakeBackend::seeded().snapshot().threads;
        let id = threads[0].id.clone();
        let mut app = AppState::new(threads);
        let cwd = std::env::current_dir().unwrap().display().to_string();
        app.threads[0].metadata.cwd = cwd.clone();
        app.view = View::Thread(id);
        app.backend_status.source =
            "codex-app-server[target=remote-fixture transport=websocket]".into();

        assert_eq!(app.cwd_locality(&cwd), CwdLocality::ForeignUnix);
        assert!(app.terminal_target_cwd().is_err());
        assert!(reduce(&mut app, Action::OpenReview).is_empty());
        assert!(reduce(&mut app, Action::OpenWorkspace).is_empty());
        assert!(app.git_contexts.is_empty());
    }
}
