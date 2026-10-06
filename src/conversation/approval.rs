//! Opaque original wire context prevents approvals following same-ID permission changes.
use serde_json::Value;

#[derive(Clone, PartialEq, Eq)]
pub struct ApprovalContext(Value);

impl From<Value> for ApprovalContext {
    fn from(value: Value) -> Self {
        Self(value)
    }
}

impl ApprovalContext {
    pub(crate) fn wire_context(&self) -> &Value {
        &self.0
    }
}

impl std::fmt::Debug for ApprovalContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApprovalContext([redacted])")
    }
}
