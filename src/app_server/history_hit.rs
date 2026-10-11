//! Exact search-hit navigation with a bounded global-history cursor contract.
use super::*;

pub(super) async fn load_transcript_hit(
    rpc: &mut RpcSession,
    hit: &crate::transcript_search::TranscriptSearchHit,
) -> Result<ConversationPage> {
    if hit.turn_id.is_none() && hit.item_id.is_none() {
        return load_conversation(rpc, hit.thread_id.clone()).await;
    }
    // Includes metadata and all pages, independently of per-RPC timeouts.
    tokio::time::timeout(SEARCH_TOTAL_TIMEOUT, load_exact_hit(rpc, hit))
        .await
        .context("transcript target lookup total deadline exceeded")?
}

async fn load_exact_hit(
    rpc: &mut RpcSession,
    hit: &crate::transcript_search::TranscriptSearchHit,
) -> Result<ConversationPage> {
    let metadata = rpc
        .request(
            "thread/read",
            json!({"threadId": hit.thread_id.0, "includeTurns": false}),
        )
        .await
        .context("read transcript search thread metadata")?;
    let title = parse_thread_title(&metadata);
    let mut budget = PageBudget::with_deadline(SEARCH_TOTAL_TIMEOUT);
    let mut cursor = None;
    loop {
        budget.remaining()?;
        // Same global, descending item scope used by LoadOlderConversation.
        // A turn-scoped ascending cursor must never escape into that path.
        let result = rpc.request("thread/items/list", json!({
            "threadId": hit.thread_id.0, "cursor": cursor, "limit": PAGE_SIZE, "sortDirection": "desc"
        })).await.context("load transcript target page")?;
        let (mut items, next) = parse_items_page(result)?;
        anyhow::ensure!(
            items.len() <= PAGE_SIZE as usize,
            "transcript page exceeds requested row limit"
        );
        let identities: Vec<_> = items
            .iter()
            .map(|item| format!("{}:{}:{}", item.turn_id.len(), item.turn_id, item.item_id))
            .collect();
        budget.observe(next.as_deref(), identities.iter().map(String::as_str))?;
        if items.iter().any(|item| matches_hit(item, hit)) {
            items.reverse();
            return Ok(merge_history(
                hit.thread_id.clone(),
                title,
                vec![],
                items,
                None,
                next,
            ));
        }
        match next {
            Some(next) => cursor = Some(next),
            None => {
                anyhow::bail!("transcript target not found; it may have been deleted or changed")
            }
        }
    }
}

fn matches_hit(
    item: &crate::conversation::ConversationItem,
    hit: &crate::transcript_search::TranscriptSearchHit,
) -> bool {
    hit.turn_id
        .as_ref()
        .is_none_or(|turn| turn == &item.turn_id)
        && hit.item_id.as_ref().is_none_or(|id| id == &item.item_id)
}
