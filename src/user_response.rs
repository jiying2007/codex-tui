//! Local response bookkeeping. A completed transport write is not server acknowledgement.
use crate::conversation::{InteractiveRequest, InteractiveResolution};
use std::collections::BTreeMap;
use std::fmt;
#[derive(Clone, PartialEq, Eq)]
pub struct UserResponseSubmission {
    pub ticket: u64,
    pub request: InteractiveRequest,
    pub resolution: InteractiveResolution,
}
impl fmt::Debug for UserResponseSubmission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserResponseSubmission")
            .field("ticket", &self.ticket)
            .field("request_id", &self.request.request_id)
            .field("resolution", &"[redacted]")
            .finish()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserResponseOutcome {
    /// No response bytes were submitted to the transport.
    NotSent(String),
    /// The local transport completed the write; no server acknowledgement is implied.
    Written,
    /// A write was attempted, or the actor stopped before a conclusive receipt.
    Unknown(String),
}
pub type UserResponseAnswers = BTreeMap<String, Vec<String>>;
