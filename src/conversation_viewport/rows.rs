use std::{
    iter::{Peekable, Skip},
    ops::Range,
};
use unicode_segmentation::{GraphemeIndices, UnicodeSegmentation};

// A newline-heavy body must not allocate one range/String descriptor per row.
const STRIDE: usize = 128;

#[derive(Clone, Debug)]
pub(super) struct Rows {
    width: u16,
    pub count: usize,
    checkpoints: Vec<usize>,
}

impl Rows {
    pub fn new(text: &str, width: u16) -> Self {
        let mut checkpoints = Vec::new();
        let mut count = 0;
        for row in WrappedRows::new(text, width, 0) {
            if count % STRIDE == 0 {
                checkpoints.push(row.start);
            }
            count += 1;
        }
        Self {
            width,
            count,
            checkpoints,
        }
    }

    pub fn from<'a>(&self, text: &'a str, row: usize) -> Skip<WrappedRows<'a>> {
        let row = row.min(self.count.saturating_sub(1));
        WrappedRows::new(text, self.width, self.checkpoints[row / STRIDE]).skip(row % STRIDE)
    }

    pub fn number_for_byte(&self, text: &str, byte: usize) -> usize {
        let checkpoint = self
            .checkpoints
            .partition_point(|start| *start <= byte)
            .saturating_sub(1);
        let preceding = WrappedRows::new(text, self.width, self.checkpoints[checkpoint])
            .take_while(|row| row.start <= byte)
            .count()
            .saturating_sub(1);
        (checkpoint * STRIDE + preceding).min(self.count.saturating_sub(1))
    }
}

pub(crate) struct WrappedRows<'a> {
    graphemes: Peekable<GraphemeIndices<'a>>,
    base: usize,
    cursor: usize,
    end: usize,
    width: usize,
    finished: bool,
}

impl<'a> WrappedRows<'a> {
    pub fn new(text: &'a str, width: u16, start: usize) -> Self {
        Self {
            graphemes: text[start..].grapheme_indices(true).peekable(),
            base: start,
            cursor: start,
            end: text.len(),
            width: usize::from(width.max(1)),
            finished: false,
        }
    }
}

impl Iterator for WrappedRows<'_> {
    type Item = Range<usize>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        let start = self.cursor;
        let mut columns = 0;
        while let Some(&(relative, grapheme)) = self.graphemes.peek() {
            let offset = self.base + relative;
            if grapheme == "\n" {
                self.graphemes.next();
                self.cursor = offset + 1;
                return Some(start..offset);
            }
            let width = crate::text::display_width(grapheme);
            if columns > 0 && columns + width > self.width {
                self.cursor = offset;
                return Some(start..offset);
            }
            columns += width;
            self.cursor = offset + grapheme.len();
            self.graphemes.next();
        }
        self.finished = true;
        Some(start..self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newline_heavy_layout_is_sparse_and_its_tail_is_reachable() {
        let text = format!("{}TAIL", "\n".repeat(1_048_576));
        let rows = Rows::new(&text, 80);
        assert_eq!(rows.count, 1_048_577);
        assert_eq!(rows.checkpoints.len(), rows.count.div_ceil(STRIDE));
        assert!(rows.checkpoints.capacity() * std::mem::size_of::<usize>() < 140_000);
        let tail = rows.from(&text, rows.count - 1).next().unwrap();
        assert_eq!(&text[tail], "TAIL");
        assert_eq!(rows.number_for_byte(&text, text.len() - 4), rows.count - 1);
    }
}
