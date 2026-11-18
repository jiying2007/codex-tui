//! A write receipt must never be confused with a read-only Queue refresh.
use super::*;

pub(super) async fn dispatch(
    rpc: &mut RpcSession,
    status: &mut BackendStatus,
    tx: &mpsc::Sender<ConversationEvent>,
    mutation: ThreadQueueMutation,
) -> Option<ThreadId> {
    let capability = thread_queue_mutation_capability(&mutation);
    let thread_id = thread_queue_mutation_thread_id(&mutation).clone();
    match async {
        rpc.ensure_subscribed(&thread_id).await?;
        execute_thread_queue_mutation(rpc, &mutation).await
    }
    .await
    {
        Ok(()) => {
            mark_optional_capability(status, capability, true);
            // Read back using the actor's cooperative page scheduler, not this
            // mutation command. The draft is retained until that read finishes.
            Some(thread_id)
        }
        Err(error) => {
            if is_thread_queue_unsupported(&error) {
                mark_optional_capability(status, capability, false);
            }
            let explicitly_rejected = error.downcast_ref::<RpcResponseError>().is_some();
            send_conversation_event(
                tx,
                ConversationEvent::ThreadQueueFailed {
                    thread_id: thread_id.clone(),
                    error: error.to_string(),
                },
            )
            .await;
            if explicitly_rejected {
                send_conversation_event(
                    tx,
                    ConversationEvent::QueueMutationOutcome {
                        thread_id,
                        committed: false,
                    },
                )
                .await;
            }
            None
        }
    }
}
