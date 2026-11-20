//! Collapsed Agent counts are a visual hint; actionable child rows keep their own ID.
use super::*;

pub(super) fn display_group_title(
    app: &AppState,
    index: usize,
    groups: &[(Option<usize>, usize, usize)],
) -> String {
    let title = sanitize_inline(app.threads[index].display_title());
    let Some(&(parent, descendants, attention)) = groups.get(index) else {
        return title;
    };
    if parent.is_some() {
        return format!("↳ {title}");
    }
    if descendants == 0 {
        return title;
    }
    if app.language.is_simplified_chinese() {
        format!("{title} · 子{descendants}/待{attention}")
    } else {
        format!("{title} · {descendants} agents/{attention} need")
    }
}
