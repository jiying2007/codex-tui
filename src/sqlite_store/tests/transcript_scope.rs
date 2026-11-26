use super::super::*;
use crate::conversation::{ConversationItem, ConversationItemKind, ConversationPage};
use crate::domain::ThreadId;
use tempfile::tempdir;

#[test]
fn transcript_cache_isolates_same_ids_and_legacy_text_in_fts_and_like() {
    use crate::conversation::ConversationItem;
    use crate::domain::ThreadId;
    let root = tempdir().unwrap();
    let store = SqliteStore::at(root.path());
    let mut page = ConversationPage {
        thread_id: ThreadId::new("same-id"),
        title: None,
        turns: vec![],
        items: vec![ConversationItem {
            turn_id: "turn".into(),
            item_id: "item".into(),
            kind: ConversationItemKind::User,
            text: "common A-only 正文".into(),
            status: None,
        }],
        next_turn_cursor: None,
        next_item_cursor: None,
    };
    store.index_conversation_page("A", &page).unwrap();
    page.items[0].text = "common B-only 正文".into();
    store.index_conversation_page("B", &page).unwrap();
    let conn = store.open_ready().unwrap();
    // Even an old TEXT key with the exact encoded spelling cannot collide.
    conn.execute(
        "INSERT INTO transcript_documents VALUES('1:A:same-id','turn','item','user',NULL,'common legacy-only',?1)",
        [u64_to_i64(now_unix_ms()).unwrap()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO transcript_fts VALUES('1:A:same-id','turn','item','user',NULL,'common legacy-only')",
        [],
    )
    .unwrap();
    for has_fts in [true, false] {
        if !has_fts {
            conn.execute("DROP TABLE transcript_fts", []).unwrap();
        }
        for (scope, own, other) in [("A", "A-only", "B-only"), ("B", "B-only", "A-only")] {
            for query in ["common", "正文"] {
                let hits = search_transcript(&conn, scope, query, 1).unwrap().hits;
                assert_eq!(hits.len(), 1);
                assert_eq!(hits[0].thread_id, page.thread_id);
                assert_eq!(hits[0].item_id.as_deref(), Some("item"));
                assert!(hits[0].snippet.contains(own));
                assert!(!hits[0].snippet.contains(other));
            }
            for query in [other, "legacy-only"] {
                assert!(
                    search_transcript(&conn, scope, query, 10)
                        .unwrap()
                        .hits
                        .is_empty()
                );
            }
        }
        assert!(search_transcript(&conn, "", "common", 10).is_err());
    }
    let count: i64 = conn
        .query_row("SELECT count(*) FROM transcript_documents", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 3);
}
