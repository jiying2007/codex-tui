use super::*;

#[test]
fn terminal_text_fields_escape_spoofing_and_preserve_column_tabs_and_json() {
    let original = "Title\tpart\n\u{202e}\u{001b}[2J";
    let displayed = headless_field(original);
    assert_eq!(displayed, "Title part \u{fffd}\u{fffd}[2J");
    assert_eq!(
        format!("{}\t{}", headless_field(original), headless_field("中文")),
        format!("{displayed}\t中文")
    );
    assert_eq!(original, "Title\tpart\n\u{202e}\u{001b}[2J");
    let json = serde_json::to_string(original).expect("serialize original");
    assert!(json.contains(r"\t"));
    assert!(json.contains(r"\n"));
    assert!(!json.contains("\u{fffd}"));
}

#[test]
fn terminal_state_top_level_aliases_are_read_only_headless_routes() {
    let cases = [
        (vec!["status"], vec!["status"]),
        (vec!["thread", "list", "--json"], vec!["threads", "--json"]),
        (
            vec!["attention", "list", "--json"],
            vec!["attention", "--json"],
        ),
        (vec!["board", "list"], vec!["board"]),
        (vec!["forge", "status"], vec!["forge"]),
        (vec!["worktree", "list"], vec!["worktrees"]),
    ];
    for (input, expected) in cases {
        let input = input.into_iter().map(str::to_string).collect::<Vec<_>>();
        let expected = expected.into_iter().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(top_level_alias(&input), Some(expected));
    }
    assert_eq!(top_level_alias(&["doctor".into(), "store".into()]), None);
}

#[tokio::test]
async fn fixture_10k_thread_snapshot_is_stable_and_not_degraded() {
    let snapshot = build_thread_snapshot(&HeadlessOptions::fixture(true)).await;
    assert_eq!(snapshot.schema, "codex-tui/headless-threads/v1");
    assert!(!snapshot.degraded);
    assert_eq!(snapshot.threads.len(), 10_000);
    assert_eq!(snapshot.source.scope, "fixture");
    assert!(snapshot.source.target_id.is_none());
    assert_eq!(
        snapshot.threads.first().expect("first").id,
        "thread-scale-00000"
    );
    assert_eq!(
        snapshot.threads.last().expect("last").id,
        "thread-scale-09999"
    );
}

#[tokio::test]
async fn fixture_work_snapshot_does_not_probe_or_load_local_state() {
    let snapshot = build_work_snapshot(&HeadlessOptions::fixture(false)).await;
    assert_eq!(snapshot.schema, "codex-tui/headless-work/v1");
    assert!(!snapshot.degraded);
    assert_eq!(snapshot.work.len(), 4);
    assert!(snapshot.local_store_error.is_none());
    assert_eq!(snapshot.local_store_scope, "fixture");
}

#[tokio::test]
async fn direct_library_calls_reject_invalid_options_before_any_source_work() {
    for args in [
        vec!["threads", "--target"],
        vec!["work", "--target", "a", "--target", "b"],
        vec!["status", "--fake", "--target", "a"],
        vec!["board", "--fixture-10k", "--target", "a"],
        vec!["forge", "--target", "a"],
        vec!["worktrees", "--target", "a"],
        vec!["threads", "--fake", "--fixture-10k"],
        vec!["threads", "--json", "--json"],
    ] {
        let args = args.into_iter().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(run(&args).await.expect("usage result"), EXIT_USAGE);
    }
}

#[test]
fn indexed_work_projection_preserves_missing_duplicate_and_scratch_overlays() {
    use crate::{
        backend::{CodexBackend, FakeBackend},
        planning::{
            ReconcileInput, ScratchState, ScratchWork, SourceRef, WorkCardOverlay, WorkCardRecord,
            reconcile_scratch_card_with_local, reconcile_thread_card,
        },
    };
    let threads = FakeBackend::scaled(40).snapshot().threads;
    let mut planning = PlanningSnapshot::default();
    for (index, thread) in threads.iter().enumerate().step_by(2) {
        let mut record = WorkCardRecord::implicit_thread(&thread.id);
        record.overlay.title_override = Some(format!("local title {index}"));
        record.overlay.priority = Some(index as i32);
        record.overlay.snooze_until_unix_ms = Some(200);
        planning.cards.push(record);
    }
    planning.cards.reverse();
    let mut duplicate = planning.cards[0].clone();
    duplicate.overlay.title_override = Some("must not replace first match".into());
    planning.cards.push(duplicate);
    planning.scratch.push(ScratchWork {
        id: "scratch-local".into(),
        title: "scratch".into(),
        note: None,
        workspace: Some("repo".into()),
        priority: None,
        state: ScratchState::Inbox,
        created_at_unix_ms: 1,
        updated_at_unix_ms: 1,
    });
    let scratch_anchor = SourceRef {
        kind: SourceKind::ScratchWork,
        value: "scratch-local".into(),
    };
    let scratch_record = WorkCardRecord {
        local_id: "local-scratch".into(),
        anchor: scratch_anchor.clone(),
        links: vec![],
        overlay: WorkCardOverlay {
            title_override: Some("local scratch title".into()),
            priority: Some(99),
            ..WorkCardOverlay::default()
        },
    };
    planning.cards.push(scratch_record.clone());
    let mut expected = threads
        .iter()
        .map(|thread| {
            let anchor = SourceRef::codex_thread(&thread.id);
            reconcile_thread_card(ReconcileInput {
                thread,
                git: None,
                local: planning.cards.iter().find(|card| card.anchor == anchor),
                collision_count: 0,
                backend_observed_at_unix_ms: Some(10),
                backend_error: Some("fixture source error"),
                now_unix_ms: 100,
            })
        })
        .collect::<Vec<_>>();
    expected.push(reconcile_scratch_card_with_local(
        &planning.scratch[0],
        Some(&scratch_record),
        100,
    ));
    assert_eq!(
        projection::reconcile_work(
            &threads,
            &planning,
            Some(10),
            Some("fixture source error"),
            100
        ),
        expected
    );
}
