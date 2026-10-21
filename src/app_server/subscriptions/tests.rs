use super::*;

#[test]
fn loaded_and_started_notifications_never_confirm_this_connection() {
    let mut subscriptions = Subscriptions::default();
    for message in [
        json!({"method":"thread/status/changed","params":{"threadId":"one","status":{"type":"idle"}}}),
        json!({"method":"thread/started","params":{"thread":{"id":"one"}}}),
    ] {
        subscriptions.observe_wire(&message);
        assert!(!subscriptions.confirmed("one"));
    }
    subscriptions.confirm("one").unwrap();
    assert!(subscriptions.confirmed("one"));
    assert!(
        !Subscriptions::default().confirmed("one"),
        "a new connection cannot inherit an acknowledgement"
    );
}

#[test]
fn only_current_history_queue_and_pending_request_owners_retain_subscriptions() {
    let history = WatchSet::default();
    let queue = WatchSet::default();
    history.admit("one", || Ok(())).unwrap();
    queue.admit("one", || Ok(())).unwrap();
    let mut pending = BTreeMap::from([(
        RpcRequestId::Integer(7),
        PendingServerRequest {
            method: "item/commandExecution/requestApproval".into(),
            params: json!({"threadId":"one"}),
        },
    )]);
    let mut subscriptions = Subscriptions::default();
    subscriptions.confirm("one").unwrap();
    history.remove("one");
    assert!(
        subscriptions
            .next_action(&desired(&history, &queue, &pending))
            .is_none()
    );
    queue.remove("one");
    assert!(
        subscriptions
            .next_action(&desired(&history, &queue, &pending))
            .is_none()
    );
    pending.clear();
    assert_eq!(
        subscriptions.next_action(&desired(&history, &queue, &pending)),
        Some((ThreadId::new("one"), false))
    );
}

#[test]
fn rejected_acquisition_is_cleaned_up_once_without_automatic_resume_retry() {
    let mut subscriptions = Subscriptions::default();
    subscriptions.entries.insert(
        "one".into(),
        Entry {
            confirmed: false,
            cleanup: true,
            blocked: true,
        },
    );
    let owner = BTreeSet::from(["one".into()]);
    assert_eq!(
        subscriptions.next_action(&owner),
        Some((ThreadId::new("one"), false))
    );
    subscriptions.entries.get_mut("one").unwrap().cleanup = false;
    assert!(subscriptions.next_action(&owner).is_none());
    assert!(subscriptions.next_action(&BTreeSet::new()).is_none());
    assert!(subscriptions.entries.is_empty());
    assert_eq!(
        subscriptions.next_action(&owner),
        Some((ThreadId::new("one"), true))
    );
}

#[test]
fn retained_connection_state_is_bounded_and_releases_before_acquiring_more() {
    let mut subscriptions = Subscriptions::default();
    for i in 0..MAX_CONNECTION_SUBSCRIPTIONS {
        subscriptions.confirm(&format!("thread-{i}")).unwrap();
    }
    assert!(subscriptions.confirm("overflow").is_err());
    let owners = subscriptions
        .entries
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut more = owners.clone();
    more.insert("overflow".into());
    assert!(subscriptions.next_action(&more).is_none());
    assert!(matches!(
        subscriptions.next_action(&BTreeSet::from(["overflow".into()])),
        Some((_, false))
    ));
}
