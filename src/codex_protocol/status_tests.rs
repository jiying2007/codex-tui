//! Unsupported and malformed upstream statuses are not evidence of an idle thread.
use super::*;
use serde_json::json;

#[test]
fn only_explicit_not_loaded_is_inactive() {
    let value = json!({"type":"notLoaded"});
    assert_eq!(normalize_status(&value).0, RuntimeStatus::Inactive);
    for value in [
        json!({"type":"newAgentStatus"}),
        json!({"type":null}),
        json!({"unexpected":"shape"}),
        json!(null),
        json!("active"),
    ] {
        let (state, attention) = normalize_status(&value);
        assert_eq!(state, RuntimeStatus::Unknown, "{value}");
        assert!(attention.is_empty());
    }
}

#[test]
fn explicit_idle_and_live_flags_preserve_existing_behavior() {
    assert!(matches!(
        normalize_status(&json!({"type":"idle"})).0,
        RuntimeStatus::Ready
    ));
    let (state, attention) = normalize_status(&json!({
        "type": "active", "activeFlags": ["waitingOnApproval"]
    }));
    assert_eq!(state, RuntimeStatus::WaitingHuman);
    assert_eq!(attention, vec![AttentionReason::ApprovalRequired]);
}
