//! Guard the UI fairness boundary: Registry snapshots are latest-value state,
//! while conversation/approval events remain an ordered bounded FIFO.

#[test]
fn full_registry_watch_is_polled_once_per_ui_turn() {
    let source = include_str!("../src/main.rs");
    let drain = source
        .split_once("fn drain_registry(")
        .expect("production Registry drain must exist")
        .1
        .split_once("\n#[cfg(test)]")
        .expect("Registry drain must remain before tests")
        .0;
    let (snapshots, events) = drain
        .split_once("    for _ in 0..32 {\n        let Some(event) = registry.try_recv_conversation()")
        .expect("ordered conversation events retain their bounded drain");
    assert!(snapshots.contains("if let Some(snapshot) = registry.try_recv() {"));
    assert_eq!(snapshots.matches("registry.try_recv()").count(), 1);
    assert!(!snapshots.contains("for _ in 0..32"));
    assert!(events.contains("changes.any |= app.settle_stopped_actor(effects)"));
}

#[test]
fn watch_updates_coalesce_without_losing_the_latest_generation() {
    let (tx, mut rx) = tokio::sync::watch::channel(0_u64);
    for generation in 1..=1_000 {
        tx.send(generation).expect("live receiver");
    }
    assert_eq!(*rx.borrow_and_update(), 1_000);
    assert!(!rx.has_changed().expect("connected"));
    tx.send(1_001).expect("another generation");
    assert_eq!(*rx.borrow_and_update(), 1_001);
    assert!(!rx.has_changed().expect("connected"));
}
