#[test]
fn large_stale_projection_is_rejected_and_rescheduled_on_worker() {
    let mut live = app(12_000);
    let id = live.threads[0].id.0.clone();
    let old_output = Input::capture(&live, 100).compute();
    let mut changed = live.threads.clone();
    changed[0].title = "latest authority".into();
    reduce(&mut live, Action::ReplaceThreads(changed));

    let (send, input) = mpsc::sync_channel(1);
    let (output, receive) = mpsc::sync_channel(1);
    output.send(old_output).expect("stale result queued");
    let mut worker = PlanningWorker {
        send,
        receive,
        in_flight: true,
        completed: None,
        clock: None,
    };
    assert!(!worker.advance(&mut live, false, 101).expect("reject stale"));
    assert!(worker.in_flight, "fresh worker input enqueued");
    assert!(live.work_cards.is_empty(), "stale cards never installed");
    let fresh = input.try_recv().expect("new current snapshot");
    assert_eq!(fresh.threads[0].title, "latest authority");

    output.send(fresh.compute()).expect("worker result");
    assert!(worker.advance(&mut live, false, 101).expect("accept fresh"));
    assert_eq!(live.work_cards[live.work_card_by_thread[&id]].title, "latest authority");
}
