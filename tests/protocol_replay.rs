use codex_tui::{
    app::{Action, AppState, reduce},
    backend::CodexBackend,
    codex_protocol::{ThreadWire, normalize_thread, parse_thread_list},
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
    let mut app = AppState::new(started.threads);
    reduce(&mut app, Action::MarkUnread);
    let waiting = replay.tick();
    assert_eq!(waiting.threads[0].runtime, RuntimeStatus::WaitingHuman);
    assert_eq!(
        waiting.threads[0].attention,
        [AttentionReason::ApprovalRequired]
    );
    reduce(&mut app, Action::ReplaceThreads(waiting.threads.clone()));
    assert!(
        app.to_local_state()
            .marked_unread
            .contains("stable-0164-thread")
    );
    assert!(
        app.threads[0]
            .attention
            .contains(&AttentionReason::MarkedUnread)
    );
    reduce(&mut app, Action::AcknowledgeAttention);
    let renamed = replay.tick();
    assert_eq!(renamed.threads[0].title, "Still actionable");
    assert_eq!(renamed.threads[0].attention, waiting.threads[0].attention);
    reduce(&mut app, Action::ReplaceThreads(renamed.threads));
    assert!(app.acknowledged_attention.contains("stable-0164-thread"));
    assert!(
        !app.to_local_state()
            .marked_unread
            .contains("stable-0164-thread")
    );
    assert!(
        !app.threads[0]
            .attention
            .contains(&AttentionReason::MarkedUnread)
    );
    assert!(
        app.thread_needs_attention(0),
        "live approval remains actionable"
    );
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
fn stable_0_171_0_registry_replays_parent_and_subagent_lineage() {
    let mut replay = ReplayBackend::from_jsonl(include_str!(
        "fixtures/protocol/stable-0.171.0/registry.jsonl"
    ))
    .expect("pinned 0.171.0 registry fixture");

    let parent = replay.tick();
    assert_eq!(parent.threads.len(), 1);
    assert_eq!(parent.threads[0].id.0, "stable-0171-parent");

    let child = replay.tick();
    assert_eq!(child.threads.len(), 2);
    let child = child
        .threads
        .iter()
        .find(|thread| thread.id.0 == "stable-0171-subagent")
        .expect("spawned subagent");
    assert_eq!(
        child.metadata.lineage.parent_thread_id.as_deref(),
        Some("stable-0171-parent")
    );
    assert_eq!(
        child.metadata.lineage.session_id.as_deref(),
        Some("stable-0171-session")
    );

    let waiting = replay.tick();
    let child = waiting
        .threads
        .iter()
        .find(|thread| thread.id.0 == "stable-0171-subagent")
        .expect("spawned subagent remains present");
    assert_eq!(child.runtime, RuntimeStatus::WaitingHuman);
    assert!(child.attention.contains(&AttentionReason::ApprovalRequired));
}

#[test]
fn stable_0_171_0_thread_list_distinguishes_parent_attachment_and_fork() {
    let value = serde_json::from_str(include_str!(
        "fixtures/protocol/stable-0.171.0/thread-list.json"
    ))
    .expect("pinned 0.171.0 thread/list fixture");
    let page = parse_thread_list(value).expect("additive attached-thread metadata");
    assert_eq!(page.data.len(), 6);

    let threads = page
        .data
        .into_iter()
        .map(|thread| normalize_thread(thread, None))
        .collect::<Vec<_>>();
    assert_eq!(
        threads[1].metadata.lineage.parent_thread_id.as_deref(),
        Some("stable-0171-parent")
    );
    assert_eq!(
        threads[2].metadata.lineage.attached_to_thread_id.as_deref(),
        Some("stable-0171-parent")
    );
    assert_eq!(
        threads[2].metadata.lineage.forked_from_id.as_deref(),
        Some("stable-0171-parent")
    );
    assert_eq!(
        threads[3].metadata.lineage.forked_from_id.as_deref(),
        Some("stable-0171-parent")
    );
    assert_eq!(threads[3].metadata.lineage.attached_to_thread_id, None);

    let app = AppState::new(threads);
    let groups = app.registry_group_projection();
    assert_eq!(
        groups[1].0,
        Some(0),
        "parentThreadId is a direct parent edge"
    );
    assert_eq!(
        groups[2].0,
        Some(0),
        "attachedToThreadId is a distinct authoritative attachment edge"
    );
    assert_eq!(groups[3].0, None, "forkedFromId alone is not a parent edge");
    assert_eq!(groups[4].0, None, "orphan parent must not be invented");
    assert_eq!(groups[5].0, None, "conflicting relation fields fail closed");
    assert_eq!(groups[0].1, 2);
}

#[test]
fn stable_0_171_0_attached_thread_read_and_item_extensions_remain_compatible() {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/protocol/stable-0.171.0/thread-read.json"
    ))
    .expect("pinned 0.171.0 thread/read fixture");
    let wire: ThreadWire =
        serde_json::from_value(value["thread"].clone()).expect("attached-thread metadata decodes");
    let summary = normalize_thread(wire, None);
    assert_eq!(
        summary.metadata.lineage.attached_to_thread_id.as_deref(),
        Some("stable-0171-parent")
    );
    assert_eq!(
        summary.metadata.lineage.forked_from_id.as_deref(),
        Some("stable-0171-parent")
    );
    let page = parse_legacy_thread_read(value, ThreadId::new("stable-0171-attached"))
        .expect("additive attached-thread read response");
    assert_eq!(page.title.as_deref(), Some("Attached session"));
    assert_eq!(page.items.len(), 2);

    let value = serde_json::from_str(include_str!("fixtures/protocol/stable-0.171.0/items.json"))
        .expect("pinned 0.171.0 items fixture");
    let (items, cursor) = parse_items_page(value).expect("additive image-generation metadata");
    assert_eq!(cursor, None);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].kind, ConversationItemKind::Other);
    assert_eq!(items[1].kind, ConversationItemKind::Assistant);
    assert_eq!(items[1].text, "Text remains visible after the image item");
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
