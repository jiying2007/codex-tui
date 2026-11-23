//! Read-only per-target Codex locator; does not grant Resume or writes.
use super::*;

pub(super) fn thread_row(thread: &ThreadSummary) -> ThreadRow {
    let mut attention = thread
        .attention
        .iter()
        .map(|reason| reason.label().to_string())
        .collect::<Vec<_>>();
    attention.sort();
    attention.dedup();
    ThreadRow {
        id: thread.id.0.clone(),
        source_kind: crate::text::sanitize_inline(&thread.metadata.source),
        model_provider: thread.metadata.lineage.model_provider.clone(),
        session_id: thread.metadata.lineage.session_id.clone(),
        parent_thread_id: thread.metadata.lineage.parent_thread_id.clone(),
        attached_to_thread_id: thread.metadata.lineage.attached_to_thread_id.clone(),
        forked_from_id: thread.metadata.lineage.forked_from_id.clone(),
        workspace: thread.workspace.clone(),
        title: thread.display_title().to_string(),
        runtime: thread.runtime.label().to_ascii_lowercase(),
        attention,
        pinned: thread.pinned,
    }
}

pub(super) fn print_source(source: &SourceStatus) {
    println!(
        "source: {} connected={}",
        headless_field(&source.source),
        source.connected
    );
    println!(
        "target: {} transport={} scope={}",
        headless_field(source.target_id.as_deref().unwrap_or("none")),
        headless_field(source.transport.as_deref().unwrap_or("none")),
        source.scope
    );
    println!(
        "codex-home: {}",
        headless_field(source.codex_home.as_deref().unwrap_or("<unknown>"))
    );
    println!("registry-complete: {}", source.registry_complete);
    if let Some(error) = &source.error {
        println!("source-error: {}", headless_field(error));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    #[test]
    fn lineage_and_provider_are_explicit_not_inferred_from_thread_id() {
        let mut thread = FakeBackend::seeded().snapshot().threads.remove(0);
        thread.metadata.source = "exec".into();
        thread.metadata.lineage.session_id = Some("family".into());
        thread.metadata.lineage.parent_thread_id = Some("actual-parent".into());
        thread.metadata.lineage.attached_to_thread_id = Some("attached-parent".into());
        thread.metadata.lineage.forked_from_id = Some("fork-source".into());
        thread.metadata.lineage.model_provider = Some("provider".into());
        let row = thread_row(&thread);
        assert_eq!(row.source_kind, "exec");
        assert_eq!(row.session_id.as_deref(), Some("family"));
        assert_eq!(row.parent_thread_id.as_deref(), Some("actual-parent"));
        assert_eq!(row.attached_to_thread_id.as_deref(), Some("attached-parent"));
        assert_eq!(row.forked_from_id.as_deref(), Some("fork-source"));
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(json["modelProvider"], "provider");
        assert!(json.get("resumeVerified").is_none());
    }
    #[test]
    fn optional_lineage_fields_are_omitted_when_unavailable() {
        let thread = FakeBackend::seeded().snapshot().threads.remove(0);
        let value = serde_json::to_value(thread_row(&thread)).unwrap();
        assert!(value.get("parentThreadId").is_none());
        assert!(value.get("attachedToThreadId").is_none());
        assert!(value.get("forkedFromId").is_none());
        assert!(value.get("modelProvider").is_none());
        assert!(value.get("sessionId").is_none());
    }
}
