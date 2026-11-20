//! Do not project unscoped historical pins/aliases on unrelated App Server Targets.
use super::*;
use crate::store::{ScopedOperatorState, operator_scope_key};

pub(super) fn apply_local_operator_state(
    threads: &mut [ThreadSummary],
    local: &LocalStateV1,
    source: &SourceStatus,
) {
    let overlay = if source.scope == "fixture" {
        ScopedOperatorState::legacy(local)
    } else {
        source
            .operator_target_key
            .as_deref()
            .filter(|_| source.connected)
            .and_then(|identity| {
                local.scopes
                    .get(&operator_scope_key(identity, source.codex_home.as_deref()))
            })
            .cloned()
            .unwrap_or_default()
    };
    for thread in threads {
        thread.pinned = overlay.pins.contains(&thread.id.0);
        thread.alias = overlay.aliases.get(&thread.id.0).cloned();
        thread.attention.retain(|reason| *reason != AttentionReason::MarkedUnread);
        if overlay.marked_unread.contains(&thread.id.0) {
            thread.attention.push(AttentionReason::MarkedUnread);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    #[test]
    fn read_only_target_must_not_apply_unscoped_legacy_preferences() {
        let mut threads = FakeBackend::seeded().snapshot().threads;
        let id = threads[0].id.0.clone();
        let mut local = LocalStateV1::default();
        local.pins.insert(id.clone());
        local.aliases.insert(id.clone(), "legacy only".into());
        let mut source = SourceStatus::unavailable(Some("A".into()));
        source.connected = true;
        source.operator_target_key = Some("stable target A".into());
        source.codex_home = Some("/home/a".into());
        apply_local_operator_state(&mut threads, &local, &source);
        assert!(!threads[0].pinned);
        assert!(threads[0].alias.is_none());

        let key = operator_scope_key("stable target A", Some("/home/a"));
        let mut scoped = ScopedOperatorState::default();
        scoped.pins.insert(id.clone());
        scoped.aliases.insert(id, "scope A".into());
        local.scopes.insert(key, scoped);
        apply_local_operator_state(&mut threads, &local, &source);
        assert!(threads[0].pinned);
        assert_eq!(threads[0].alias.as_deref(), Some("scope A"));

        source.codex_home = Some("/home/b".into());
        apply_local_operator_state(&mut threads, &local, &source);
        assert!(!threads[0].pinned);
        assert!(threads[0].alias.is_none());
    }
}
