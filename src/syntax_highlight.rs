use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    hash::{DefaultHasher, Hash, Hasher},
};
use two_face::{
    re_exports::syntect::{
        easy::HighlightLines,
        highlighting::{FontStyle, Style as SyntectStyle},
        parsing::SyntaxSet,
    },
    theme::{EmbeddedThemeName, LazyThemeSet},
};

const MAX_SYNC_HIGHLIGHT_BYTES: usize = 128 * 1024;
const CACHE_CAPACITY: usize = 8;
const REVIEW_SYNTAX: &str = "diff";
const REVIEW_THEME: &str = "ansi";

#[derive(Clone, Debug, PartialEq, Eq)]
struct CacheKey {
    revision: u64,
    syntax: String,
    theme: String,
    content_hash: u64,
}

#[derive(Clone, Debug)]
struct CacheEntry {
    key: CacheKey,
    lines: Vec<Line<'static>>,
}

pub struct SyntaxHighlighter {
    syntaxes: SyntaxSet,
    themes: LazyThemeSet,
    cache: VecDeque<CacheEntry>,
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntaxHighlighter {
    pub fn new() -> Self {
        Self {
            syntaxes: two_face::syntax::extra_newlines(),
            themes: two_face::theme::extra(),
            cache: VecDeque::with_capacity(CACHE_CAPACITY),
        }
    }

    pub fn highlight_lines(
        &mut self,
        lines: &[String],
        revision: u64,
        syntax_hint: &str,
    ) -> Option<Vec<Line<'static>>> {
        if lines.is_empty() {
            return Some(vec![]);
        }

        let byte_len = lines
            .iter()
            .fold(0usize, |total, line| total.saturating_add(line.len() + 1));
        if byte_len > MAX_SYNC_HIGHLIGHT_BYTES {
            return None;
        }

        let key = CacheKey {
            revision,
            syntax: syntax_hint.to_ascii_lowercase(),
            theme: REVIEW_THEME.into(),
            content_hash: content_hash(lines),
        };
        if let Some(index) = self.cache.iter().position(|entry| entry.key == key) {
            let entry = self.cache.remove(index)?;
            let lines = entry.lines.clone();
            self.cache.push_front(entry);
            return Some(lines);
        }

        let syntax = self
            .syntaxes
            .find_syntax_by_extension(syntax_hint)
            .or_else(|| self.syntaxes.find_syntax_by_name(syntax_hint))?;
        let theme = &self.themes[EmbeddedThemeName::Ansi];
        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut highlighted = Vec::with_capacity(lines.len());

        for line in lines {
            let source = format!("{line}\n");
            let ranges = highlighter.highlight_line(&source, &self.syntaxes).ok()?;
            let spans = ranges
                .into_iter()
                .filter_map(|(style, text)| {
                    let text = text.strip_suffix('\n').unwrap_or(text);
                    (!text.is_empty()).then(|| Span::styled(text.to_string(), ratatui_style(style)))
                })
                .collect::<Vec<_>>();
            highlighted.push(Line::from(spans));
        }

        self.cache.push_front(CacheEntry {
            key,
            lines: highlighted.clone(),
        });
        while self.cache.len() > CACHE_CAPACITY {
            self.cache.pop_back();
        }
        Some(highlighted)
    }

    #[cfg(test)]
    fn cache_len(&self) -> usize {
        self.cache.len()
    }
}

thread_local! {
    static REVIEW_HIGHLIGHTER: RefCell<SyntaxHighlighter> =
        RefCell::new(SyntaxHighlighter::new());
}

pub fn highlight_review_diff(
    lines: &[String],
    revision: u64,
) -> Option<Vec<Line<'static>>> {
    REVIEW_HIGHLIGHTER.with(|highlighter| {
        highlighter
            .borrow_mut()
            .highlight_lines(lines, revision, REVIEW_SYNTAX)
    })
}

pub const fn sync_highlight_limit_bytes() -> usize {
    MAX_SYNC_HIGHLIGHT_BYTES
}

pub fn asset_acknowledgements_markdown() -> String {
    two_face::acknowledgement::listing().to_md()
}

fn content_hash(lines: &[String]) -> u64 {
    let mut hasher = DefaultHasher::new();
    lines.hash(&mut hasher);
    hasher.finish()
}

fn ratatui_style(style: SyntectStyle) -> Style {
    let mut output = Style::default().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ));
    if style.font_style.contains(FontStyle::BOLD) {
        output = output.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        output = output.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        output = output.add_modifier(Modifier::UNDERLINED);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_highlighting_preserves_text_and_caches_by_revision() {
        let mut highlighter = SyntaxHighlighter::new();
        let lines = vec![
            "diff --git a/src/main.rs b/src/main.rs".into(),
            "@@ -1,2 +1,2 @@".into(),
            "-let old = true;".into(),
            "+let new = false;".into(),
        ];

        let first = highlighter
            .highlight_lines(&lines, 7, "diff")
            .expect("diff highlight");
        assert_eq!(first.len(), lines.len());
        let flattened = first
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(flattened, lines);
        assert_eq!(highlighter.cache_len(), 1);

        let second = highlighter
            .highlight_lines(&lines, 7, "diff")
            .expect("cached diff highlight");
        assert_eq!(second, first);
        assert_eq!(highlighter.cache_len(), 1);
    }

    #[test]
    fn unsupported_syntax_falls_back_to_plain_text() {
        let mut highlighter = SyntaxHighlighter::new();
        assert!(
            highlighter
                .highlight_lines(&["text".into()], 1, "definitely-not-a-syntax")
                .is_none()
        );
    }

    #[test]
    fn embedded_asset_acknowledgements_are_retained() {
        let markdown = asset_acknowledgements_markdown();
        assert!(markdown.contains("# Syntaxes"));
        assert!(markdown.contains("# Themes"));
        assert!(markdown.len() > 1_000);
    }

    #[test]
    fn large_content_never_highlights_synchronously() {
        let mut highlighter = SyntaxHighlighter::new();
        let lines = vec!["x".repeat(sync_highlight_limit_bytes() + 1)];
        assert!(highlighter.highlight_lines(&lines, 1, "diff").is_none());
        assert_eq!(highlighter.cache_len(), 0);
    }
}
