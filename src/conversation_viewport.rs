//! Bounded, grapheme-aware conversation navigation. Anchors survive page prepends
//! and width changes because they name an item and a UTF-8 byte in its presentation.
use crate::{conversation::ConversationState, domain::ThreadUiState};
use std::sync::{Arc, LazyLock, Mutex};
mod rows;
use rows::Rows;
pub(crate) use rows::WrappedRows;

const MIN_WINDOW_ITEMS: usize = 64;
const MAX_WINDOW_ITEMS: usize = 512;

/// Last painted size is derived layout state, independent in each AppState clone.
#[derive(Debug)]
pub(crate) struct Size(std::sync::atomic::AtomicU32);

impl Size {
    pub fn new(size: (u16, u16)) -> Self {
        Self(std::sync::atomic::AtomicU32::new(
            (u32::from(size.0) << 16) | u32::from(size.1),
        ))
    }
    pub fn get(&self) -> (u16, u16) {
        let packed = self.0.load(std::sync::atomic::Ordering::Relaxed);
        ((packed >> 16) as u16, packed as u16)
    }
    pub fn set(&self, size: (u16, u16)) {
        self.0.store(
            (u32::from(size.0) << 16) | u32::from(size.1),
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

impl Clone for Size {
    fn clone(&self) -> Self {
        Self::new(self.get())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub item_id: String,
    pub turn_id: String,
    pub text: String,
    rows: Rows,
}

struct Cache {
    thread: String,
    revision: u64,
    start: usize,
    end: usize,
    width: u16,
    items: Arc<Vec<Item>>,
}
static CACHE: LazyLock<Mutex<Option<Cache>>> = LazyLock::new(|| Mutex::new(None));

fn format_items(
    conversation: &ConversationState,
    start: usize,
    end: usize,
    width: u16,
) -> Arc<Vec<Item>> {
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(entry) = cache.as_ref()
        && entry.thread == conversation.thread_id.0
        && entry.revision == conversation.presentation_revision()
        && entry.start == start
        && entry.end == end
        && entry.width == width
    {
        return Arc::clone(&entry.items);
    }
    let items = Arc::new(
        conversation.items[start..end]
            .iter()
            .map(|item| {
                let status = item
                    .status
                    .as_deref()
                    .map(|s| format!(" [{}]", crate::text::sanitize_inline(s)))
                    .unwrap_or_default();
                let text = crate::text::sanitize_multiline(&format!(
                    "{:>5}{status} {}",
                    item.kind.label(),
                    item.text
                ));
                Item {
                    item_id: item.item_id.clone(),
                    turn_id: item.turn_id.clone(),
                    rows: Rows::new(&text, width),
                    text,
                }
            })
            .collect(),
    );
    *cache = Some(Cache {
        thread: conversation.thread_id.0.clone(),
        revision: conversation.presentation_revision(),
        start,
        end,
        width,
        items: Arc::clone(&items),
    });
    items
}

pub(crate) struct Viewport {
    items: Arc<Vec<Item>>,
    pub start: usize,
    pub end: usize,
    top: usize,
    rows: usize,
    height: usize,
    total_items: usize,
}

impl Viewport {
    pub fn at_start(&self) -> bool {
        self.start == 0 && self.top == 0
    }

    pub fn visible(&self) -> Vec<(&str, &str, &str)> {
        let mut skip = self.top;
        let mut out = Vec::with_capacity(self.height);
        for item in self.items.iter() {
            if skip >= item.rows.count {
                skip -= item.rows.count;
                continue;
            }
            for row in item.rows.from(&item.text, skip) {
                if out.len() >= self.height {
                    return out;
                }
                out.push((
                    item.turn_id.as_str(),
                    item.item_id.as_str(),
                    &item.text[row],
                ));
            }
            skip = 0;
        }
        out
    }

    /// Unmeasured items outside the bounded window count as one row. The thumb
    /// remains useful for a single long item without laying out the whole history.
    pub fn scrollbar(&self) -> (usize, usize, usize) {
        (
            self.start + self.rows + self.total_items.saturating_sub(self.end),
            self.start + self.top,
            self.height.min(self.rows.saturating_sub(self.top)),
        )
    }

    fn remember(&self, ui: &mut ThreadUiState, row: usize) {
        let mut row = row.min(self.rows.saturating_sub(1));
        for (index, item) in self.items.iter().enumerate() {
            if row < item.rows.count {
                ui.scroll = u16::try_from(self.start + index).unwrap_or(u16::MAX);
                ui.scroll_anchor = Some(item.item_id.clone());
                ui.scroll_turn_id = Some(item.turn_id.clone());
                ui.scroll_offset = item
                    .rows
                    .from(&item.text, row)
                    .next()
                    .map_or(0, |row| row.start);
                return;
            }
            row -= item.rows.count;
        }
        ui.scroll = 0;
        ui.scroll_anchor = None;
        ui.scroll_turn_id = None;
        ui.scroll_offset = 0;
    }
}

pub(crate) fn viewport(
    conversation: &ConversationState,
    ui: &ThreadUiState,
    width: u16,
    height: u16,
) -> Viewport {
    let total = conversation.items.len();
    let capacity = usize::from(height)
        .saturating_mul(4)
        .clamp(MIN_WINDOW_ITEMS, MAX_WINDOW_ITEMS);
    let index = ui
        .scroll_anchor
        .as_deref()
        .and_then(|id| {
            conversation.items.iter().position(|item| {
                item.item_id == id
                    && ui
                        .scroll_turn_id
                        .as_deref()
                        .is_none_or(|turn| item.turn_id == turn)
            })
        })
        .unwrap_or(usize::from(ui.scroll))
        .min(total.saturating_sub(1));
    let start = if ui.follow {
        total.saturating_sub(capacity)
    } else {
        index.saturating_sub(capacity / 2)
    };
    let end = start.saturating_add(capacity).min(total);
    let items = format_items(conversation, start, end, width);
    let rows = items.iter().map(|item| item.rows.count).sum::<usize>();
    let height = usize::from(height);
    let top = if ui.follow {
        rows.saturating_sub(height)
    } else if let Some(item) = items.get(index.saturating_sub(start)) {
        let row = item.rows.number_for_byte(&item.text, ui.scroll_offset);
        items
            .iter()
            .take(index - start)
            .map(|item| item.rows.count)
            .sum::<usize>()
            + row
    } else {
        0
    };
    Viewport {
        items,
        start,
        end,
        top,
        rows,
        height,
        total_items: total,
    }
}

pub(crate) fn scroll(
    conversation: &ConversationState,
    ui: &mut ThreadUiState,
    delta: i16,
    width: u16,
    height: u16,
) {
    let view = viewport(conversation, ui, width, height);
    let row = if delta < 0 {
        view.top.saturating_sub(usize::from(delta.unsigned_abs()))
    } else {
        view.top.saturating_add(delta as usize)
    };
    view.remember(ui, row);
    ui.follow = false;
}

#[cfg(test)]
mod tests;
