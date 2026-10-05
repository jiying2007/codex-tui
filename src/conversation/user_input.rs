use super::{InteractiveRequest, InteractiveRequestKind};
use anyhow::Result;
use std::collections::BTreeSet;

impl InteractiveRequest {
    /// Ambiguous question IDs cannot be represented by the wire answer map.
    /// Optional fields and unknown extensions keep their existing compatibility.
    pub(crate) fn validate_user_input(&self) -> Result<()> {
        let InteractiveRequestKind::UserInput { questions } = &self.kind else {
            return Ok(());
        };
        anyhow::ensure!(
            !self.thread_id.0.trim().is_empty()
                && !self.turn_id.trim().is_empty()
                && !self.item_id.trim().is_empty(),
            "user input request has an empty target identity"
        );
        anyhow::ensure!(!questions.is_empty(), "user input request has no questions");
        let mut ids = BTreeSet::new();
        for question in questions {
            anyhow::ensure!(
                !question.id.trim().is_empty(),
                "user input question has an empty id"
            );
            anyhow::ensure!(
                ids.insert(&question.id),
                "user input request has duplicate question ids"
            );
        }
        Ok(())
    }
}
