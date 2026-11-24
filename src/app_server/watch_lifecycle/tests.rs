use super::*;
use serde_json::json;

fn owners(ids: &[&str]) -> WatchSet {
    let watches = WatchSet::default();
    for id in ids {
        watches.admit(id, || Ok(())).unwrap();
    }
    watches
}

#[tokio::test]
async fn archive_and_delete_retire_history_without_a_registry_row() {
    for method in ["thread/archived", "thread/deleted"] {
        let history = owners(&["provisional", "other"]);
        let (tx, mut rx) = mpsc::channel(2);
        retire_removed_watches(
            method,
            &json!({"threadId":"provisional"}),
            &history,
            &tx,
        )
        .await;
        assert!(!history.contains("provisional"));
        assert!(history.contains("other"));
        let ConversationEvent::Failed { thread_id, .. } = rx.try_recv().unwrap() else {
            panic!("history retirement must invalidate provisional UI ownership");
        };
        assert_eq!(thread_id.0, "provisional");
        assert!(rx.try_recv().is_err());
    }
}

#[tokio::test]
async fn malformed_unrelated_and_unwatched_removals_preserve_other_owners() {
    for (method, params) in [
        ("thread/deleted", json!({})),
        ("thread/deleted", json!({"threadId":null})),
        ("thread/deleted", json!({"threadId":7})),
        ("thread/archived", json!({"threadId":"unknown"})),
        ("thread/unarchived", json!({"threadId":"provisional"})),
        ("thread/name/updated", json!({"threadId":"provisional"})),
    ] {
        let history = owners(&["provisional", "other"]);
        let (tx, mut rx) = mpsc::channel(2);
        retire_removed_watches(method, &params, &history, &tx).await;
        assert!(history.contains("provisional"));
        assert!(history.contains("other"));
        assert!(rx.try_recv().is_err());
    }
}

#[tokio::test]
async fn duplicate_removal_is_idempotent_without_persistent_tombstones() {
    let history = owners(&["provisional", "other"]);
    let (tx, mut rx) = mpsc::channel(2);
    for _ in 0..3 {
        retire_removed_watches(
            "thread/deleted",
            &json!({"threadId":"provisional"}),
            &history,
            &tx,
        )
        .await;
    }
    assert!(matches!(
        rx.try_recv().unwrap(),
        ConversationEvent::Failed { .. }
    ));
    assert!(rx.try_recv().is_err());
    assert!(!history.contains("provisional"));
    assert!(history.contains("other"));
    // A later explicit admission is not suppressed by an unbounded tombstone.
    history.admit("provisional", || Ok(())).unwrap();
    assert!(history.contains("provisional"));
}

#[tokio::test]
async fn event_backpressure_does_not_delay_shared_ownership_removal() {
    use futures_util::FutureExt;
    let history = owners(&["provisional", "other"]);
    let shared_history = history.clone();
    let (tx, _rx) = mpsc::channel(1);
    tx.try_send(ConversationEvent::Failed {
        thread_id: ThreadId::new("prior"),
        error: "fill bounded event channel".into(),
    })
    .unwrap();
    assert!(
        retire_removed_watches(
            "thread/deleted",
            &json!({"threadId":"provisional"}),
            &history,
            &tx,
        )
        .now_or_never()
        .is_none()
    );
    // Cancellation during event delivery cannot undo already-published removal.
    assert!(!shared_history.contains("provisional"));
    assert!(shared_history.contains("other"));
}

#[tokio::test]
async fn closed_event_receiver_does_not_retain_or_recreate_owners() {
    let history = owners(&["provisional"]);
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    retire_removed_watches(
        "thread/deleted",
        &json!({"threadId":"provisional"}),
        &history,
        &tx,
    )
    .await;
    assert!(!history.contains("provisional"));
}

#[tokio::test]
async fn removal_does_not_match_a_different_history_id() {
    let history = owners(&["provisional "]);
    let (tx, mut rx) = mpsc::channel(2);
    retire_removed_watches(
        "thread/archived",
        &json!({"threadId":"provisional"}),
        &history,
        &tx,
    )
    .await;
    assert!(history.contains("provisional "));
    assert!(rx.try_recv().is_err());
}
