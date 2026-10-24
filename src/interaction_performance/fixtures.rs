//! Metadata-only fixtures exercise populated local overlays and grouped layouts.
use crate::{
    app::AppState,
    backend::{CodexBackend, FakeBackend},
    domain::ThreadSummary,
    planning::{
        PlanningSnapshot, SavedView, SavedViewLayout, ScratchState, ScratchWork, SourceKind,
        SourceRef, WorkCardOverlay, WorkCardRecord,
    },
    render_performance::board_fixture,
};
use std::{hint::black_box, time::Instant};

pub(super) const NAMES: [&str; 11] = [
    "registry-input-and-render-10k",
    "board-navigation-and-render-10k",
    "thread-revision-and-render-10k",
    "planning-ui-snapshot-10k",
    "planning-worker-compute-10k",
    "planning-ui-commit-10k",
    "list-navigation-and-render-10k",
    "review-queue-navigation-and-render-10k",
    "headless-local-cards-10k",
    "planning-ui-rebase-10k",
    "planning-ui-rebase-50k",
];

pub(super) fn list_fixture(layout: SavedViewLayout) -> AppState {
    let mut app = board_fixture();
    app.planning_snapshot.saved_views.push(SavedView {
        id: "diagnostic-list".into(),
        name: "Diagnostic list".into(),
        source_scope: "all".into(),
        filter: String::new(),
        group_by: Some("workspace".into()),
        order_by: Some("priority".into()),
        layout,
        visible_fields: vec!["attention".into(), "workspace".into(), "branch".into()],
    });
    app.planning_view_index = 4;
    app.board_selected = app.work_cards.len() / 2;
    app
}

pub(super) struct HeadlessFixture {
    threads: Vec<ThreadSummary>,
    local: PlanningSnapshot,
}

impl HeadlessFixture {
    pub(super) fn new(rows: usize) -> Self {
        let threads = FakeBackend::scaled(rows / 2).snapshot().threads;
        let mut local = PlanningSnapshot {
            cards: threads
                .iter()
                .enumerate()
                .map(|(index, thread)| {
                    let mut record = WorkCardRecord::implicit_thread(&thread.id);
                    record.overlay.priority = Some(index as i32);
                    record.overlay.title_override = Some(format!("Local thread {index}"));
                    record
                })
                .collect(),
            ..PlanningSnapshot::default()
        };
        for index in threads.len()..rows {
            let id = format!("local-scratch-{index}");
            local.scratch.push(ScratchWork {
                id: id.clone(),
                title: format!("Scratch {index}"),
                note: None,
                workspace: Some(format!("repo-{}", index % 64)),
                priority: None,
                state: ScratchState::Ready,
                created_at_unix_ms: 1,
                updated_at_unix_ms: 1,
            });
            local.cards.push(WorkCardRecord {
                local_id: format!("scratch:{id}"),
                anchor: SourceRef {
                    kind: SourceKind::ScratchWork,
                    value: id,
                },
                links: vec![],
                overlay: WorkCardOverlay {
                    title_override: Some(format!("Local scratch {index}")),
                    priority: Some(index as i32),
                    ..WorkCardOverlay::default()
                },
            });
        }
        local.cards.reverse();
        Self { threads, local }
    }

    pub(super) fn measure(&self) -> f64 {
        let started = Instant::now();
        let cards = crate::headless::projection::reconcile_work(
            &self.threads,
            &self.local,
            Some(1),
            None,
            10,
        );
        black_box(&cards);
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(cards.len(), self.local.cards.len());
        elapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headless_fixture_covers_both_populated_local_anchor_kinds() {
        let fixture = HeadlessFixture::new(10_000);
        assert_eq!(fixture.threads.len(), 5_000);
        assert_eq!(fixture.local.scratch.len(), 5_000);
        assert_eq!(fixture.local.cards.len(), 10_000);
        let cards = crate::headless::projection::reconcile_work(
            &fixture.threads,
            &fixture.local,
            Some(1),
            None,
            10,
        );
        for (index, card) in cards.iter().enumerate() {
            assert_eq!(card.overlay.priority, Some(index as i32));
            assert!(card.title.starts_with("Local "));
        }
    }
}
