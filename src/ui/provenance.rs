//! Read-only, provenance-aware identity hints for one selected Codex thread.
use crate::domain::ThreadSummary;
use crate::i18n::UiLanguage;
use crate::text::sanitize_inline;

pub(super) fn model_provider_label(thread: &ThreadSummary, language: UiLanguage) -> String {
    let unknown = if language.is_simplified_chinese() {
        "未知"
    } else {
        "unknown"
    };
    let model = thread.metadata.model.as_deref().unwrap_or(unknown);
    let provider = thread
        .metadata
        .lineage
        .model_provider
        .as_deref()
        .unwrap_or(unknown);
    format!("{} / {}", sanitize_inline(model), sanitize_inline(provider))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    #[test]
    fn provider_is_distinct_from_model_and_safe_to_print() {
        let mut thread = FakeBackend::seeded().snapshot().threads.remove(0);
        thread.metadata.model = Some("gpt-6".into());
        thread.metadata.lineage.model_provider = Some("provider\u{001b}[2J".into());
        assert_eq!(
            model_provider_label(&thread, UiLanguage::English),
            "gpt-6 / provider�[2J"
        );
    }

    #[test]
    fn missing_provider_must_not_be_inferred_from_model() {
        let mut thread = FakeBackend::seeded().snapshot().threads.remove(0);
        thread.metadata.model = Some("model-label".into());
        assert_eq!(
            model_provider_label(&thread, UiLanguage::SimplifiedChinese),
            "model-label / 未知"
        );
    }
}
