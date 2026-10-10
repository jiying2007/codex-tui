use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn sanitize_inline(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if matches!(ch, '\n' | '\r' | '\t') {
                ' '
            } else if ch.is_control()
                || matches!(
                    ch,
                    '\u{061c}'
                        | '\u{200e}'
                        | '\u{200f}'
                        | '\u{2028}'..='\u{202e}'
                        | '\u{2066}'..='\u{206f}'
                        | '\u{feff}'
                )
            {
                '\u{fffd}'
            } else {
                ch
            }
        })
        .collect()
}

pub fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

pub fn truncate_display(value: &str, max_columns: usize) -> String {
    if max_columns == 0 {
        return String::new();
    }

    let value = sanitize_inline(value);
    if display_width(&value) <= max_columns {
        return value;
    }

    let ellipsis = "…";
    let ellipsis_width = display_width(ellipsis);
    if max_columns <= ellipsis_width {
        return ellipsis.to_string();
    }

    let budget = max_columns - ellipsis_width;
    let mut out = String::new();
    let mut used: usize = 0;
    for grapheme in UnicodeSegmentation::graphemes(value.as_str(), true) {
        let width = display_width(grapheme);
        if used.saturating_add(width) > budget {
            break;
        }
        out.push_str(grapheme);
        used = used.saturating_add(width);
    }
    out.push_str(ellipsis);
    out
}

pub fn fit_display(value: &str, columns: usize) -> String {
    let mut value = truncate_display(value, columns);
    let width = display_width(&value);
    if width < columns {
        value.push_str(&" ".repeat(columns - width));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_truncation_uses_terminal_columns() {
        assert_eq!(display_width("中文ABC"), 7);
        assert_eq!(truncate_display("中文ABC", 5), "中文…");
        assert_eq!(display_width(&truncate_display("中文ABC", 5)), 5);
    }

    #[test]
    fn grapheme_clusters_are_never_split() {
        let combined = "e\u{301}clair";
        assert_eq!(truncate_display(combined, 2), "e\u{301}…");

        let family = "👨‍👩‍👧‍👦abc";
        let shortened = truncate_display(family, 3);
        assert!(
            shortened == "👨‍👩‍👧‍👦…" || shortened == "…",
            "unexpected grapheme truncation: {shortened:?}"
        );
        assert!(display_width(&shortened) <= 3);
    }

    #[test]
    fn fit_display_pads_by_visible_width() {
        let fitted = fit_display("中文", 6);
        assert_eq!(display_width(&fitted), 6);
        assert!(fitted.starts_with("中文"));
    }

    #[test]
    fn inline_sanitizer_removes_layout_controls() {
        assert_eq!(sanitize_inline("a\nb\rc\td"), "a b c d");
        assert_eq!(sanitize_inline("a\u{0007}b"), "a\u{fffd}b");
    }

    #[test]
    fn bidi_and_line_separators_cannot_spoof_review_or_thread_chrome() {
        let hostile = format!("safe{}reversed{}hidden{}next", '\u{202e}', '\u{2066}', '\u{2029}');
        let displayed = sanitize_inline(&hostile);
        assert_eq!(displayed, "safe�reversed�hidden�next");
        for control in ['\u{202e}', '\u{2066}', '\u{2029}', '\u{061c}', '\u{200f}'] {
            assert!(!displayed.contains(control));
        }
        // Emoji graphemes still require their ZWJ and combining marks.
        let family = "👨‍👩‍👧‍👦";
        assert_eq!(sanitize_inline(family), family);
    }

    #[test]
    fn zero_width_budget_is_empty() {
        assert_eq!(truncate_display("abc", 0), "");
        assert_eq!(fit_display("abc", 0), "");
    }
}
