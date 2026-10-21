//! Local, request-bound inspection. This state never owns or resolves a request.
use super::{AppState, InputMode, InteractiveRequest, View};
use crate::conversation_viewport::{Size, WrappedRows};

mod format;
pub(crate) use format::request_lines;

#[derive(Clone, Debug)]
pub(crate) struct RequestDetails {
    request: InteractiveRequest,
    text: String,
    byte_offset: usize,
    at_end: bool,
    size: Size,
}

impl RequestDetails {
    fn window(&self, width: u16, height: u16) -> (usize, usize) {
        let mut total = 0usize;
        let mut anchor = 0;
        for row in WrappedRows::new(&self.text, width, 0) {
            if row.start <= self.byte_offset {
                anchor = total;
            }
            total += 1;
        }
        let last = total.saturating_sub(usize::from(height.max(1)));
        (if self.at_end { last } else { anchor.min(last) }, total)
    }

    /// Only visible strings are collected; even newline-heavy requests have no
    /// per-row allocation or u16 scroll limit. The byte anchor survives reflow.
    pub(crate) fn visible(&self, width: u16, height: u16) -> (Vec<&str>, usize, usize) {
        self.size.set((width, height));
        let (top, total) = self.window(width, height);
        let lines = WrappedRows::new(&self.text, width, 0)
            .skip(top)
            .take(usize::from(height))
            .map(|row| &self.text[row])
            .collect();
        (lines, top, total)
    }

    fn scroll(&mut self, delta: i16, page: bool) {
        let (width, height) = self.size.get();
        let (top, total) = self.window(width, height);
        let step = usize::from(delta.unsigned_abs()).saturating_mul(if page {
            usize::from(height.max(1))
        } else {
            1
        });
        let target = if delta < 0 {
            top.saturating_sub(step)
        } else {
            top.saturating_add(step)
        }
        .min(total.saturating_sub(usize::from(height.max(1))));
        self.byte_offset = WrappedRows::new(&self.text, width, 0)
            .nth(target)
            .map_or(0, |row| row.start);
        self.at_end = false;
    }
}

impl AppState {
    fn request_details_available(&self) -> bool {
        matches!(self.view, View::Thread(_))
            && self.input_mode == InputMode::Normal
            && !self.goal_actions_open
            && !self.thread_queue_open
            && !self.command_palette_open
            && !self.context_open
            && !self.launch_menu_open
            && !self.transcript_search_open
            && !self.terminal_focused
            && !self.show_help
            && self.saved_view_editor.is_none()
            && self.pending_operation.is_none()
            && self.pending_forge_operation.is_none()
            && self.pending_local_batch.is_none()
            && self.pending_launch_plan.is_none()
            && self.pending_queue_confirmation.is_none()
            && self.hot_slot_bind_pending.is_none()
    }

    pub(crate) fn active_request_details(&self) -> Option<&RequestDetails> {
        let details = self.request_details.as_ref()?;
        (self.request_details_available()
            && self.current_pending_request() == Some(&details.request))
        .then_some(details)
    }

    pub fn request_details_open(&self) -> bool {
        self.active_request_details().is_some()
    }

    pub(super) fn open_request_details(&mut self) {
        if !self.request_details_available() {
            return;
        }
        let Some(request) = self.current_pending_request().cloned() else {
            return;
        };
        self.request_details = Some(RequestDetails {
            text: request_lines(&request, self.language, true).join("\n"),
            request,
            byte_offset: 0,
            at_end: false,
            size: Size::new(self.thread_viewport_size.get()),
        });
    }

    pub(super) fn close_request_details(&mut self) {
        self.request_details = None;
    }

    pub(super) fn invalidate_request_details(&mut self) {
        if self.active_request_details().is_none() {
            self.request_details = None;
        }
    }

    pub(super) fn scroll_request_details(&mut self, delta: i16) {
        self.invalidate_request_details();
        if let Some(details) = &mut self.request_details {
            details.scroll(delta, false);
        }
    }

    pub(super) fn page_request_details(&mut self, delta: i16) {
        self.invalidate_request_details();
        if let Some(details) = &mut self.request_details {
            details.scroll(delta, true);
        }
    }

    pub(super) fn request_details_boundary(&mut self, at_end: bool) {
        self.invalidate_request_details();
        if let Some(details) = &mut self.request_details {
            details.byte_offset = 0;
            details.at_end = at_end;
        }
    }
}

#[cfg(test)]
mod tests;
