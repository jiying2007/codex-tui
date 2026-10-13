use super::*;

/// Centralized watch ownership applies even to reducer routes that return early.
/// History-cache eviction does not imply an active backend subscription.
pub(super) fn reconcile(state: &mut AppState, mut effects: Vec<Effect>) -> Vec<Effect> {
    // Provisional owners are only unresolved watches, never tombstones.
    state.unregistered_watches.retain(|id| {
        state.watched_conversations.contains(id) && !state.thread_index_by_id.contains_key(id)
    });
    for effect in &effects {
        match effect {
            Effect::LoadConversation(id)
            | Effect::LoadOlderConversation { thread_id: id, .. }
            | Effect::SubmitPrompt { thread_id: id, .. } => {
                track_watch(state, id);
            }
            Effect::JumpToTranscriptHit(hit) => {
                track_watch(state, &hit.thread_id);
            }
            // A navigation route can request Stop while an active background
            // turn still owns the same history watch. Resolve owners first.
            Effect::StopWatchingConversation(_) => {}
            _ => {}
        }
    }

    let mut owners = BTreeSet::new();
    if !state.should_quit {
        owners.extend(reachable_threads(state));
        owners.extend(
            state
                .prompt_submissions
                .pending_thread_ids()
                .filter(|id| {
                    state.thread_index_by_id.contains_key(*id)
                        || state.unregistered_watches.contains(*id)
                })
                .cloned(),
        );
        for id in &state.watched_conversations {
            // Only inspect already watched threads; no global 50k-thread scan.
            if state.thread_index_by_id.contains_key(id) || state.unregistered_watches.contains(id)
            {
                let active_runtime = state
                    .thread_index_by_id
                    .get(id)
                    .and_then(|index| state.threads.get(*index))
                    .is_some_and(|thread| {
                        matches!(
                            &thread.runtime,
                            RuntimeStatus::Working | RuntimeStatus::WaitingHuman
                        )
                    });
                let active_turn = state.active_history_watches.contains(id);
                if active_runtime || active_turn {
                    owners.insert(id.clone());
                }
            }
        }
    }
    // Legacy navigation stops are advisory: never cancel watches owned by
    // ongoing turns or pending prompts. Remove only real stopped subscriptions.
    effects.retain(
        |effect| !matches!(effect, Effect::StopWatchingConversation(id) if owners.contains(&id.0)),
    );
    for effect in &effects {
        if let Effect::StopWatchingConversation(id) = effect {
            state.watched_conversations.remove(&id.0);
        }
    }

    let stale = state
        .watched_conversations
        .difference(&owners)
        .cloned()
        .collect::<Vec<_>>();
    for id in stale {
        state.watched_conversations.remove(&id);
        effects.push(Effect::StopWatchingConversation(ThreadId::new(id)));
    }
    state
        .unregistered_watches
        .retain(|id| state.watched_conversations.contains(id));
    state
        .active_history_watches
        .retain(|id| state.watched_conversations.contains(id));
    effects
}

fn track_watch(state: &mut AppState, id: &ThreadId) {
    state.watched_conversations.insert(id.0.clone());
    if !state.thread_index_by_id.contains_key(&id.0) {
        state.unregistered_watches.insert(id.0.clone());
    }
}

// Follow only return routes reachable from the current surface. Stale saved
// return fields are not owners. This graph has a fixed number of view slots,
// and cycles are cut without scanning the registry or conversation cache.
fn reachable_threads(state: &AppState) -> BTreeSet<String> {
    let mut owners = BTreeSet::new();
    let mut pending = vec![&state.view];
    if state.input_mode == InputMode::Search {
        pending.extend(state.search_return_view.as_ref());
    }
    let mut seen = Vec::new();
    while let Some(view) = pending.pop() {
        if seen.contains(&view) {
            continue;
        }
        seen.push(view);
        if let Some(id) = AppState::thread_id_for_view(view)
            && (state.thread_index_by_id.contains_key(&id.0)
                || state.unregistered_watches.contains(&id.0))
        {
            owners.insert(id.0);
        }
        match view {
            View::Board | View::Scratch(_) => pending.extend(state.board_return_view.as_ref()),
            View::Review(_) => pending.extend(state.review_return_view.as_ref()),
            View::Workspace(_) => pending.extend(state.workspace_return_view.as_ref()),
            View::ManagedWorktrees(_) => pending.extend(state.managed_return_view.as_ref()),
            View::Registry | View::Thread(_) => {}
        }
    }
    if !owners.is_empty()
        && let Some(previous) = &state.previous_target
        && (state.thread_index_by_id.contains_key(&previous.0)
            || state.unregistered_watches.contains(&previous.0))
    {
        owners.insert(previous.0.clone());
    }
    owners
}

#[cfg(test)]
mod tests;
