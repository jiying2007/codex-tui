//! Scope-aware operator preferences; old raw ThreadIds stay quarantined.
use crate::{app_server_target::ResolvedAppServerTarget, domain::ThreadUiState};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopedOperatorState {
    #[serde(default)]
    pub thread_ui: BTreeMap<String, ThreadUiState>,
    #[serde(default)]
    pub pins: BTreeSet<String>,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub marked_unread: BTreeSet<String>,
    #[serde(default)]
    pub acknowledged_attention: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hot_slots: BTreeMap<u8, String>,
}

impl ScopedOperatorState {
    pub fn legacy(local: &super::LocalStateV1) -> Self {
        Self {
            thread_ui: local.thread_ui.clone(),
            pins: local.pins.clone(),
            aliases: local.aliases.clone(),
            marked_unread: local.marked_unread.clone(),
            acknowledged_attention: local.acknowledged_attention.clone(),
            hot_slots: BTreeMap::new(),
        }
    }
}

/// Persist no bearer token, URL query or fragment in a Target's local namespace.
pub fn target_instance_identity(target: &ResolvedAppServerTarget) -> String {
    let endpoint = target.diagnostic_endpoint();
    format!("{}:{}:{}", target.name.len(), target.name, endpoint)
}

/// Length-prefixed composite key, distinguishing missing from empty Codex home.
/// Identity is scoped operator preference metadata, not remote-host attestation.
pub fn operator_scope_key(target: &str, home: Option<&str>) -> String {
    let home_text = home.unwrap_or("");
    format!(
        "scope-v1:{}:{}:{}:{}:{}",
        target.len(),
        target,
        u8::from(home.is_some()),
        home_text.len(),
        home_text,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_server_target::ResolvedAppServerEndpoint;

    #[test]
    fn different_target_and_codex_homes_never_overlap() {
        let a = ResolvedAppServerTarget::implicit_local(None);
        let b = ResolvedAppServerTarget {
            name: "remote".into(),
            endpoint: ResolvedAppServerEndpoint::WebSocket {
                url: "wss://example.test/rpc?private=secret".into(),
                auth_token: Some("never-serialize-token".into()),
            },
        };
        let ak = target_instance_identity(&a);
        let bk = target_instance_identity(&b);
        assert!(!bk.contains("secret"));
        assert!(!bk.contains("never-serialize"));
        assert_ne!(
            operator_scope_key(&ak, Some("/same/.codex")),
            operator_scope_key(&bk, Some("/same/.codex"))
        );
        assert_ne!(
            operator_scope_key(&ak, Some("/one")),
            operator_scope_key(&ak, Some("/two"))
        );
        assert_ne!(
            operator_scope_key(&ak, None),
            operator_scope_key(&ak, Some(""))
        );
    }

    #[test]
    fn legacy_operator_state_deserializes_without_implicit_target_binding() {
        let old = serde_json::json!({
            "schemaVersion": 1,
            "pins": ["thread-a"],
            "aliases": {"thread-a": "do not leak"}
        });
        let decoded: super::super::LocalStateV1 = serde_json::from_value(old).unwrap();
        assert!(decoded.scopes.is_empty());
        assert!(decoded.pins.contains("thread-a"));
    }
}
