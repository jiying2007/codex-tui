//! Verified active-Target parent projection; no synthetic Agent identity or RPC routing.
use super::*;
use std::collections::VecDeque;

impl AppState {
    /// (trusted direct parent, descendant count, actionable descendant count).
    /// All indices refer only to the current active Target's authoritative list.
    pub fn registry_group_projection(&self) -> Vec<(Option<usize>, usize, usize)> {
        let size = self.threads.len();
        let mut parents = vec![None; size];
        for (index, thread) in self.threads.iter().enumerate() {
            let lineage = &thread.metadata.lineage;
            let parent_id = match (
                lineage.parent_thread_id.as_deref(),
                lineage.attached_to_thread_id.as_deref(),
            ) {
                (Some(parent), Some(attached)) if parent != attached => continue,
                (Some(parent), _) | (None, Some(parent)) => parent,
                (None, None) => continue,
            };
            let Some(&parent_index) = self.thread_index_by_id.get(parent_id) else {
                continue;
            };
            if index == parent_index || self.threads[parent_index].id.0 != parent_id {
                continue;
            }
            let owner = &self.threads[parent_index].metadata.lineage;
            if lineage
                .session_id
                .as_deref()
                .zip(owner.session_id.as_deref())
                .is_some_and(|(left, right)| left != right)
            {
                continue;
            }
            parents[index] = Some(parent_index);
        }

        // Iterative DFS: discard cycles and every descendant leading into a cycle.
        // Marking finished nodes keeps even very deep subagent trees O(N).
        let mut validity = vec![0_u8; size];
        for index in 0..size {
            if validity[index] != 0 {
                continue;
            }
            let mut stack = Vec::new();
            let mut current = index;
            loop {
                if validity[current] != 0 {
                    let result = if validity[current] == 2 { 2 } else { 3 };
                    for entry in stack {
                        validity[entry] = result;
                    }
                    break;
                }
                validity[current] = 1;
                stack.push(current);
                if let Some(parent_index) = parents[current] {
                    current = parent_index;
                } else {
                    for entry in stack {
                        validity[entry] = 2;
                    }
                    break;
                }
            }
        }
        for index in 0..size {
            if validity[index] != 2 {
                parents[index] = None;
            }
        }

        // Bottom-up propagation avoids per-row N×N parent scans.
        let mut pending_children = vec![0_usize; size];
        let mut descendants = vec![0_usize; size];
        let mut actionable = vec![0_usize; size];
        for parent_index in parents.iter().flatten() {
            pending_children[*parent_index] += 1;
        }
        let mut leaves = VecDeque::new();
        for (index, remaining) in pending_children.iter().enumerate() {
            if *remaining == 0 {
                leaves.push_back(index);
            }
        }
        while let Some(index) = leaves.pop_front() {
            if let Some(parent_index) = parents[index] {
                descendants[parent_index] += descendants[index] + 1;
                actionable[parent_index] +=
                    actionable[index] + usize::from(self.thread_needs_attention(index));
                pending_children[parent_index] -= 1;
                if pending_children[parent_index] == 0 {
                    leaves.push_back(parent_index);
                }
            }
        }
        parents
            .into_iter()
            .enumerate()
            .map(|(index, parent)| (parent, descendants[index], actionable[index]))
            .collect()
    }

    pub fn visible_indices_with_match_count(&self) -> (Vec<usize>, usize) {
        let query = self.filter.trim().to_lowercase();
        let show_all_matches = self.show_all_history || !query.is_empty();
        let mut visible = Vec::with_capacity(if show_all_matches {
            self.threads.len()
        } else {
            REGISTRY_RECENT_LIMIT.min(self.threads.len())
        });
        let mut matched_count = 0_usize;
        let mut displayed_positions = 0_usize;
        let groups = self.registry_group_projection();

        for (index, thread) in self.threads.iter().enumerate() {
            if !self.thread_visible_in_registry_with_query(thread, &query) {
                continue;
            }
            matched_count = matched_count.saturating_add(1);
            let actionable = self.thread_needs_attention(index);
            // A child is folded only when its owner is present in this one Target.
            // Pinned, selected and Needs-You children remain independently actionable.
            if !show_all_matches
                && groups[index].0.is_some()
                && !actionable
                && !thread.pinned
                && index != self.selected
            {
                continue;
            }
            let recent_position = displayed_positions;
            displayed_positions = displayed_positions.saturating_add(1);
            if show_all_matches
                || recent_position < REGISTRY_RECENT_LIMIT
                || thread.pinned
                || actionable
                || groups[index].2 > 0
                || index == self.selected
            {
                visible.push(index);
            }
        }

        // Bound the source window before sorting; preserve the selected row.
        visible.sort_by_cached_key(|&index| {
            let thread = &self.threads[index];
            (
                std::cmp::Reverse(self.thread_needs_attention(index) || groups[index].2 > 0),
                std::cmp::Reverse(thread.pinned),
                std::cmp::Reverse(thread.metadata.updated_at),
                &thread.id.0,
            )
        });
        (visible, matched_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};

    fn sample() -> AppState {
        let mut threads = FakeBackend::scaled(4).snapshot().threads;
        for (index, thread) in threads.iter_mut().enumerate() {
            thread.id = ThreadId::new(format!("thread-{index}"));
            thread.metadata.lineage.session_id = Some("session-A".into());
            thread.attention.clear();
            thread.runtime = RuntimeStatus::Ready;
            thread.pinned = false;
        }
        threads[1].metadata.lineage.parent_thread_id = Some("thread-0".into());
        // Fork shares session ID but is independent without a parent edge.
        threads[2].metadata.lineage.forked_from_id = Some("thread-0".into());
        threads[3].metadata.lineage.parent_thread_id = Some("missing".into());
        AppState::new(threads)
    }

    #[test]
    fn fold_only_real_child_while_fork_and_orphan_remain_visible() {
        let app = sample();
        let info = app.registry_group_projection();
        assert_eq!(info[1].0, Some(0));
        assert_eq!(info[0].1, 1);
        assert_eq!(info[2].0, None);
        assert_eq!(info[3].0, None);
        assert!(!app.visible_indices().contains(&1));
        assert!(app.visible_indices().contains(&2));
        assert!(app.visible_indices().contains(&3));
    }

    #[test]
    fn actionable_child_is_visible_and_parent_reports_it() {
        let mut app = sample();
        app.threads[1]
            .attention
            .push(AttentionReason::ApprovalRequired);
        let info = app.registry_group_projection();
        assert_eq!((info[0].1, info[0].2), (1, 1));
        assert!(app.visible_indices().contains(&1));
        app.threads[1].attention.clear();
        app.selected = 1;
        assert!(app.visible_indices().contains(&1));
        app.selected = 0;
        app.filter = app.threads[1].title.clone();
        assert!(app.visible_indices().contains(&1));
    }

    #[test]
    fn reject_conflicting_session_cycles_and_attachment_conflicts() {
        let mut app = sample();
        app.threads[1].metadata.lineage.session_id = Some("session-B".into());
        assert_eq!(app.registry_group_projection()[1].0, None);
        app.threads[1].metadata.lineage.session_id = Some("session-A".into());
        app.threads[0].metadata.lineage.parent_thread_id = Some("thread-1".into());
        assert_eq!(app.registry_group_projection()[1].0, None);
        app.threads[0].metadata.lineage.parent_thread_id = None;
        app.threads[1].metadata.lineage.attached_to_thread_id = Some("thread-2".into());
        assert_eq!(app.registry_group_projection()[1].0, None);
    }

    #[test]
    fn ten_thousand_descendants_are_linear_and_do_not_recurse() {
        let mut threads = FakeBackend::scaled(10_000).snapshot().threads;
        for (index, thread) in threads.iter_mut().enumerate().skip(1) {
            thread.metadata.lineage.parent_thread_id =
                Some(format!("thread-scale-{:05}", index - 1));
        }
        let app = AppState::new(threads);
        let groups = app.registry_group_projection();
        assert_eq!(groups[0].1, 9_999);
        assert_eq!(groups[9_999].0, Some(9_998));
    }
}
