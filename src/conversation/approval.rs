//! Opaque original wire context prevents approvals following same-ID permission changes.
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, PartialEq, Eq)]
pub struct ApprovalContext(Value);

impl From<Value> for ApprovalContext {
    fn from(value: Value) -> Self {
        Self(value)
    }
}

impl ApprovalContext {
    pub fn visible_scope(&self) -> Vec<(String, String)> {
        let Some(root) = self.0.as_object() else {
            return Vec::new();
        };
        let mut out = BTreeSet::new();
        if let Some(value) = root.get("grantRoot").and_then(Value::as_str) {
            out.insert(("grant-root".into(), value.into()));
        }
        for field in ["permissions", "additionalPermissions"] {
            let Some(scopes) = root.get(field).and_then(Value::as_object) else {
                continue;
            };
            if let Some(network) = scopes.get("network").filter(|value| !value.is_null()) {
                let mut detailed = false;
                for key in ["host", "hosts"] {
                    detailed |= collect_strings(&mut out, "network-host", network.get(key));
                }
                if !detailed {
                    out.insert(("network".into(), "requested".into()));
                }
            }
            if let Some(filesystem) = scopes.get("fileSystem").filter(|value| !value.is_null()) {
                let mut detailed = false;
                if let Some(object) = filesystem.as_object() {
                    for key in ["read", "write"] {
                        detailed |= collect_strings(&mut out, &format!("filesystem-{key}"), object.get(key));
                    }
                }
                if !detailed {
                    out.insert(("filesystem".into(), "requested".into()));
                }
            }
            for key in scopes.keys().filter(|key| !matches!(key.as_str(), "network" | "fileSystem")) {
                out.insert(("permission-category".into(), key.clone()));
            }
        }
        out.into_iter().collect()
    }
}

fn collect_strings(out: &mut BTreeSet<(String, String)>, label: &str, value: Option<&Value>) -> bool {
    let Some(value) = value else { return false };
    let values = value.as_array().map_or_else(|| vec![value], |items| items.iter().collect());
    let mut found = false;
    for value in values {
        if let Some(value) = value.as_str() {
            out.insert((label.into(), value.into()));
            found = true;
        }
    }
    found
}

impl std::fmt::Debug for ApprovalContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApprovalContext([redacted])")
    }
}
