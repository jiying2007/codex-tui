//! Deterministic completion scheduling through the production Forge actor.
use super::*;
use tokio::sync::oneshot;

type Request = (ThreadId, String, oneshot::Sender<&'static str>);
struct ControlledProvider(mpsc::UnboundedSender<Request>);
impl ForgeProvider for ControlledProvider {
    fn probe<'a>(&'a self, thread_id: ThreadId, cwd: String) -> ForgeFuture<'a, ForgeObservation> {
        Box::pin(async move {
            let (reply, receive) = oneshot::channel();
            self.0
                .send((thread_id.clone(), cwd.clone(), reply))
                .unwrap();
            let label = receive.await.unwrap();
            ForgeObservation::unavailable(thread_id, cwd, label)
        })
    }
    fn probe_review<'a>(&'a self, _: ForgeReviewTarget) -> ForgeFuture<'a, ForgeReviewSummary> {
        Box::pin(async { panic!("not a review fixture") })
    }
}
async fn started(requests: &mut mpsc::UnboundedReceiver<Request>) -> Request {
    tokio::time::timeout(Duration::from_secs(2), requests.recv())
        .await
        .unwrap()
        .unwrap()
}
async fn buffered(handle: &ForgeHandle) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while handle.event_rx.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
fn label(event: Option<ForgeEvent>) -> Option<String> {
    match event {
        Some(ForgeEvent::Observation(value)) => value.error,
        _ => None,
    }
}
#[tokio::test]
async fn late_old_completion_cannot_replace_the_newer_accepted_probe() {
    let (tx, mut requests) = mpsc::unbounded_channel();
    let mut handle = ForgeHandle::start_with_provider(Arc::new(ControlledProvider(tx)));
    handle
        .probe(ThreadId::new("leader-old"), "/repo".into())
        .unwrap();
    let old = started(&mut requests).await;
    handle
        .probe(ThreadId::new("leader-new"), "/repo".into())
        .unwrap();
    let fresh = started(&mut requests).await;
    fresh.2.send("fresh").unwrap();
    buffered(&handle).await;
    assert_eq!(label(handle.try_recv()).as_deref(), Some("fresh"));
    old.2.send("obsolete").unwrap();
    buffered(&handle).await;
    assert!(
        handle.try_recv().is_none(),
        "a slower old read must not overwrite the fresh result"
    );
}
#[tokio::test]
async fn an_already_buffered_result_is_rechecked_after_new_admission() {
    let (tx, mut requests) = mpsc::unbounded_channel();
    let mut handle = ForgeHandle::start_with_provider(Arc::new(ControlledProvider(tx)));
    handle
        .probe(ThreadId::new("thread"), "/repo".into())
        .unwrap();
    started(&mut requests).await.2.send("buffered-old").unwrap();
    buffered(&handle).await;
    handle
        .probe(ThreadId::new("thread"), "/repo".into())
        .unwrap();
    assert!(
        handle.try_recv().is_none(),
        "admission must invalidate old buffered events too"
    );
    started(&mut requests).await.2.send("fresh").unwrap();
    buffered(&handle).await;
    assert_eq!(label(handle.try_recv()).as_deref(), Some("fresh"));
}
