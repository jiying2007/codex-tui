use codex_tui::{
    backend::CodexBackend,
    codex_protocol::{normalize_thread, parse_thread_list},
    conversation::{ConversationItemKind, parse_items_page, parse_legacy_thread_read},
    domain::{AttentionReason, RuntimeStatus, ThreadId},
    replay_backend::ReplayBackend,
};

#[test]
fn current_registry_fixture_replays_through_production_normalization() {
    let mut replay =
        ReplayBackend::from_jsonl(include_str!("fixtures/protocol/current/registry.jsonl"))
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
fn stable_0_164_0_read_state_events_do_not_change_local_attention_or_drop_updates() {
    let mut replay = ReplayBackend::from_jsonl(include_str!(
        "fixtures/protocol/stable-0.164.0/registry.jsonl"
    ))
    .expect("reviewed upstream notification shapes");
    assert_eq!(
        replay.frame_count(),
        4,
        "read-state events stay unsupported"
    );
    let started = replay.tick();
    assert!(started.threads[0].attention.is_empty());
    let waiting = replay.tick();
    assert_eq!(waiting.threads[0].runtime, RuntimeStatus::WaitingHuman);
    assert_eq!(
        waiting.threads[0].attention,
        [AttentionReason::ApprovalRequired]
    );
    let renamed = replay.tick();
    assert_eq!(renamed.threads[0].title, "Still actionable");
    assert_eq!(renamed.threads[0].attention, waiting.threads[0].attention);
}

#[test]
fn stable_0_164_0_thread_list_accepts_additive_read_state_without_owning_it() {
    let value = serde_json::from_str(include_str!(
        "fixtures/protocol/stable-0.164.0/thread-list.json"
    ))
    .unwrap();
    let page = parse_thread_list(value).expect("additive list response");
    assert_eq!(page.next_cursor.as_deref(), Some("next-page"));
    let thread = normalize_thread(page.data.into_iter().next().unwrap(), None);
    assert_eq!(thread.id.0, "stable-0164-thread");
    assert_eq!(thread.title, "Protocol fixture");
    assert_eq!(thread.runtime, RuntimeStatus::Ready);
    assert!(
        thread.attention.is_empty(),
        "no implicit upstream unread sync"
    );
}

#[test]
fn stable_0_164_0_read_state_does_not_break_existing_history_parser() {
    let value = serde_json::from_str(include_str!(
        "fixtures/protocol/stable-0.164.0/thread-read.json"
    ))
    .unwrap();
    let page = parse_legacy_thread_read(value, ThreadId::new("stable-0164-thread"))
        .expect("additive read response");
    assert_eq!(page.title.as_deref(), Some("Protocol fixture"));
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].kind, ConversationItemKind::User);
    assert_eq!(page.items[1].text, "Fixture response");
}

#[test]
fn stable_0_164_0_image_result_variants_keep_neighboring_history_and_pagination() {
    let value =
        serde_json::from_str(include_str!("fixtures/protocol/stable-0.164.0/items.json")).unwrap();
    let (items, cursor) = parse_items_page(value).expect("image result variants");
    assert_eq!(cursor.as_deref(), Some("older-items"));
    assert_eq!(items.len(), 3);
    for (item, id) in items[..2].iter().zip(["image-inline", "image-reference"]) {
        assert_eq!(item.item_id, id);
        assert_eq!(item.kind, ConversationItemKind::Other);
        assert_eq!(item.text, "imageGeneration");
        assert_eq!(item.status.as_deref(), Some("completed"));
    }
    assert_eq!(items[2].kind, ConversationItemKind::Assistant);
    assert_eq!(items[2].text, "Text after images");
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
    let error =
        ReplayBackend::from_jsonl(include_str!("fixtures/protocol/malformed/registry.jsonl"))
            .expect_err("malformed fixture must fail");

    let message = format!("{error:#}");
    assert!(message.contains("decode app-server JSON line"));
    assert!(message.contains("bytes"));
    assert!(!message.contains("THIS_IS_NOT_VALID_JSON_PAYLOAD"));
}

#[test]
fn stable_0_162_1_project_unassignment_replays_without_stale_authority() {
    let mut replay = ReplayBackend::from_jsonl(include_str!(
        "fixtures/protocol/stable-0.162.1/project-unassignment.jsonl"
    ))
    .expect("pinned upstream null-project fixture");
    assert_eq!(replay.frame_count(), 5);
    let start = replay.tick();
    assert_eq!(start.threads[0].workspace, "project:project-A");
    let unassigned = replay.tick();
    assert_eq!(unassigned.threads[0].workspace, "repo");
    assert_eq!(unassigned.threads[0].metadata.project_id, None);
    assert_eq!(
        unassigned.threads[0].metadata.workspace_key,
        "cwd:/srv/team/repo"
    );
    let assigned = replay.tick();
    assert_eq!(assigned.threads[0].workspace, "project:project-B");
    let named = replay.tick();
    assert_eq!(named.threads[0].title, "New title");
    assert!(!format!("{named:?}").contains("do-not-retain"));
}
