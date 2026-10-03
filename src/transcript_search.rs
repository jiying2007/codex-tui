use crate::domain::ThreadId;
use anyhow::{Context, Result};
use serde_json::Value;

pub const TRANSCRIPT_SEARCH_THREAD_LIMIT: u32 = 24;
pub const TRANSCRIPT_SEARCH_OCCURRENCE_LIMIT: u32 = 6;
pub const TRANSCRIPT_SEARCH_RESULT_LIMIT: usize = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranscriptSearchSource {
    LocalFts,
    AppServer,
}

impl TranscriptSearchSource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalFts => "local-fts",
            Self::AppServer => "app-server",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptSearchHit {
    pub thread_id: ThreadId,
    pub turn_id: Option<String>,
    pub item_id: Option<String>,
    pub snippet: String,
    pub turn_cursor: Option<String>,
    pub match_start_utf16: Option<u32>,
    pub match_end_utf16: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptSearchResults {
    pub query: String,
    pub source: TranscriptSearchSource,
    pub hits: Vec<TranscriptSearchHit>,
    pub complete: bool,
}

impl TranscriptSearchResults {
    pub fn empty(query: impl Into<String>, source: TranscriptSearchSource) -> Self {
        Self {
            query: query.into(),
            source,
            hits: vec![],
            complete: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadSearchCandidate {
    pub thread_id: ThreadId,
    pub snippet: String,
}

pub fn parse_thread_search(result: Value) -> Result<(Vec<ThreadSearchCandidate>, Option<String>)> {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .context("thread/search response missing data")?;
    let mut candidates = Vec::with_capacity(data.len());
    for entry in data {
        let thread_id = entry
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .context("thread/search result missing thread.id")?;
        let snippet = entry
            .get("snippet")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        candidates.push(ThreadSearchCandidate {
            thread_id: ThreadId::new(thread_id),
            snippet,
        });
    }
    let next_cursor = result
        .get("nextCursor")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    Ok((candidates, next_cursor))
}

pub fn parse_search_occurrences(
    thread_id: ThreadId,
    result: Value,
) -> Result<(Vec<TranscriptSearchHit>, Option<String>)> {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .context("thread/searchOccurrences response missing data")?;
    let mut hits = Vec::with_capacity(data.len());
    for entry in data {
        let turn_id = entry
            .get("turnId")
            .and_then(Value::as_str)
            .context("search occurrence missing turnId")?;
        let item_id = entry
            .get("itemId")
            .and_then(Value::as_str)
            .context("search occurrence missing itemId")?;
        let snippet = entry
            .get("snippet")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let turn_cursor = entry
            .get("turnCursor")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let range = entry.get("snippetMatchRange");
        hits.push(TranscriptSearchHit {
            thread_id: thread_id.clone(),
            turn_id: Some(turn_id.to_string()),
            item_id: Some(item_id.to_string()),
            snippet,
            turn_cursor,
            match_start_utf16: range
                .and_then(|value| value.get("start"))
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
            match_end_utf16: range
                .and_then(|value| value.get("end"))
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
        });
    }
    let next_cursor = result
        .get("nextCursor")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    Ok((hits, next_cursor))
}

pub fn thread_level_hit(candidate: ThreadSearchCandidate) -> TranscriptSearchHit {
    TranscriptSearchHit {
        thread_id: candidate.thread_id,
        turn_id: None,
        item_id: None,
        snippet: candidate.snippet,
        turn_cursor: None,
        match_start_utf16: None,
        match_end_utf16: None,
    }
}

pub fn fts_match_query(raw: &str) -> Option<String> {
    let query = raw.trim();
    if query.is_empty() {
        return None;
    }
    Some(format!("\"{}\"", query.replace('"', "\"\"")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_thread_search_and_occurrences_without_losing_identity() {
        let (threads, next) = parse_thread_search(json!({
            "data": [{
                "thread": {"id": "thread-1"},
                "snippet": "audio regression"
            }],
            "nextCursor": "next-thread"
        }))
        .expect("thread search");
        assert_eq!(threads[0].thread_id.0, "thread-1");
        assert_eq!(next.as_deref(), Some("next-thread"));

        let (hits, next) = parse_search_occurrences(
            ThreadId::new("thread-1"),
            json!({
                "data": [{
                    "turnId": "turn-7",
                    "itemId": "item-9",
                    "snippet": "audio regression happened",
                    "snippetMatchRange": {"start": 0, "end": 5},
                    "turnCursor": "turn-cursor-7"
                }],
                "nextCursor": null
            }),
        )
        .expect("occurrences");
        assert_eq!(hits[0].turn_id.as_deref(), Some("turn-7"));
        assert_eq!(hits[0].item_id.as_deref(), Some("item-9"));
        assert_eq!(hits[0].turn_cursor.as_deref(), Some("turn-cursor-7"));
        assert_eq!(hits[0].match_start_utf16, Some(0));
        assert_eq!(hits[0].match_end_utf16, Some(5));
        assert!(next.is_none());
    }

    #[test]
    fn fts_match_query_is_literal_phrase_and_rejects_empty_input() {
        assert_eq!(
            fts_match_query(" audio regression "),
            Some("\"audio regression\"".into())
        );
        assert_eq!(
            fts_match_query("say \"hello\""),
            Some("\"say \"\"hello\"\"\"".into())
        );
        assert_eq!(fts_match_query("  "), None);
    }
}
