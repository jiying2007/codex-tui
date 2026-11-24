//! Single ordered SQLite worker. The UI only uses bounded, non-blocking queues.
//! Local mutations report success after commit; operator snapshots coalesce on
//! queue pressure. Shutdown is a barrier and runs after terminal restoration.
use crate::runtime_store::{RuntimeStore as StoreBackend, StoreBootstrap};
use anyhow::{Context, Result};
use codex_tui::{
    planning::PlanningSnapshot, store::LocalStateV1,
    transcript_search::TranscriptSearchResults, workref::WorkRefLedger,
};
use std::{
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    time::{Duration, Instant},
};

const CAPACITY: usize = 32;
const WRITE_BEHIND: Duration = Duration::from_millis(250);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
type Job = Box<dyn FnOnce(&mut StoreBackend) -> StoreEvent + Send>;

type PlanningResult = std::result::Result<PlanningSnapshot, String>;
pub(crate) enum StoreEvent {
    Operator(Option<String>),
    Planning(PlanningResult, Option<String>, Option<u64>),
    Stopped,
    Search(std::result::Result<TranscriptSearchResults, String>),
    Notice(Option<String>),
    WorkrefLinked {
        key: String,
        result: std::result::Result<(WorkRefLedger, String), String>,
    },
}
enum Request {
    Job(Job),
    Operator(Box<LocalStateV1>),
    Flush(
        Box<LocalStateV1>,
        SyncSender<std::result::Result<(), String>>,
    ),
}

pub(crate) struct StoreWorker {
    requests: SyncSender<Request>,
    events: Receiver<StoreEvent>,
    pending_operator: Option<Box<LocalStateV1>>,
    dirty_since: Option<Instant>,
    error: Option<String>,
    stopped: bool,
}

impl StoreWorker {
    pub(crate) fn discover() -> Result<(Self, StoreBootstrap)> {
        let (backend, bootstrap) = StoreBackend::discover()?;
        Ok((Self::start(backend)?, bootstrap))
    }
    fn start(mut backend: StoreBackend) -> Result<Self> {
        let error = backend.error();
        let (requests, incoming) = mpsc::sync_channel::<Request>(CAPACITY);
        let (outgoing, events) = mpsc::sync_channel(CAPACITY);
        std::thread::Builder::new()
            .name("codex-tui-store".into())
            .spawn(move || {
                while let Ok(request) = incoming.recv() {
                    let event = match request {
                        Request::Job(job) => job(&mut backend),
                        Request::Operator(state) => {
                            StoreEvent::Operator(backend.persist_operator_state(&state))
                        }
                        Request::Flush(state, reply) => {
                            let result = backend
                                .flush_operator_state_on_exit(&state)
                                .map_err(|error| format!("{error:#}"));
                            let _ = reply.send(result);
                            break;
                        }
                    };
                    if outgoing.send(event).is_err() {
                        break;
                    }
                }
            })
            .context("start local state worker")?;
        Ok(Self {
            requests,
            events,
            pending_operator: None,
            dirty_since: None,
            error,
            stopped: false,
        })
    }
    pub(crate) fn error(&self) -> Option<String> {
        self.error.clone()
    }
    pub(crate) fn defer_operator_state(&mut self) {
        self.dirty_since.get_or_insert_with(Instant::now);
    }
    pub(crate) fn operator_state_flush_due(&self) -> bool {
        self.dirty_since
            .is_some_and(|since| since.elapsed() >= WRITE_BEHIND)
    }
    pub(crate) fn persist_operator_state(&mut self, state: &LocalStateV1) -> Option<String> {
        self.dirty_since = None;
        self.pending_operator = Some(Box::new(state.clone()));
        self.flush_pending()
    }
    fn flush_pending(&mut self) -> Option<String> {
        let state = self.pending_operator.take()?;
        match self.requests.try_send(Request::Operator(state)) {
            Ok(()) => None,
            Err(TrySendError::Full(Request::Operator(state))) => {
                self.pending_operator = Some(state);
                None
            }
            Err(_) => Some("local state worker disconnected; operator state is not durable".into()),
        }
    }
    pub(crate) fn submit<F>(&mut self, job: F) -> std::result::Result<(), String>
    where
        F: FnOnce(&mut StoreBackend) -> StoreEvent + Send + 'static,
    {
        self.requests
            .try_send(Request::Job(Box::new(job)))
            .map_err(|error| match error {
                TrySendError::Full(_) => {
                    "local state queue is full; operation was not accepted".into()
                }
                TrySendError::Disconnected(_) => {
                    "local state worker disconnected; operation was not accepted".into()
                }
            })
    }
    pub(crate) fn planning<F>(
        &mut self,
        job: F,
        notice: Option<String>,
    ) -> std::result::Result<(), String>
    where
        F: FnOnce(&mut StoreBackend) -> PlanningResult + Send + 'static,
    {
        self.planning_with_ticket(job, notice, None)
    }
    pub(crate) fn planning_with_ticket<F>(
        &mut self,
        job: F,
        notice: Option<String>,
        ticket: Option<u64>,
    ) -> std::result::Result<(), String>
    where
        F: FnOnce(&mut StoreBackend) -> PlanningResult + Send + 'static,
    {
        self.submit(move |store| StoreEvent::Planning(job(store), notice, ticket))
    }
    pub(crate) fn index_conversation_page(
        &mut self,
        page: &codex_tui::conversation::ConversationPage,
    ) -> Option<String> {
        let page = page.clone();
        self.submit(move |store| StoreEvent::Notice(store.index_conversation_page(&page)))
            .err()
    }
    pub(crate) fn try_event(&mut self) -> Option<StoreEvent> {
        if let Some(error) = self.flush_pending() {
            return Some(StoreEvent::Operator(Some(error)));
        }
        match self.events.try_recv() {
            Ok(event) => {
                match &event {
                    StoreEvent::Operator(Some(error)) | StoreEvent::Planning(Err(error), _, _) => {
                        self.error = Some(error.clone());
                    }
                    StoreEvent::WorkrefLinked {
                        result: Err(error), ..
                    } if error.starts_with("SQLite workref") => {
                        self.error = Some(error.clone());
                    }
                    _ => {}
                }
                Some(event)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) if !self.stopped => {
                self.stopped = true;
                Some(StoreEvent::Stopped)
            }
            Err(TryRecvError::Disconnected) => None,
        }
    }
    fn drain_exit_events(&self, failure: &mut Option<String>) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                StoreEvent::Operator(Some(error)) | StoreEvent::Planning(Err(error), _, _) => {
                    failure.get_or_insert(error);
                }
                StoreEvent::WorkrefLinked {
                    result: Err(error), ..
                } if error.starts_with("SQLite workref") => {
                    failure.get_or_insert(error);
                }
                _ => {}
            }
        }
    }
    pub(crate) fn flush_operator_state_on_exit(&mut self, state: &LocalStateV1) -> Result<()> {
        // Supersede the unsent operator snapshot. Already accepted jobs precede this barrier.
        self.pending_operator = None;
        let (reply, receipt) = mpsc::sync_channel(1);
        let mut request = Request::Flush(Box::new(state.clone()), reply);
        let started = Instant::now();
        let mut failure = None;
        loop {
            self.drain_exit_events(&mut failure);
            match self.requests.try_send(request) {
                Ok(()) => break,
                Err(TrySendError::Full(returned)) => request = returned,
                Err(TrySendError::Disconnected(_)) => {
                    anyhow::bail!("final state flush: worker disconnected")
                }
            }
            anyhow::ensure!(
                started.elapsed() < SHUTDOWN_TIMEOUT,
                "final state flush queue deadline exceeded"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        loop {
            self.drain_exit_events(&mut failure);
            match receipt.try_recv() {
                Ok(result) => {
                    self.stopped = true;
                    self.drain_exit_events(&mut failure);
                    return crate::runtime_shutdown::combine(
                        failure.map_or(Ok(()), |error| Err(anyhow::Error::msg(error))),
                        result.map_err(anyhow::Error::msg),
                    );
                }
                Err(TryRecvError::Disconnected) => {
                    anyhow::bail!("final state flush: receipt unavailable")
                }
                Err(TryRecvError::Empty) => {}
            }
            anyhow::ensure!(
                started.elapsed() < SHUTDOWN_TIMEOUT,
                "final state flush deadline exceeded"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(test)]
impl StoreWorker {
    pub(crate) fn at_for_test(root: &std::path::Path) -> Self {
        Self::start(StoreBackend::at_for_test(root)).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn busy_worker_does_not_block_submission_and_preserves_commit_order() {
        let root = tempfile::tempdir().unwrap();
        let backend = StoreBackend::at_for_test(root.path());
        let mut worker = StoreWorker::start(backend).unwrap();
        let (release, wait) = mpsc::sync_channel(0);
        worker
            .submit(move |_| {
                wait.recv().unwrap();
                StoreEvent::Notice(None)
            })
            .unwrap();
        let started = Instant::now();
        let mut first = LocalStateV1::default();
        first.pins.insert("old".into());
        assert!(worker.persist_operator_state(&first).is_none());
        assert!(started.elapsed() < Duration::from_millis(100));
        release.send(()).unwrap();
        let mut last = LocalStateV1::default();
        last.pins.insert("latest".into());
        worker.flush_operator_state_on_exit(&last).unwrap();
        use codex_tui::store::LocalStore;
        assert_eq!(
            codex_tui::sqlite_store::SqliteStore::at(root.path())
                .load_state()
                .unwrap(),
            last
        );
    }
    #[test]
    fn saturation_rejects_explicit_work_but_coalesces_operator_state() {
        let root = tempfile::tempdir().unwrap();
        let mut worker = StoreWorker::start(StoreBackend::at_for_test(root.path())).unwrap();
        let (started, ready) = mpsc::sync_channel(0);
        let (release, wait) = mpsc::sync_channel(0);
        worker
            .submit(move |_| {
                started.send(()).unwrap();
                wait.recv().unwrap();
                StoreEvent::Notice(None)
            })
            .unwrap();
        ready.recv().unwrap();
        for _ in 0..CAPACITY {
            worker.submit(|_| StoreEvent::Notice(None)).unwrap();
        }
        assert!(
            worker
                .submit(|_| StoreEvent::Notice(None))
                .unwrap_err()
                .contains("not accepted")
        );
        let last = LocalStateV1::default();
        assert!(worker.persist_operator_state(&last).is_none());
        assert!(worker.pending_operator.is_some());
        release.send(()).unwrap();
        worker.flush_operator_state_on_exit(&last).unwrap();
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;
    #[test]
    fn final_flush_tracks_only_workref_storage_failures_not_rejected_keys() {
        let root = tempfile::tempdir().unwrap();
        let mut rejected = StoreWorker::start(StoreBackend::at_for_test(root.path())).unwrap();
        rejected
            .submit(|_| StoreEvent::WorkrefLinked {
                key: "MISSING".into(),
                result: Err("unknown work-item key".into()),
            })
            .unwrap();
        rejected
            .flush_operator_state_on_exit(&LocalStateV1::default())
            .expect("a rejected key is not a failed SQLite commit");

        let root = tempfile::tempdir().unwrap();
        let mut failed = StoreWorker::start(StoreBackend::at_for_test(root.path())).unwrap();
        failed
            .submit(|_| StoreEvent::WorkrefLinked {
                key: "BUG17".into(),
                result: Err("SQLite workref write failed: injected".into()),
            })
            .unwrap();
        let error = failed
            .flush_operator_state_on_exit(&LocalStateV1::default())
            .unwrap_err();
        assert!(error.to_string().contains("SQLite workref write failed"));
    }

    #[test]
    fn final_flush_does_not_hide_an_unobserved_accepted_mutation_failure() {
        let root = tempfile::tempdir().unwrap();
        let mut worker = StoreWorker::start(StoreBackend::at_for_test(root.path())).unwrap();
        worker
            .submit(|_| StoreEvent::Planning(Err("accepted write failed".into()), None, None))
            .unwrap();
        let error = worker
            .flush_operator_state_on_exit(&LocalStateV1::default())
            .unwrap_err();
        assert!(error.to_string().contains("accepted write failed"));
    }
}

#[cfg(test)]
mod write_behind_tests {
    use super::*;
    #[test]
    fn repeated_edits_do_not_postpone_the_production_flush_deadline() {
        let root = tempfile::tempdir().unwrap();
        let mut worker = StoreWorker::start(StoreBackend::at_for_test(root.path())).unwrap();
        worker.dirty_since = Some(Instant::now() - WRITE_BEHIND);
        let first = worker.dirty_since;
        worker.defer_operator_state();
        assert_eq!(worker.dirty_since, first);
        assert!(worker.operator_state_flush_due());
        worker.persist_operator_state(&LocalStateV1::default());
        assert!(!worker.operator_state_flush_due());
        worker
            .flush_operator_state_on_exit(&LocalStateV1::default())
            .unwrap();
    }
}
