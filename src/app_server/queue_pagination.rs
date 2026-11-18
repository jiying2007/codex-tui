//! Bounded queue pagination. Actor advances one page at a time and yields to controls.
use super::*;
use crate::thread_queue::QueuedSubmission;

pub(super) struct QueueLoadState {
    pub(super) thread_id: ThreadId,
    pub(super) mutation_readback: bool,
    cursor: Option<String>,
    submissions: Vec<QueuedSubmission>,
    cursors: BTreeSet<String>,
    pages: usize,
}

impl QueueLoadState {
    pub(super) fn new(thread_id: ThreadId, mutation_readback: bool) -> Self {
        Self {
            thread_id, mutation_readback, cursor: None,
            submissions: Vec::new(), cursors: BTreeSet::new(), pages: 0,
        }
    }

    pub(super) async fn next_page(&mut self, rpc: &mut RpcSession) -> Result<Option<ThreadQueueSnapshot>> {
        anyhow::ensure!(self.pages < THREAD_QUEUE_MAX_PAGES, "thread queue page budget exhausted");
        let result = rpc.request("thread/queue/list", json!({
            "threadId": self.thread_id.0,
            "cursor": self.cursor,
            "limit": THREAD_QUEUE_PAGE_LIMIT
        })).await.context("list thread queue")?;
        let page = parse_queue_list(self.thread_id.clone(), result)?;
        self.pages += 1;
        self.submissions.extend(page.submissions);
        if let Some(cursor) = page.next_cursor {
            anyhow::ensure!(self.cursors.insert(cursor.clone()), "thread queue pagination cursor repeated");
            anyhow::ensure!(self.pages < THREAD_QUEUE_MAX_PAGES,
                "thread queue exceeds bounded pagination limit of {} entries",
                THREAD_QUEUE_MAX_PAGES * THREAD_QUEUE_PAGE_LIMIT as usize);
            self.cursor = Some(cursor);
            return Ok(None);
        }
        let snapshot = ThreadQueueSnapshot {
            thread_id: self.thread_id.clone(),
            submissions: std::mem::take(&mut self.submissions),
            next_cursor: None,
        };
        snapshot.validate_complete()?;
        Ok(Some(snapshot))
    }
}

// Single-call helper for isolated wire tests and legacy support paths.
// The live registry actor uses QueueLoadState page-by-page instead.
pub(super) async fn load_thread_queue(rpc: &mut RpcSession, id: ThreadId) -> Result<ThreadQueueSnapshot> {
    let mut state = QueueLoadState::new(id, false);
    loop { if let Some(snapshot) = state.next_page(rpc).await? { return Ok(snapshot); } }
}
