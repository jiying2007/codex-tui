use codex_tui::{
    backend::{CodexBackend, FakeBackend},
    planning::{
        ReconcileInput, SavedView, SavedViewLayout, WorkCardProjection, apply_saved_view,
        reconcile_thread_card,
    },
};
use std::sync::OnceLock;

fn main() {
    divan::main();
}

fn cards_10k() -> &'static Vec<WorkCardProjection> {
    static CARDS: OnceLock<Vec<WorkCardProjection>> = OnceLock::new();
    CARDS.get_or_init(|| {
        let snapshot = FakeBackend::scaled(10_000).snapshot();
        snapshot
            .threads
            .iter()
            .map(|thread| {
                reconcile_thread_card(ReconcileInput {
                    thread,
                    git: None,
                    local: None,
                    collision_count: 0,
                    backend_observed_at_unix_ms: Some(1),
                    backend_error: None,
                    now_unix_ms: 1,
                })
            })
            .collect()
    })
}

#[divan::bench]
fn planning_attention_filter_10k() {
    let view = SavedView {
        id: "bench:attention".into(),
        name: "Attention".into(),
        source_scope: "all".into(),
        filter: "status:needs-you".into(),
        group_by: Some("workspace".into()),
        order_by: Some("priority".into()),
        layout: SavedViewLayout::List,
        visible_fields: vec![],
    };
    divan::black_box(apply_saved_view(cards_10k(), &view));
}

#[divan::bench]
fn planning_workspace_filter_10k() {
    let view = SavedView {
        id: "bench:workspace".into(),
        name: "Workspace".into(),
        source_scope: "all".into(),
        filter: "workspace:repo-031".into(),
        group_by: Some("workspace".into()),
        order_by: Some("title".into()),
        layout: SavedViewLayout::List,
        visible_fields: vec![],
    };
    divan::black_box(apply_saved_view(cards_10k(), &view));
}

#[divan::bench]
fn planning_rich_query_10k() {
    let view = SavedView {
        id: "bench:rich-query".into(),
        name: "Rich Query".into(),
        source_scope: "all".into(),
        filter: "status:needs-you -stage:done project:repo".into(),
        group_by: Some("workspace".into()),
        order_by: Some("priority".into()),
        layout: SavedViewLayout::List,
        visible_fields: vec![],
    };
    divan::black_box(apply_saved_view(cards_10k(), &view));
}
