//! Read-only, provenance-aware identity hints for one selected Codex thread.
use crate::app::AppState;
use crate::domain::ThreadSummary;
use crate::i18n::UiLanguage;
use crate::text::sanitize_inline;
use ratatui::text::Line;

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

/// Human-readable, read-only startup-snapshot hint. A missing/invalid identity
/// never licenses a cross-Target/archived session association.
pub(super) fn manual_workref_heading(app: &AppState, thread_id: &str) -> Option<String> {
    if app.workref_unavailable {
        return Some(if app.language.is_simplified_chinese() {
            "本地事项关联不可用（需检查存储）".into()
        } else {
            "Manual work links unavailable (check store)".into()
        });
    }
    let links = app.manual_workrefs_for_thread(thread_id);
    let (key, _, item) = links.first()?;
    let tail = if links.len() > 1 {
        format!(" +{}", links.len() - 1)
    } else {
        String::new()
    };
    let text = if app.language.is_simplified_chinese() {
        format!(
            "关联（手工未核验）: {key} {}#{}{tail}",
            item.provider, item.external_id
        )
    } else {
        format!(
            "Linked (manual/unverified): {key} {}#{}{tail}",
            item.provider, item.external_id
        )
    };
    Some(sanitize_inline(&text))
}

/// Bounded projection. Separate test outcomes, contract versions and handoff
/// state are only copied from the manual ledger; none imply upstream approval.
pub(super) fn manual_workref_lines(app: &AppState, thread_id: &str) -> Vec<Line<'static>> {
    if app.workref_unavailable {
        return vec![Line::from(if app.language.is_simplified_chinese() {
            "G2/G3 本地关联不可用 · 不视作没有事项"
        } else {
            "G2/G3 links UNAVAILABLE · not an empty result"
        })];
    }
    let links = app.manual_workrefs_for_thread(thread_id);
    if links.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Line::from(if app.language.is_simplified_chinese() {
        format!("G2/G3 关联 {} · 手工/未核验", links.len())
    } else {
        format!("G2/G3 linked {} · manual/unverified", links.len())
    })];
    for (key, alias, item) in links.iter().take(2) {
        lines.push(Line::from(sanitize_inline(&format!(
            "{} [{}] · {} {}#{}",
            key, alias, item.provider, item.kind, item.external_id,
        ))));
        let contracts = item
            .contracts
            .iter()
            .take(2)
            .map(|contract| format!("{}@{}", contract.kind, contract.revision))
            .collect::<Vec<_>>();
        let evidence = item
            .evidence
            .iter()
            .take(3)
            .map(|e| format!("{}={}", e.kind, e.result))
            .collect::<Vec<_>>();
        let prepared = item
            .handoffs
            .iter()
            .filter(|handoff| handoff.status == "prepared")
            .count();
        let acknowledged = item
            .handoffs
            .iter()
            .filter(|handoff| handoff.status == "acknowledged")
            .count();
        let contract = if contracts.is_empty() {
            "none".into()
        } else {
            contracts.join(",")
        };
        let result = if evidence.is_empty() {
            "none".into()
        } else {
            evidence.join(",")
        };
        lines.push(Line::from(sanitize_inline(&format!(
            "  C:{contract} · E:{result} · handoff prepared={prepared} ack={acknowledged}",
        ))));
    }
    if links.len() > 2 {
        lines.push(Line::from(if app.language.is_simplified_chinese() {
            format!("其余 {} 条请用 workref find 查看", links.len() - 2)
        } else {
            format!("{} more: use workref find", links.len() - 2)
        }));
    }
    lines
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
