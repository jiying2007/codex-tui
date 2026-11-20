//! Bounded source-aware registry queries: never change filters mid-cursor.
use super::*;

/// Omitting sourceKinds (or sending []) returns only interactive threads upstream.
const REGISTRY_SOURCE_KINDS: &[&str] = &[
    "cli", "vscode", "exec", "appServer", "subAgent", "subAgentReview",
    "subAgentCompact", "subAgentThreadSpawn", "subAgentOther", "unknown",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RegistrySourceScope {
    RequestedKnownKinds,
    InteractiveFallback,
}

impl RegistrySourceScope {
    fn known_kinds_requested(self) -> bool {
        matches!(self, Self::RequestedKnownKinds)
    }
}

fn is_registry_source_filter_unsupported(error: &anyhow::Error) -> bool {
    let Some(source) = error.downcast_ref::<RpcResponseError>() else {
        return false;
    };
    if source.method != "thread/list" || !matches!(source.code, Some(-32600 | -32602)) {
        return false;
    }
    let message = source.message.to_ascii_lowercase();
    [
        "sourcekinds", "source kinds", "source kind",
        "modelproviders", "model providers",
        "unknown variant", "invalid variant",
        "unknown field", "unexpected field",
    ]
    .into_iter()
    .any(|part| message.contains(part))
}

pub(super) fn apply_registry_source_coverage(status: &mut BackendStatus, scope: RegistrySourceScope) {
    let requested = scope.known_kinds_requested();
    mark_optional_capability(status, "thread/list:known-source-kinds", requested);
    if !requested {
        status.registry_complete = false;
        status.error.get_or_insert_with(|| {
            "Registry source coverage partial: target rejected explicit sourceKinds; only interactive threads are visible".into()
        });
    }
}

fn registry_query_params(
    cursor: Option<&str>,
    use_state_db_only: bool,
    optimized: bool,
    source_scope: RegistrySourceScope,
) -> Value {
    let mut params = json!({"cursor": cursor, "limit": PAGE_SIZE});
    if optimized {
        params["sortKey"] = json!("recency_at");
        params["sortDirection"] = json!("desc");
        params["useStateDbOnly"] = json!(use_state_db_only);
    }
    if source_scope.known_kinds_requested() {
        params["sourceKinds"] = json!(REGISTRY_SOURCE_KINDS);
        // Upstream [] means every provider; unlike sourceKinds, omission is also unrestricted.
        params["modelProviders"] = json!([]);
    }
    params
}

pub(super) async fn load_registry_page(
    rpc: &mut RpcSession,
    cursor: Option<String>,
    use_state_db_only: bool,
    mut optimized_query: bool,
    mut source_scope: RegistrySourceScope,
) -> Result<RegistryPage> {
    // Downgrade only on the FIRST page: changing filter semantics mid-cursor
    // would fabricate a complete inventory from incompatible result sets.
    let result = loop {
        let params = registry_query_params(
            cursor.as_deref(),
            use_state_db_only,
            optimized_query,
            source_scope,
        );
        match rpc.request("thread/list", params).await {
            Ok(result) => break result,
            Err(error) if optimized_query && is_registry_query_optimization_unsupported(&error) => {
                optimized_query = false;
            }
            Err(error)
                if cursor.is_none()
                    && source_scope.known_kinds_requested()
                    && is_registry_source_filter_unsupported(&error) =>
            {
                source_scope = RegistrySourceScope::InteractiveFallback;
                optimized_query = false;
            }
            Err(error) => return Err(error),
        }
    };

    let page = parse_thread_list(result)?;
    Ok(RegistryPage {
        threads: page
            .data
            .into_iter()
            .map(|thread| normalize_thread(thread, None))
            .collect(),
        next_cursor: page.next_cursor,
        optimized_query,
        source_scope,
    })
}

pub(super) async fn load_registry_with_page_limit(
    rpc: &mut RpcSession,
    use_state_db_only: bool,
    max_pages: Option<usize>,
) -> Result<RegistryLoad> {
    let mut cursor = None;
    let mut threads = BTreeMap::new();
    let mut optimized_query = true;
    let mut source_scope = RegistrySourceScope::RequestedKnownKinds;
    let mut budget = PageBudget::default();
    loop {
        let page = match guarded_registry_page(
            rpc,
            cursor,
            use_state_db_only,
            optimized_query,
            source_scope,
            &mut budget,
        )
        .await
        {
            Ok(page) => page,
            Err(error) if !threads.is_empty() => {
                return Ok(RegistryLoad {
                    threads: threads.into_values().collect(),
                    error: Some(format!("Registry inventory incomplete: {error}")),
                    registry_complete: false,
                    source_scope,
                    hydration: None,
                });
            }
            Err(error) => return Err(error),
        };
        optimized_query = page.optimized_query;
        source_scope = page.source_scope;
        for thread in page.threads {
            threads.insert(thread.id.0.clone(), thread);
        }
        let Some(next) = page.next_cursor else {
            return Ok(RegistryLoad {
                threads: threads.into_values().collect(),
                error: None,
                registry_complete: true,
                source_scope,
                hydration: None,
            });
        };
        if max_pages.is_some_and(|limit| budget.pages >= limit) {
            return Ok(RegistryLoad {
                threads: threads.into_values().collect(),
                error: None,
                registry_complete: false,
                source_scope,
                hydration: Some(RegistryHydration {
                    cursor: next,
                    budget,
                    use_state_db_only,
                    optimized_query,
                    source_scope,
                    tombstones: BTreeSet::new(),
                    pages_since_publish: 0,
                }),
            });
        }
        cursor = Some(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_explicitly_requests_all_known_sources_and_degrades_honestly() {
        let params = registry_query_params(
            None, true, true, RegistrySourceScope::RequestedKnownKinds,
        );
        assert!(params["sourceKinds"].as_array().unwrap().iter().any(|s| s == "exec"));
        assert!(params["sourceKinds"].as_array().unwrap().iter().any(|s| s == "subAgentReview"));
        assert_eq!(params["modelProviders"], json!([]));
        let fallback = registry_query_params(
            None, false, false, RegistrySourceScope::InteractiveFallback,
        );
        assert!(fallback.get("sourceKinds").is_none());
        assert!(fallback.get("modelProviders").is_none());

        let error = anyhow::Error::new(RpcResponseError {
            method: "thread/list".into(),
            code: Some(-32602),
            message: "unknown variant subAgentReview".into(),
        });
        assert!(is_registry_source_filter_unsupported(&error));
        let wrong_method = anyhow::Error::new(RpcResponseError {
            method: "thread/read".into(),
            code: Some(-32602),
            message: "unknown variant subAgentReview".into(),
        });
        assert!(!is_registry_source_filter_unsupported(&wrong_method));

        let mut status = BackendStatus::fake();
        apply_registry_source_coverage(&mut status, RegistrySourceScope::InteractiveFallback);
        assert!(!status.registry_complete);
        assert!(status.error.as_deref().unwrap().contains("only interactive"));
        assert!(status.optional_capabilities_missing.contains(&"thread/list:known-source-kinds".into()));
    }
}
