use super::*;
use crate::domain::{classify_cwd, classify_cwd_without_io};

impl AppState {
    // Only a process started over stdio by this TUI has proved its cwd is local.
    // WebSocket/Unix-socket peers may report a directory with the same spelling.
    pub(super) fn app_server_target_is_remote(&self) -> bool {
        let source = &self.backend_status.source;
        source.starts_with("codex-app-server[target=")
            && !source.ends_with(" transport=stdio]")
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
        let locality = self.cwd_locality(cwd);
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
                    if remote { CwdLocality::ForeignUnix } else { classify_cwd(&cwd) }
                });
            }
        }
    }

}
