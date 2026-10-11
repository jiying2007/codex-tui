use super::*;
use crate::forge::{ForgeIdentity, ForgeProviderKind};
use crate::forge_mutation::{ForgeMutationPlan, ForgeMutationReceipt};
use crate::planning::{LinkRole, SavedViewLayout};
use std::collections::{BTreeMap, BTreeSet};
use tempfile::tempdir;

#[test]
fn transcript_index_is_derived_search_state_and_excludes_reasoning() {
    use crate::conversation::{ConversationItem, ConversationItemKind, ConversationPage};
    use crate::domain::ThreadId;

    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let page = ConversationPage {
        thread_id: ThreadId::new("thread-audio"),
        title: Some("Audio regression".into()),
        turns: vec![],
        items: vec![
            ConversationItem {
                turn_id: "turn-1".into(),
                item_id: "user-1".into(),
                kind: ConversationItemKind::User,
                text: "远场 audio regression appears".into(),
                status: None,
            },
            ConversationItem {
                turn_id: "turn-1".into(),
                item_id: "assistant-1".into(),
                kind: ConversationItemKind::Assistant,
                text: "check the AEC pipeline".into(),
                status: None,
            },
            ConversationItem {
                turn_id: "turn-1".into(),
                item_id: "reasoning-1".into(),
                kind: ConversationItemKind::Reasoning,
                text: "private-search-sentinel".into(),
                status: None,
            },
        ],
        next_turn_cursor: None,
        next_item_cursor: None,
    };

    assert_eq!(store.index_conversation_page(&page).expect("index"), 2);
    let audio = store.search_transcript("audio", 20).expect("search");
    assert_eq!(audio.source, TranscriptSearchSource::LocalFts);
    assert_eq!(audio.hits.len(), 1);
    assert_eq!(audio.hits[0].thread_id.0, "thread-audio");
    assert_eq!(audio.hits[0].item_id.as_deref(), Some("user-1"));
    assert!(
        !audio.complete,
        "local cache is never full-history authority"
    );

    let private = store
        .search_transcript("private-search-sentinel", 20)
        .expect("reasoning search");
    assert!(
        private.hits.is_empty(),
        "reasoning must never enter local FTS"
    );

    let short = store
        .search_transcript("远场", 20)
        .expect("short LIKE fallback");
    assert_eq!(short.hits.len(), 1);
}

#[test]
fn local_short_search_treats_sql_like_metacharacters_as_literals() {
    use crate::conversation::{ConversationItem, ConversationItemKind, ConversationPage};
    use crate::domain::ThreadId;
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let page = ConversationPage {
        thread_id: ThreadId::new("literal-like"),
        title: None,
        turns: vec![],
        items: vec![
            ConversationItem {
                turn_id: "t".into(),
                item_id: "literal".into(),
                kind: ConversationItemKind::User,
                text: "100% under_score C:\\data".into(),
                status: None,
            },
            ConversationItem {
                turn_id: "t".into(),
                item_id: "ordinary".into(),
                kind: ConversationItemKind::User,
                text: "100x underXscore C:/data".into(),
                status: None,
            },
        ],
        next_turn_cursor: None,
        next_item_cursor: None,
    };
    store.index_conversation_page(&page).expect("index");
    for literal in ["%", "_", "\\"] {
        let result = store
            .search_transcript(literal, 20)
            .expect("literal search");
        assert_eq!(result.hits.len(), 1, "query {literal:?} must not wildcard");
        assert_eq!(result.hits[0].item_id.as_deref(), Some("literal"));
        assert!(!result.complete);
    }
}

#[test]
fn transcript_title_and_text_persistence_share_explicit_size_budget() {
    use crate::conversation::{ConversationItem, ConversationItemKind, ConversationPage};
    use crate::domain::ThreadId;

    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let page = ConversationPage {
        thread_id: ThreadId::new("bounded-title"),
        title: Some("T".repeat(TRANSCRIPT_MAX_CHARS + 50)),
        turns: vec![],
        items: vec![ConversationItem {
            turn_id: "turn".into(),
            item_id: "item".into(),
            kind: ConversationItemKind::Assistant,
            text: "S".repeat(TRANSCRIPT_MAX_CHARS + 50),
            status: None,
        }],
        next_turn_cursor: None,
        next_item_cursor: None,
    };
    store.index_conversation_page(&page).expect("bounded index");
    let conn = store.open_ready().expect("open database");
    let (title_len, text_len): (i64, i64) = conn
        .query_row(
            "SELECT length(title), length(text) FROM transcript_documents
             WHERE thread_id='bounded-title' AND item_id='item'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("bounded stored values");
    assert_eq!(title_len, TRANSCRIPT_MAX_CHARS as i64);
    assert_eq!(text_len, TRANSCRIPT_MAX_CHARS as i64);
}

#[test]
fn rehydrating_transcript_does_not_extend_retention() {
    use crate::conversation::{ConversationItem, ConversationItemKind, ConversationPage};
    use crate::domain::ThreadId;

    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let page = ConversationPage {
        thread_id: ThreadId::new("ttl-fixture"),
        title: Some("First seen retention".into()),
        turns: vec![],
        items: vec![ConversationItem {
            turn_id: "turn".into(),
            item_id: "item".into(),
            kind: ConversationItemKind::User,
            text: "sensitive-lifetime-sentinel".into(),
            status: None,
        }],
        next_turn_cursor: None,
        next_item_cursor: None,
    };
    store.index_conversation_page(&page).expect("first index");
    let first_seen = u64_to_i64(now_unix_ms().saturating_sub(TRANSCRIPT_RETENTION_MS / 2))
        .expect("valid first seen time");
    let conn = store.open_ready().expect("open database");
    conn.execute(
        "UPDATE transcript_documents SET observed_at_unix_ms=?1 WHERE item_id='item'",
        [first_seen],
    )
    .expect("fixture timestamp");
    drop(conn);
    store
        .index_conversation_page(&page)
        .expect("replay existing item");
    let conn = store.open_ready().expect("check database");
    let kept: i64 = conn
        .query_row(
            "SELECT observed_at_unix_ms FROM transcript_documents WHERE item_id='item'",
            [],
            |row| row.get(0),
        )
        .expect("first-seen value");
    assert_eq!(kept, first_seen, "replay must not renew retention");
    conn.execute(
        "UPDATE transcript_documents SET observed_at_unix_ms=1 WHERE item_id='item'",
        [],
    )
    .expect("expire cached item");
    drop(conn);
    store
        .index_conversation_page(&page)
        .expect("replay expired item");
    assert!(
        store
            .search_transcript("sensitive-lifetime-sentinel", 10)
            .expect("post-expiry search")
            .hits
            .is_empty(),
        "old user text must not survive replay in local FTS"
    );
}

#[test]
fn transcript_retention_removes_stale_fts_and_opt_out_clears_remaining_text() {
    use crate::conversation::{ConversationItem, ConversationItemKind, ConversationPage};
    use crate::domain::ThreadId;

    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let item = |id: &str, text: &str| ConversationItem {
        turn_id: "turn".into(),
        item_id: id.into(),
        kind: ConversationItemKind::User,
        text: text.into(),
        status: None,
    };
    let page = ConversationPage {
        thread_id: ThreadId::new("private"),
        title: None,
        turns: vec![],
        items: vec![
            item("old", "old-private-needle"),
            item("new", "new-private-needle"),
        ],
        next_turn_cursor: None,
        next_item_cursor: None,
    };
    store.index_conversation_page(&page).expect("index fixture");
    let conn = store.open_ready().expect("fixture connection");
    conn.execute(
        "UPDATE transcript_documents SET observed_at_unix_ms=1 WHERE item_id='old'",
        [],
    )
    .expect("age old row");
    drop(conn);
    store.prune_transcript_index().expect("retention prune");
    assert!(
        store
            .search_transcript("old-private-needle", 10)
            .expect("search old")
            .hits
            .is_empty()
    );
    assert_eq!(
        store
            .search_transcript("new-private-needle", 10)
            .expect("search new")
            .hits
            .len(),
        1
    );
    store.clear_transcript_index().expect("opt-out purge");
    assert!(
        store
            .search_transcript("new-private-needle", 10)
            .expect("search after purge")
            .hits
            .is_empty()
    );
}

#[test]
fn local_batch_is_atomic_when_a_frozen_target_disappears() {
    use crate::batch_local::{LocalBatchAction, LocalBatchPlan, LocalBatchTarget};

    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let scratch = store
        .create_scratch("one", None, None, None)
        .expect("scratch");
    let plan = LocalBatchPlan {
        action: LocalBatchAction::SetDone(true),
        targets: vec![
            LocalBatchTarget {
                local_id: scratch.id.clone(),
                anchor: SourceRef {
                    kind: SourceKind::ScratchWork,
                    value: scratch.id.clone(),
                },
                title: scratch.title.clone(),
            },
            LocalBatchTarget {
                local_id: "scratch:999999".into(),
                anchor: SourceRef {
                    kind: SourceKind::ScratchWork,
                    value: "scratch:999999".into(),
                },
                title: "gone".into(),
            },
        ],
        planned_at_unix_ms: 100,
    };

    assert!(store.apply_local_batch(&plan).is_err());
    let snapshot = store.load_planning_snapshot().expect("snapshot");
    assert_eq!(snapshot.scratch[0].state, ScratchState::Inbox);
}

#[test]
fn local_batch_updates_scratch_authority_and_thread_overlay_in_one_api() {
    use crate::batch_local::{LocalBatchAction, LocalBatchPlan, LocalBatchTarget};

    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let scratch = store
        .create_scratch("scratch", None, None, None)
        .expect("scratch");
    let scratch_plan = LocalBatchPlan {
        action: LocalBatchAction::SetPriority(7),
        targets: vec![LocalBatchTarget {
            local_id: scratch.id.clone(),
            anchor: SourceRef {
                kind: SourceKind::ScratchWork,
                value: scratch.id.clone(),
            },
            title: scratch.title.clone(),
        }],
        planned_at_unix_ms: 100,
    };
    store
        .apply_local_batch(&scratch_plan)
        .expect("scratch batch");

    let thread_plan = LocalBatchPlan {
        action: LocalBatchAction::AddTag("focus".into()),
        targets: vec![LocalBatchTarget {
            local_id: "thread:thread-1".into(),
            anchor: SourceRef::codex_thread(&crate::domain::ThreadId::new("thread-1")),
            title: "thread".into(),
        }],
        planned_at_unix_ms: 100,
    };
    store.apply_local_batch(&thread_plan).expect("thread batch");

    let snapshot = store.load_planning_snapshot().expect("snapshot");
    assert_eq!(snapshot.scratch[0].priority, Some(7));
    let thread = snapshot
        .cards
        .iter()
        .find(|card| card.anchor == thread_plan.targets[0].anchor)
        .expect("thread overlay");
    assert!(thread.overlay.tags.contains("focus"));
}

#[test]
fn first_install_ignores_obsolete_json_state_without_modifying_it() {
    let root = tempdir().expect("tempdir");
    let obsolete = root.path().join("state").join("state-v1.json");
    fs::create_dir_all(obsolete.parent().unwrap()).expect("state directory");
    let old_bytes = br#"{"schemaVersion":1,"pins":["old-session"]}"#;
    fs::write(&obsolete, old_bytes).expect("write obsolete state");
    let store = SqliteStore::at(root.path());

    let state = store.load_state().expect("fresh SQLite");
    assert!(state.pins.is_empty(), "old JSON state must not be imported");
    assert_eq!(fs::read(&obsolete).expect("read obsolete state"), old_bytes);
    assert!(store.db_path().exists());
    let health = store.health().expect("fresh health");
    assert_eq!(health.schema_version, DB_SCHEMA_VERSION);
    assert_eq!(health.integrity, "ok");
}

#[test]
fn planning_relations_round_trip_without_copying_external_state() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());

    let mut card = WorkCardRecord::implicit_thread(&crate::domain::ThreadId::new("thread-1"));
    card.overlay.note = Some("local note".into());
    card.overlay.tags = BTreeSet::from(["kws".into(), "p0".into()]);
    card.links.push(WorkCardLink {
        role: LinkRole::Worktree,
        source: SourceRef {
            kind: SourceKind::Worktree,
            value: "/repo/wt".into(),
        },
    });
    store.upsert_work_card(&card).expect("card");

    let scratch = store
        .create_scratch(
            "Investigate wake miss",
            Some("local only"),
            Some("kws"),
            Some(1),
        )
        .expect("scratch");

    let view = store
        .save_view(&SavedView {
            id: String::new(),
            name: "Needs Review".into(),
            source_scope: "all".into(),
            filter: "stage:review".into(),
            group_by: Some("workspace".into()),
            order_by: Some("priority".into()),
            layout: SavedViewLayout::Board,
            visible_fields: vec!["stage".into(), "attention".into()],
        })
        .expect("view");

    let snapshot = store.load_planning_snapshot().expect("snapshot");
    assert_eq!(snapshot.cards, vec![card]);
    assert_eq!(snapshot.scratch, vec![scratch]);
    assert_eq!(snapshot.saved_views, vec![view]);
}

#[test]
fn anchor_uniqueness_prevents_duplicate_active_work_cards() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let first = WorkCardRecord::implicit_thread(&crate::domain::ThreadId::new("thread-1"));
    store.upsert_work_card(&first).expect("first");

    let mut duplicate = first.clone();
    duplicate.local_id = "another-local-id".into();
    let error = store
        .upsert_work_card(&duplicate)
        .expect_err("duplicate anchor must fail");
    assert!(error.to_string().contains("WorkCard"));
}

#[test]
fn local_notes_bookmarks_and_hot_slots_round_trip() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let thread = SourceRef {
        kind: SourceKind::CodexThread,
        value: "thread-1".into(),
    };

    store
        .upsert_note(&LocalNote {
            owner: thread.clone(),
            text: "remember this".into(),
            updated_at_unix_ms: 10,
        })
        .expect("note");
    let bookmark = store
        .create_bookmark(thread.clone(), Some("review"), Some("line 42"))
        .expect("bookmark");
    store.set_hot_slot(3, thread.clone()).expect("hot slot");

    let snapshot = store.load_planning_snapshot().expect("snapshot");
    assert_eq!(snapshot.notes.len(), 1);
    assert_eq!(snapshot.bookmarks, vec![bookmark.clone()]);
    assert_eq!(snapshot.hot_slots.len(), 1);
    assert_eq!(snapshot.hot_slots[0].slot, 3);
    assert_eq!(snapshot.hot_slots[0].target, thread);

    store
        .delete_bookmark(&bookmark.id)
        .expect("delete bookmark");
    store.clear_hot_slot(3).expect("clear hot slot");
    store
        .delete_note(&snapshot.notes[0].owner)
        .expect("delete note");
    let snapshot = store
        .load_planning_snapshot()
        .expect("snapshot after delete");
    assert!(snapshot.notes.is_empty());
    assert!(snapshot.bookmarks.is_empty());
    assert!(snapshot.hot_slots.is_empty());
}

#[test]
fn managed_worktree_and_unknown_receipt_survive_restart() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let repo = crate::domain::LocalRepoIdentity {
        git_common_dir: "/repo/.git".into(),
        primary_root: "/repo".into(),
    };
    let plan = OperationPlan::create_worktree(
        repo.clone(),
        "/repo".into(),
        "/tmp/wt-feature".into(),
        "feature".into(),
        "HEAD".into(),
        10,
    );
    let mut receipt = OperationReceipt::planned(plan.clone());
    receipt.start(11);
    receipt.outcome_unknown(12, "git process timed out".into());
    store
        .save_operation_receipt(&receipt)
        .expect("save unknown receipt");

    let managed = ManagedWorktreeRecord {
        repo: repo.clone(),
        canonical_path: "/tmp/wt-feature".into(),
        branch: Some("feature".into()),
        created_by_operation_id: plan.operation_id.clone(),
        adopted: false,
        created_at_unix_ms: 10,
        last_verified_at_unix_ms: 13,
    };
    store
        .upsert_managed_worktree(&managed)
        .expect("save managed worktree");

    let reopened = SqliteStore::at(root.path());
    assert_eq!(
        reopened
            .operation_receipt(&plan.operation_id)
            .expect("load receipt"),
        Some(receipt)
    );
    assert_eq!(
        reopened.load_managed_worktrees().expect("managed list"),
        vec![managed]
    );
}

#[test]
fn recoverable_receipts_only_return_planned_executing_and_unknown() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let repo = crate::domain::LocalRepoIdentity {
        git_common_dir: "/repo/.git".into(),
        primary_root: "/repo".into(),
    };

    let mut planned = OperationReceipt::planned(OperationPlan::delete_branch(
        repo.clone(),
        "/repo".into(),
        "planned".into(),
        1,
    ));
    let mut executing = OperationReceipt::planned(OperationPlan::delete_branch(
        repo.clone(),
        "/repo".into(),
        "executing".into(),
        2,
    ));
    executing.start(3);
    let mut unknown = OperationReceipt::planned(OperationPlan::delete_branch(
        repo.clone(),
        "/repo".into(),
        "unknown".into(),
        4,
    ));
    unknown.start(5);
    unknown.outcome_unknown(6, "timeout".into());
    let mut succeeded = OperationReceipt::planned(OperationPlan::delete_branch(
        repo,
        "/repo".into(),
        "done".into(),
        7,
    ));
    succeeded.start(8);
    succeeded.succeed(9, "done".into(), "verified".into());

    for receipt in [&planned, &executing, &unknown, &succeeded] {
        store.save_operation_receipt(receipt).expect("save receipt");
    }

    let recoverable = store
        .load_recoverable_operation_receipts()
        .expect("recoverable");
    assert_eq!(recoverable.len(), 3);
    assert!(recoverable.iter().all(|receipt| matches!(
        receipt.state,
        OperationState::Planned | OperationState::Executing | OperationState::OutcomeUnknown
    )));

    planned.fail(10, "cancelled".into());
    store
        .save_operation_receipt(&planned)
        .expect("update planned");
    assert_eq!(
        store
            .load_recoverable_operation_receipts()
            .expect("recoverable after update")
            .len(),
        2
    );
}

fn forge_identity() -> ForgeIdentity {
    ForgeIdentity {
        provider: ForgeProviderKind::GitLab,
        host: "gitlab.example.com".into(),
        project_id: "42".into(),
        path_with_namespace: "team/repo".into(),
        web_url: "https://gitlab.example.com/team/repo".into(),
        default_branch: Some("main".into()),
    }
}

#[test]
fn forge_mutation_receipts_round_trip_and_comment_body_is_never_persisted() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let secret_body = "review comment that must stay memory-only";
    let plan = ForgeMutationPlan::comment_merge_request(
        &forge_identity(),
        "/repo".into(),
        7,
        "feature/m6b".into(),
        "main".into(),
        secret_body.len(),
        10,
    )
    .expect("plan");
    let mut receipt = ForgeMutationReceipt::planned(plan.clone());
    receipt.start(11);
    receipt.outcome_unknown(12, "transport timeout".into());
    store
        .save_forge_mutation_receipt(&receipt)
        .expect("save forge receipt");

    assert_eq!(
        store
            .forge_mutation_receipt(&plan.operation_id)
            .expect("load forge receipt"),
        Some(receipt.clone())
    );

    let recoverable = store
        .load_recoverable_forge_mutation_receipts()
        .expect("recoverable forge receipts");
    assert_eq!(recoverable, vec![receipt]);

    let conn = Connection::open(store.db_path()).expect("open raw database");
    let plan_json: String = conn
        .query_row(
            "SELECT plan_json FROM forge_mutation_receipts WHERE operation_id=?1",
            [&plan.operation_id],
            |row| row.get(0),
        )
        .expect("stored plan json");
    assert!(!plan_json.contains(secret_body));
    assert!(plan_json.contains("\"payload_bytes\""));
}

#[test]
fn forge_recoverable_query_excludes_terminal_receipts() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());

    let planned_plan = ForgeMutationPlan::approve_merge_request(
        &forge_identity(),
        "/repo".into(),
        7,
        "feature".into(),
        "main".into(),
        1,
    )
    .expect("planned");
    let planned = ForgeMutationReceipt::planned(planned_plan);

    let succeeded_plan = ForgeMutationPlan::merge_merge_request(
        &forge_identity(),
        "/repo".into(),
        8,
        "feature-2".into(),
        "main".into(),
        2,
    )
    .expect("succeeded");
    let mut succeeded = ForgeMutationReceipt::planned(succeeded_plan);
    succeeded.start(3);
    succeeded.succeed(4, "mr:8".into(), "verified".into());

    store
        .save_forge_mutation_receipt(&planned)
        .expect("save planned");
    store
        .save_forge_mutation_receipt(&succeeded)
        .expect("save succeeded");

    let recoverable = store
        .load_recoverable_forge_mutation_receipts()
        .expect("recoverable");
    assert_eq!(recoverable.len(), 1);
    assert_eq!(recoverable[0].state, OperationState::Planned);
}

#[test]
fn config_remains_toml_while_runtime_state_moves_to_sqlite() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let config = store.load_config().expect("config");
    assert!(config.ui.mouse);
    assert!(store.config_store.config_path().exists());
    store
        .save_state(&LocalStateV1::default())
        .expect("save state");
    assert!(store.db_path().exists());
}

#[cfg(unix)]
#[test]
fn sqlite_open_rejects_symlinked_database_before_touching_target() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let target = root.path().join("unrelated-data");
    fs::write(&target, b"preserve unrelated contents").expect("target");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).expect("target mode");
    fs::create_dir_all(store.db_path().parent().expect("state parent"))
        .expect("create state dir");
    symlink(&target, store.db_path()).expect("install unsafe link");
    let err = store
        .load_state()
        .expect_err("SQLite must reject file link");
    assert!(format!("{err:#}").contains("must be a regular file"));
    assert_eq!(
        fs::read(&target).expect("target still readable"),
        b"preserve unrelated contents"
    );
    assert_eq!(
        fs::metadata(&target)
            .expect("target metadata")
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
}

#[cfg(unix)]
#[test]
fn sqlite_writer_lock_rejects_symlinked_file_without_touching_target() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let target = root.path().join("unrelated-owner-target");
    fs::write(&target, b"preserve owner target").expect("target");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).expect("target mode");
    let lock_path = store.db_path().with_extension("sqlite3.owner-lock");
    fs::create_dir_all(lock_path.parent().expect("owner directory")).expect("state dir");
    symlink(&target, &lock_path).expect("unsafe owner symlink");
    let err = store
        .acquire_local_writer()
        .err()
        .expect("reject linked owner");
    assert!(format!("{err:#}").contains("must be a regular file"));
    assert_eq!(
        fs::read(&target).expect("target still readable"),
        b"preserve owner target"
    );
    assert_eq!(
        fs::metadata(&target)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
}

#[test]
fn sqlite_path_preflight_rejects_non_regular_file() {
    let root = tempdir().expect("tempdir");
    assert!(refuse_unsafe_sqlite_path(root.path()).is_err());
    assert!(refuse_unsafe_sqlite_path(&root.path().join("new.db")).is_ok());
}

#[test]
fn local_writer_is_exclusive_and_releases_after_drop() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let owner = store.acquire_local_writer().expect("first writer");
    let contender = SqliteStore::at(root.path());
    let denied = contender
        .acquire_local_writer()
        .err()
        .expect("second writer must be denied");
    assert!(format!("{denied:#}").contains("another writer"));
    assert!(
        store
            .restore_recovery_backup(root.path().join("backup.sqlite3"))
            .is_err(),
        "offline restore must refuse while TUI owns state"
    );
    drop(owner);
    let _second = contender.acquire_local_writer().expect("released lock");
}

#[test]
fn integrity_scan_remains_on_explicit_health_not_per_operation() {
    let source = include_str!("../sqlite_store.rs");
    let health = source
        .split("pub fn health(&self)")
        .nth(1)
        .expect("health method");
    assert!(
        health
            .split("pub fn create_recovery_backup")
            .next()
            .expect("health method body")
            .contains("quick_check(&conn)?")
    );
    let hot_path = source
        .split("fn open_ready(&self)")
        .nth(1)
        .expect("open_ready method")
        .split("impl LocalStore")
        .next()
        .expect("open_ready body");
    assert!(!hot_path.contains("quick_check(&conn)"));
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    // The existing health API keeps explicit corruption screening.
    assert_eq!(store.health().expect("health").integrity, "ok");
}

#[test]
fn default_state_round_trips_through_sqlite() {
    let root = tempdir().expect("tempdir");
    let store = SqliteStore::at(root.path());
    let state = LocalStateV1 {
        schema_version: 1,
        thread_ui: BTreeMap::new(),
        pins: BTreeSet::new(),
        aliases: BTreeMap::new(),
        marked_unread: BTreeSet::new(),
        acknowledged_attention: BTreeSet::new(),
        host_local_only: true,
        repo_backed_only: true,
    };
    store.save_state(&state).expect("save");
    assert_eq!(store.load_state().expect("load"), state);
}
