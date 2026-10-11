//! Bounded upstream pagination; partial inventories never become mutation authority.
use super::*;

const MAX_PAGES: usize = 1024;
const MAX_ROWS: usize = 200_000;
const MAX_CURSOR_BYTES: usize = 8192;
const MAX_IDENTITY_BYTES: usize = 32 * 1024 * 1024;
const NO_PROGRESS_PAGES: usize = 3;
const TOTAL_DEADLINE: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub(super) struct PageBudget {
    started: Option<std::time::Instant>,
    deadline: Duration,
    pages: usize,
    identity_bytes: usize,
    stalled: usize,
    cursors: BTreeSet<String>,
    ids: BTreeSet<String>,
}

impl Default for PageBudget {
    fn default() -> Self {
        Self::with_deadline(TOTAL_DEADLINE)
    }
}

impl PageBudget {
    pub(super) fn with_deadline(deadline: Duration) -> Self {
        Self {
            started: None,
            deadline,
            pages: 0,
            identity_bytes: 0,
            stalled: 0,
            cursors: BTreeSet::new(),
            ids: BTreeSet::new(),
        }
    }
    pub(super) fn remaining(&mut self) -> Result<Duration> {
        anyhow::ensure!(
            self.pages < MAX_PAGES,
            "pagination exceeded {MAX_PAGES} page budget"
        );
        let started = *self.started.get_or_insert_with(std::time::Instant::now);
        self.deadline
            .checked_sub(started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .context("pagination total deadline exceeded")
    }
    fn charge_identity(&mut self, identity: &str) -> Result<()> {
        // Account for retained keys plus a per-node allowance; this is not RSS.
        let next = self
            .identity_bytes
            .checked_add(identity.len().saturating_add(128))
            .context("pagination identity accounting overflow")?;
        anyhow::ensure!(
            next <= MAX_IDENTITY_BYTES,
            "pagination retained identity byte budget exceeded"
        );
        self.identity_bytes = next;
        Ok(())
    }
    pub(super) fn observe<'a>(
        &mut self,
        next: Option<&str>,
        ids: impl Iterator<Item = &'a str>,
    ) -> Result<()> {
        self.pages += 1;
        let before = self.ids.len();
        for id in ids {
            anyhow::ensure!(
                id.len() <= MAX_CURSOR_BYTES,
                "pagination row identity exceeds byte budget"
            );
            if !self.ids.contains(id) {
                self.charge_identity(id)?;
                self.ids.insert(id.to_owned());
            }
            anyhow::ensure!(self.ids.len() <= MAX_ROWS, "pagination row budget exceeded");
        }
        self.stalled = if self.ids.len() == before {
            self.stalled + 1
        } else {
            0
        };
        if let Some(next) = next {
            anyhow::ensure!(
                !next.is_empty() && next.len() <= MAX_CURSOR_BYTES,
                "pagination cursor has invalid size"
            );
            anyhow::ensure!(!self.cursors.contains(next), "pagination repeated cursor");
            self.charge_identity(next)?;
            self.cursors.insert(next.to_owned());
            anyhow::ensure!(
                self.stalled < NO_PROGRESS_PAGES,
                "pagination made no row progress"
            );
            anyhow::ensure!(
                self.pages < MAX_PAGES,
                "pagination exceeded {MAX_PAGES} page budget"
            );
        }
        Ok(())
    }
}

pub(super) async fn guarded_registry_page(
    rpc: &mut RpcSession,
    cursor: Option<String>,
    use_state_db_only: bool,
    optimized_query: bool,
    budget: &mut PageBudget,
) -> Result<RegistryPage> {
    let page = tokio::time::timeout(
        budget.remaining()?,
        load_registry_page(rpc, cursor, use_state_db_only, optimized_query),
    )
    .await
    .context("Registry pagination total deadline exceeded")??;
    anyhow::ensure!(
        page.threads.len() <= PAGE_SIZE as usize,
        "Registry page exceeds requested row limit"
    );
    budget.observe(
        page.next_cursor.as_deref(),
        page.threads.iter().map(|thread| thread.id.0.as_str()),
    )?;
    Ok(page)
}

pub(super) async fn load_registry_with_page_limit(
    rpc: &mut RpcSession,
    use_state_db_only: bool,
    max_pages: Option<usize>,
) -> Result<RegistryLoad> {
    let mut cursor = None;
    let mut threads = BTreeMap::new();
    let mut optimized_query = true;
    let mut budget = PageBudget::default();
    loop {
        let page = match guarded_registry_page(
            rpc,
            cursor,
            use_state_db_only,
            optimized_query,
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
                    hydration: None,
                });
            }
            Err(error) => return Err(error),
        };
        optimized_query = page.optimized_query;
        for thread in page.threads {
            threads.insert(thread.id.0.clone(), thread);
        }
        let Some(next) = page.next_cursor else {
            return Ok(RegistryLoad {
                threads: threads.into_values().collect(),
                error: None,
                registry_complete: true,
                hydration: None,
            });
        };
        if max_pages.is_some_and(|limit| budget.pages >= limit) {
            return Ok(RegistryLoad {
                threads: threads.into_values().collect(),
                error: None,
                registry_complete: false,
                hydration: Some(RegistryHydration {
                    cursor: next,
                    budget,
                    use_state_db_only,
                    optimized_query,
                    tombstones: BTreeSet::new(),
                    pages_since_publish: 0,
                }),
            });
        }
        cursor = Some(next);
    }
}

pub(super) async fn guarded_loaded_page(
    rpc: &mut RpcSession,
    cursor: Option<String>,
    budget: &mut PageBudget,
) -> Result<(Vec<String>, Option<String>)> {
    let result = tokio::time::timeout(
        budget.remaining()?,
        rpc.request(
            "thread/loaded/list",
            json!({"cursor": cursor, "limit": PAGE_SIZE}),
        ),
    )
    .await
    .context("Loaded inventory total deadline exceeded")??;
    let page = parse_loaded_list(result)?;
    anyhow::ensure!(
        page.data.len() <= PAGE_SIZE as usize,
        "Loaded page exceeds requested row limit"
    );
    budget.observe(
        page.next_cursor.as_deref(),
        page.data.iter().map(String::as_str),
    )?;
    Ok((page.data, page.next_cursor))
}

pub(super) async fn load_all_loaded_ids(rpc: &mut RpcSession) -> Result<BTreeSet<String>> {
    let mut cursor = None;
    let mut loaded = BTreeSet::new();
    let mut budget = PageBudget::default();
    loop {
        let (ids, next) = guarded_loaded_page(rpc, cursor, &mut budget).await?;
        loaded.extend(ids);
        cursor = next;
        if cursor.is_none() {
            return Ok(loaded);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_fifty_thousand_row_inventory_remains_allowed() {
        let mut budget = PageBudget::default();
        for page in 0..250 {
            budget.remaining().unwrap();
            let rows: Vec<_> = (page * 200..(page + 1) * 200)
                .map(|id| format!("thread-{id}"))
                .collect();
            let next = (page < 249).then(|| format!("cursor-{page}"));
            budget
                .observe(next.as_deref(), rows.iter().map(String::as_str))
                .unwrap();
        }
        assert_eq!(budget.ids.len(), 50_000);
    }
    #[test]
    fn retained_identity_budget_is_bounded_and_duplicate_rows_are_not_recharged() {
        let mut budget = PageBudget::default();
        budget.observe(None, ["one"].into_iter()).unwrap();
        let bytes = budget.identity_bytes;
        budget.observe(None, ["one"].into_iter()).unwrap();
        assert_eq!(budget.identity_bytes, bytes);
        budget.identity_bytes = MAX_IDENTITY_BYTES;
        assert!(
            budget
                .observe(None, ["two"].into_iter())
                .unwrap_err()
                .to_string()
                .contains("byte budget")
        );
    }
    #[test]
    fn cycles_stalls_and_total_deadline_are_independent_limits() {
        let mut budget = PageBudget::default();
        budget.observe(Some("a"), ["one"].into_iter()).unwrap();
        budget.observe(Some("b"), ["two"].into_iter()).unwrap();
        assert!(
            budget
                .observe(Some("a"), ["three"].into_iter())
                .unwrap_err()
                .to_string()
                .contains("repeated cursor")
        );
        let mut budget = PageBudget::default();
        for cursor in ["a", "b"] {
            budget.observe(Some(cursor), std::iter::empty()).unwrap();
        }
        assert!(
            budget
                .observe(Some("c"), std::iter::empty())
                .unwrap_err()
                .to_string()
                .contains("no row progress")
        );
        let mut budget = PageBudget::with_deadline(Duration::ZERO);
        assert!(budget.remaining().is_err());
        let mut budget = PageBudget::default();
        budget.pages = MAX_PAGES;
        assert!(budget.remaining().is_err());
    }
}
