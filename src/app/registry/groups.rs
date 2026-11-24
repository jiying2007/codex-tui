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
        let groups = self.registry_group_projection();
        let matched: Vec<_> = self
            .threads
            .iter()
            .map(|thread| self.thread_visible_in_registry_with_query(thread, &query))
            .collect();
        let matched_count = matched.iter().filter(|&&value| value).count();
        let mut recency: Vec<_> = self
            .threads
            .iter()
            .map(|thread| thread.metadata.updated_at)
            .collect();

        if show_all_matches {
            visible.extend((0..matched.len()).filter(|&index| matched[index]));
        } else {
            // An out-of-scope ancestor cannot represent a matching child.
            // Resolve roots iteratively with path compression so deep chains
            // remain linear, even when the newest member precedes its owner.
            let roots = scoped_group_roots(&groups, &matched);
            for (index, root) in roots.iter().enumerate() {
                if let Some(root) = *root {
                    recency[root] = recency[root].max(self.threads[index].metadata.updated_at);
                }
            }
            let mut represented = vec![false; self.threads.len()];
            let mut displayed_positions = 0_usize;
            for (index, root) in roots.into_iter().enumerate() {
                let Some(root) = root else {
                    continue;
                };
                if !represented[root] {
                    represented[root] = true;
                    let owner = &self.threads[root];
                    if displayed_positions < REGISTRY_RECENT_LIMIT
                        || owner.pinned
                        || self.thread_needs_attention(root)
                        || groups[root].2 > 0
                        || root == self.selected
                    {
                        visible.push(root);
                    }
                    displayed_positions += 1;
                }
                // Exceptional child rows retain their own identity and action
                // path, even when the representative is outside the window.
                let thread = &self.threads[index];
                if index != root
                    && (thread.pinned
                        || self.thread_needs_attention(index)
                        || index == self.selected)
                {
                    visible.push(index);
                    displayed_positions += 1;
                }
            }
        }

        // Bound the source window before sorting; preserve the selected row.
        visible.sort_by_cached_key(|&index| {
            let thread = &self.threads[index];
            (
                std::cmp::Reverse(self.thread_needs_attention(index) || groups[index].2 > 0),
                std::cmp::Reverse(thread.pinned),
                std::cmp::Reverse(recency[index]),
                &thread.id.0,
            )
        });
        (visible, matched_count)
    }
}

// Input edges have already been checked for cycles by registry_group_projection.
fn scoped_group_roots(
    groups: &[(Option<usize>, usize, usize)],
    matched: &[bool],
) -> Vec<Option<usize>> {
    let mut roots = vec![None; groups.len()];
    let mut path = Vec::new();
    for index in 0..groups.len() {
        if !matched[index] || roots[index].is_some() {
            continue;
        }
        let mut current = index;
        let root = loop {
            if let Some(root) = roots[current] {
                break root;
            }
            path.push(current);
            match groups[current].0 {
                Some(parent) if matched[parent] => current = parent,
                _ => break current,
            }
        };
        for member in path.drain(..) {
            roots[member] = Some(root);
        }
    }
    roots
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
    fn filtered_owner_cannot_hide_matching_child_or_its_attention() {
        let mut app = sample();
        let cwd = std::env::current_dir().unwrap().display().to_string();
        app.threads[0].metadata.cwd = "relative-parent".into();
        app.threads[1].metadata.cwd = cwd.clone();
        app.threads[2].metadata.cwd = cwd;
        app.threads[2].metadata.lineage.parent_thread_id = Some("thread-1".into());
        app.threads[3].metadata.cwd = "relative-orphan".into();
        app.selected = 0;
        for (local, repo) in [(true, false), (false, true)] {
            app.host_local_only = local;
            app.repo_backed_only = repo;
            assert_eq!(app.visible_indices_with_match_count(), (vec![1], 2));
            app.threads[2]
                .attention
                .push(AttentionReason::ApprovalRequired);
            let visible = app.visible_indices();
            assert!(visible.contains(&1));
            assert!(visible.contains(&2));
            assert!(!visible.contains(&0));
            app.threads[2].attention.clear();
        }
    }

    #[test]
    fn recent_descendant_promotes_older_group_root_without_growing_window() {
        let mut threads = FakeBackend::scaled(64).snapshot().threads;
        for (index, thread) in threads.iter_mut().enumerate() {
            thread.runtime = RuntimeStatus::Ready;
            thread.attention.clear();
            thread.pinned = false;
            thread.metadata.updated_at = 100 - index as i64;
        }
        threads[0].metadata.lineage.parent_thread_id = Some("thread-scale-00030".into());
        threads[30].metadata.lineage.parent_thread_id = Some("thread-scale-00040".into());
        let mut app = AppState::new(threads);
        app.selected = 1;
        let (visible, matched) = app.visible_indices_with_match_count();
        assert_eq!(matched, 64);
        assert_eq!(visible.len(), REGISTRY_RECENT_LIMIT);
        assert_eq!(visible[0], 40);
        assert!(!visible.contains(&0));
        assert!(!visible.contains(&30));
        assert!(!visible.contains(&20));
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
