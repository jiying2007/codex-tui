//! Keep provenance separate from location: session ID is not a grouping key.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadLineage {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub parent_thread_id: Option<String>,
    #[serde(default)]
    pub attached_to_thread_id: Option<String>,
    #[serde(default)]
    pub forked_from_id: Option<String>,
    #[serde(default)]
    pub originator: Option<String>,
    #[serde(default)]
    pub model_provider: Option<String>,
}
