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
                local
                    .scopes
                    .get(&operator_scope_key(identity, source.codex_home.as_deref()))
            })
            .cloned()
            .unwrap_or_default()
    };
    for thread in threads {
        thread.pinned = overlay.pins.contains(&thread.id.0);
        thread.alias = overlay.aliases.get(&thread.id.0).cloned();
        thread
            .attention
            .retain(|reason| *reason != AttentionReason::MarkedUnread);
        if overlay.marked_unread.contains(&thread.id.0) {
            thread.attention.push(AttentionReason::MarkedUnread);
        }
    }
}

/// Project current scoped snooze only. Historical rows stay on disk.
pub(super) fn scope_work_preferences(planning: &mut PlanningSnapshot, source: &SourceStatus) {
    if source.scope == "fixture" {
        return;
    }
    let scope = source
        .operator_target_key
        .as_deref()
        .filter(|_| source.connected)
        .zip(
            source
                .codex_home
                .as_deref()
                .filter(|home| !home.trim().is_empty()),
        )
        .map(|(target, home)| operator_scope_key(target, Some(home)));
    planning.cards.retain_mut(|card| match card.anchor.kind {
        SourceKind::CodexThread => false,
        SourceKind::ScopedSession => {
            let Some(thread) = scope
                .as_deref()
                .and_then(|scope| card.anchor.thread_in_scope(scope))
            else {
                return false;
            };
            let mut current = crate::planning::WorkCardRecord::implicit_thread(
                &crate::domain::ThreadId::new(thread),
            );
            current.overlay.snooze_until_unix_ms = card.overlay.snooze_until_unix_ms;
            *card = current;
            true
        }
        _ => true,
    });
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

    #[test]
    fn headless_only_projects_current_session_snooze_not_old_completion() {
        use crate::{
            domain::ThreadId,
            planning::{SourceRef, WorkCardRecord},
        };
        let id = ThreadId::new("same-id");
        let mut old = WorkCardRecord::implicit_thread(&id);
        old.overlay.done_at_unix_ms = Some(1);
        old.overlay.snooze_until_unix_ms = Some(50);
        let mut scoped = old.clone();
        scoped.anchor =
            SourceRef::scoped_session(&operator_scope_key("target", Some("/home/a")), &id.0);
        let planning = PlanningSnapshot {
            cards: vec![old, scoped],
            ..Default::default()
        };
        let mut source = SourceStatus::unavailable(Some("A".into()));
        source.connected = true;
        source.operator_target_key = Some("target".into());
        source.codex_home = Some("/home/a".into());
        let mut current = planning.clone();
        scope_work_preferences(&mut current, &source);
        assert_eq!(current.cards.len(), 1);
        assert_eq!(current.cards[0].anchor, SourceRef::codex_thread(&id));
        assert_eq!(current.cards[0].overlay.snooze_until_unix_ms, Some(50));
        assert!(current.cards[0].overlay.done_at_unix_ms.is_none());
        source.codex_home = Some("/home/b".into());
        let mut other = planning;
        scope_work_preferences(&mut other, &source);
        assert!(other.cards.is_empty());
    }
}
