//! Distinguish incomplete origin coverage from pending pagination.
use super::*;

pub(super) fn registry_inventory_label(app: &AppState) -> String {
    let partial = app.backend_status.optional_capabilities_missing.iter()
        .any(|capability| capability == "thread/list:known-source-kinds");
    let history = registry_history_label(
        app.language,
        app.backend_status.connected && !app.backend_status.registry_complete && !partial,
        !app.filter.is_empty(),
        app.show_all_history,
    );
    let suffix = if partial {
        if app.language.is_simplified_chinese() {
            " · 来源不全"
        } else {
            " · SOURCES PARTIAL"
        }
    } else {
        ""
    };
    format!("{history}{suffix}")
}
