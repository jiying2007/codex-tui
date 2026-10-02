use codex_tui::{
    backend::CodexBackend,
    domain::{AttentionReason, RuntimeStatus},
    replay_backend::ReplayBackend,
};

#[test]
fn current_registry_fixture_replays_through_production_normalization() {
    let mut replay = ReplayBackend::from_jsonl(include_str!(
        "fixtures/protocol/current/registry.jsonl"
    ))
    .expect("current replay");

    assert_eq!(replay.frame_count(), 4);
    assert!(replay.snapshot().threads.is_empty());

    let started = replay.tick();
    let thread = &started.threads[0];
    assert_eq!(thread.id.0, "thread-current");
    assert_eq!(thread.workspace, "project:proj-current");
    assert_eq!(thread.runtime, RuntimeStatus::Ready);

    let waiting = replay.tick();
    let thread = &waiting.threads[0];
    assert_eq!(thread.runtime, RuntimeStatus::WaitingHuman);
    assert_eq!(thread.attention, vec![AttentionReason::ApprovalRequired]);

    let renamed = replay.tick();
    assert_eq!(renamed.threads[0].title, "Current renamed");
}

#[test]
fn previous_stable_fixture_remains_compatible_without_new_optional_fields() {
    let mut replay = ReplayBackend::from_jsonl(include_str!(
        "fixtures/protocol/previous-stable/registry.jsonl"
    ))
    .expect("previous stable replay");

    while replay.snapshot().generation + 1 < replay.frame_count() as u64 {
        replay.tick();
    }
    let snapshot = replay.snapshot();
    assert_eq!(snapshot.threads.len(), 1);
    let thread = &snapshot.threads[0];
    assert_eq!(thread.id.0, "thread-previous");
    assert_eq!(thread.workspace, "legacy-repo");
    assert_eq!(thread.runtime, RuntimeStatus::Ready);
}

#[test]
fn unknown_registry_events_fail_soft_without_creating_fake_frames() {
    let mut replay = ReplayBackend::from_jsonl(include_str!(
        "fixtures/protocol/unknown-events/registry.jsonl"
    ))
    .expect("unknown event replay");

    assert_eq!(replay.frame_count(), 3);
    replay.tick();
    let final_snapshot = replay.tick();
    assert_eq!(final_snapshot.threads[0].runtime, RuntimeStatus::Working);
}

#[test]
fn malformed_wire_fixture_fails_with_bounded_decode_context() {
    let error = ReplayBackend::from_jsonl(include_str!(
        "fixtures/protocol/malformed/registry.jsonl"
    ))
    .expect_err("malformed fixture must fail");

    let message = format!("{error:#}");
    assert!(message.contains("decode app-server JSON line"));
    assert!(message.contains("bytes"));
    assert!(!message.contains("THIS_IS_NOT_VALID_JSON_PAYLOAD"));
}
