//! Connection acknowledgements are separate from UI owners and server loaded state.
use super::*;
use tokio::time::Instant;

const MAX_CONNECTION_SUBSCRIPTIONS: usize = 1024;

#[derive(Default)]
struct Entry {
    confirmed: bool,
    cleanup: bool,
    blocked: bool,
}

struct Pending {
    request_id: u64,
    thread_id: String,
    subscribe: bool,
    deadline: Instant,
    acknowledgement_seen: bool,
    closed_after_acknowledgement: bool,
}

#[derive(Default)]
pub(super) struct Subscriptions {
    entries: BTreeMap<String, Entry>,
    pending: Option<Pending>,
}

pub(super) fn desired(
    history: &WatchSet,
    queue: &WatchSet,
    pending: &BTreeMap<RpcRequestId, PendingServerRequest>,
) -> BTreeSet<String> {
    let mut ids = history.snapshot();
    ids.extend(queue.snapshot());
    ids.extend(pending.values().filter_map(|request| {
        request
            .params
            .get("threadId")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }));
    ids
}

pub(super) async fn refresh_owned(
    rpc: &mut RpcSession,
    id: ThreadId,
    history: &WatchSet,
    queue: &WatchSet,
    tx: &mpsc::Sender<ConversationEvent>,
) {
    if history.contains(&id.0) {
        emit_conversation_load(rpc, id.clone(), tx).await;
    }
    if queue.contains(&id.0) {
        let event = match load_thread_queue(rpc, id.clone()).await {
            Ok(snapshot) => ConversationEvent::ThreadQueueLoaded(snapshot),
            Err(error) => ConversationEvent::ThreadQueueFailed {
                thread_id: id,
                error: error.to_string(),
            },
        };
        send_conversation_event(tx, event).await;
    }
}

pub(super) async fn retire_removed_owners(
    method: &str,
    params: &Value,
    pending: &mut BTreeMap<RpcRequestId, PendingServerRequest>,
    history: &WatchSet,
    queue: &WatchSet,
    tx: &mpsc::Sender<ConversationEvent>,
) {
    let mut retired = Vec::new();
    if matches!(method, "thread/archived" | "thread/deleted")
        && let Some(id) = params.get("threadId").and_then(Value::as_str)
    {
        // Authoritative removal invalidates the outstanding requests as well
        // as view watches. Otherwise a stale approval could resume the removed
        // thread before a later serverRequest/resolved reaches the actor.
        pending.retain(|request_id, request| {
            let keep = request.params.get("threadId").and_then(Value::as_str) != Some(id);
            if !keep {
                retired.push(request_id.clone());
            }
            keep
        });
    }
    watch_lifecycle::retire_removed_watches(method, params, history, queue, tx).await;
    for request_id in retired {
        send_conversation_event(tx, ConversationEvent::InteractiveResolved { request_id }).await;
    }
}

impl Subscriptions {
    pub(super) fn confirmed(&self, id: &str) -> bool {
        self.entries.get(id).is_some_and(|entry| entry.confirmed)
    }

    pub(super) fn capacity(&self) -> Result<()> {
        anyhow::ensure!(
            self.entries.len() < MAX_CONNECTION_SUBSCRIPTIONS,
            "App Server connection subscription limit reached"
        );
        Ok(())
    }

    pub(super) fn confirm(&mut self, id: &str) -> Result<()> {
        if !self.entries.contains_key(id) {
            self.capacity()?;
        }
        self.entries.insert(
            id.to_owned(),
            Entry {
                confirmed: true,
                ..Entry::default()
            },
        );
        Ok(())
    }

    pub(super) fn next_action(&mut self, desired: &BTreeSet<String>) -> Option<(ThreadId, bool)> {
        // Failed acquisitions are blocked until an explicit command retries, and
        // forgotten when their last owner goes away. There is no tombstone log.
        self.entries
            .retain(|id, entry| entry.confirmed || entry.cleanup || desired.contains(id));
        if self.pending.is_some() {
            return None;
        }
        if let Some((id, _)) = self
            .entries
            .iter()
            .find(|(id, entry)| entry.cleanup || (entry.confirmed && !desired.contains(*id)))
        {
            return Some((ThreadId::new(id.clone()), false));
        }
        desired
            .iter()
            .find(|id| {
                self.entries
                    .get(*id)
                    .is_none_or(|entry| !entry.confirmed && !entry.blocked)
                    && (self.entries.contains_key(*id) || self.capacity().is_ok())
            })
            .map(|id| (ThreadId::new(id.clone()), true))
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.pending.as_ref().map(|pending| pending.deadline)
    }

    pub(super) fn is_pending_response(&self, message: &Value) -> bool {
        message.get("method").is_none()
            && self.pending.as_ref().is_some_and(|pending| {
                message.get("id").and_then(Value::as_u64) == Some(pending.request_id)
            })
    }

    pub(super) fn observe_wire(&mut self, message: &Value) {
        if self.is_pending_response(message) {
            self.pending
                .as_mut()
                .expect("matched pending response")
                .acknowledgement_seen = true;
        }
        let method = message.get("method").and_then(Value::as_str);
        let closed = matches!(
            method,
            Some("thread/closed" | "thread/archived" | "thread/deleted")
        ) || (method == Some("thread/status/changed")
            && message
                .pointer("/params/status/type")
                .and_then(Value::as_str)
                == Some("notLoaded"));
        if closed && let Some(id) = message.pointer("/params/threadId").and_then(Value::as_str) {
            // Observe in wire order, before queuing notifications. An older
            // notLoaded event must not invalidate a later resume acknowledgement.
            if self
                .pending
                .as_ref()
                .is_none_or(|pending| pending.thread_id != id)
            {
                if let Some(entry) = self.entries.get_mut(id)
                    && entry.blocked
                {
                    // Closing proves release, but does not authorize retrying
                    // a rejected acquisition for an unchanged owner.
                    entry.confirmed = false;
                    entry.cleanup = false;
                } else {
                    self.entries.remove(id);
                }
            } else {
                let pending = self.pending.as_mut().expect("matched pending thread");
                pending.closed_after_acknowledgement |= pending.acknowledgement_seen;
                if let Some(entry) = self.entries.get_mut(id) {
                    entry.confirmed = false;
                }
            }
        }
    }
}

pub(super) fn should_queue_rpc_message(message: &Value) -> bool {
    if message.get("id").is_some() && message.get("method").is_some() {
        return true;
    }

    matches!(
        message.get("method").and_then(Value::as_str),
        Some(
            "serverRequest/resolved"
                | "thread/goal/updated"
                | "thread/goal/cleared"
                | "thread/queue/changed"
                | "thread/started"
                | "thread/closed"
                | "thread/status/changed"
                | "thread/archived"
                | "thread/deleted"
                | "thread/unarchived"
                | "thread/name/updated"
                | "thread/project/updated"
                | "turn/started"
                | "turn/completed"
                | "item/completed"
        )
    )
}

pub(super) fn response_result(message: &Value, method: &str) -> Result<Value> {
    if let Some(error) = message.get("error") {
        return Err(RpcResponseError {
            method: method.to_owned(),
            code: error.get("code").and_then(Value::as_i64),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| error.to_string()),
        }
        .into());
    }
    message
        .get("result")
        .cloned()
        .with_context(|| format!("{method} response missing result"))
}

impl RpcSession {
    pub(super) async fn ensure_subscribed(&mut self, id: &ThreadId) -> Result<()> {
        if self.subscriptions.confirmed(&id.0) {
            return Ok(());
        }
        anyhow::ensure!(
            self.subscriptions.pending.is_none(),
            "App Server subscription acknowledgement is pending"
        );
        if let Some(entry) = self.subscriptions.entries.get_mut(&id.0) {
            anyhow::ensure!(
                !entry.cleanup,
                "previous App Server subscription attempt awaits cleanup"
            );
            entry.blocked = false; // Only a new explicit accepted command retries a rejection.
        }
        self.change_subscription(id, true).await
    }

    pub(super) async fn change_subscription(
        &mut self,
        id: &ThreadId,
        subscribe: bool,
    ) -> Result<()> {
        anyhow::ensure!(
            self.subscriptions.pending.is_none(),
            "App Server subscription acknowledgement is pending"
        );
        if !self.subscriptions.entries.contains_key(&id.0) {
            self.subscriptions.capacity()?;
        }
        let entry = self.subscriptions.entries.entry(id.0.clone()).or_default();
        entry.cleanup = true;
        if !subscribe {
            // A release already on the wire may have detached this connection,
            // even while its acknowledgement is delayed by an approval.
            entry.confirmed = false;
        }
        let request_id = self.next_id;
        self.subscriptions.pending = Some(Pending {
            request_id,
            thread_id: id.0.clone(),
            subscribe,
            deadline: Instant::now() + RPC_REQUEST_TIMEOUT,
            acknowledgement_seen: false,
            closed_after_acknowledgement: false,
        });
        let method = if subscribe {
            "thread/resume"
        } else {
            "thread/unsubscribe"
        };
        let params = if subscribe {
            json!({"threadId": id.0, "excludeTurns": true})
        } else {
            json!({"threadId": id.0})
        };
        let result = self.request(method, params).await;
        if result.is_err() && self.interrupted_request == Some(request_id) && !self.is_quarantined()
        {
            // The actor must service the interactive request. Keep exactly this
            // request id until its late acknowledgement or original deadline;
            // do not resend a resume/unsubscribe whose outcome is unknown.
            return result.map(|_| ());
        }
        self.finish_subscription(result).map(|_| ())
    }

    fn finish_subscription(&mut self, result: Result<Value>) -> Result<Option<ThreadId>> {
        let pending = self
            .subscriptions
            .pending
            .take()
            .context("subscription request is not pending")?;
        let result = result.and_then(|value| {
            if pending.subscribe {
                anyhow::ensure!(
                    value.pointer("/thread/id").and_then(Value::as_str)
                        == Some(pending.thread_id.as_str()),
                    "thread/resume did not confirm the requested thread identity"
                );
            } else {
                anyhow::ensure!(
                    matches!(
                        value.get("status").and_then(Value::as_str),
                        Some("unsubscribed" | "notSubscribed" | "notLoaded")
                    ),
                    "thread/unsubscribe did not confirm release"
                );
            }
            Ok(())
        });
        if let Err(error) = result {
            let entry = self
                .subscriptions
                .entries
                .entry(pending.thread_id)
                .or_default();
            entry.confirmed = false;
            entry.cleanup = true;
            entry.blocked = true;
            // A resume can attach before returning an application error: retain
            // it for explicit unsubscribe cleanup. Unconfirmed release or a
            // malformed/timed-out exchange requires connection teardown instead
            // of an automatic retry that could race a later subscription.
            if !pending.subscribe || error.downcast_ref::<RpcResponseError>().is_none() {
                self_quarantine(self, &error);
            }
            return Err(error);
        }
        if pending.subscribe {
            if pending.closed_after_acknowledgement {
                // Another RPC may have queued this late acknowledgement and a
                // later close before the actor drains either. The old response
                // cannot restore confirmation or refresh a removed thread.
                self.subscriptions.entries.remove(&pending.thread_id);
                return Ok(None);
            }
            self.subscriptions.confirm(&pending.thread_id)?;
            Ok(Some(ThreadId::new(pending.thread_id)))
        } else {
            if self
                .subscriptions
                .entries
                .get(&pending.thread_id)
                .is_some_and(|entry| entry.blocked)
            {
                let entry = self
                    .subscriptions
                    .entries
                    .get_mut(&pending.thread_id)
                    .expect("checked entry");
                entry.confirmed = false;
                entry.cleanup = false;
            } else {
                self.subscriptions.entries.remove(&pending.thread_id);
            }
            Ok(None)
        }
    }

    pub(super) fn subscription_response(
        &mut self,
        message: &Value,
    ) -> Option<Result<Option<ThreadId>>> {
        if !self.subscriptions.is_pending_response(message) {
            return None;
        }
        let subscribe = self
            .subscriptions
            .pending
            .as_ref()
            .expect("matched pending response")
            .subscribe;
        let method = if subscribe {
            "thread/resume"
        } else {
            "thread/unsubscribe"
        };
        Some(self.finish_subscription(response_result(message, method)))
    }
}

#[cfg(test)]
mod fault_tests;
#[cfg(test)]
mod fixture;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod wire_order_tests;
#[cfg(test)]
mod wire_tests;
