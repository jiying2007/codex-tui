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
