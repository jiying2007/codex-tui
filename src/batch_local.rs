use crate::planning::{SourceRef, WorkCardProjection};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_BATCH_TARGETS: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocalBatchAction {
    AddTag(String),
    RemoveTag(String),
    SetPriority(i32),
    ClearPriority,
    SetReady(bool),
    SetDone(bool),
    SnoozeUntil(Option<u64>),
}

impl LocalBatchAction {
    pub fn validate(&self, planned_at_unix_ms: u64) -> Result<()> {
        match self {
            Self::AddTag(tag) | Self::RemoveTag(tag) => {
                let tag = tag.trim();
                anyhow::ensure!(!tag.is_empty(), "batch tag must not be empty");
                anyhow::ensure!(
                    tag.chars().count() <= 64,
                    "batch tag must be at most 64 characters"
                );
            }
            Self::SnoozeUntil(Some(until)) => {
                anyhow::ensure!(
                    *until > planned_at_unix_ms,
                    "batch snooze deadline must be in the future"
                );
            }
            Self::SetPriority(_)
            | Self::ClearPriority
            | Self::SetReady(_)
            | Self::SetDone(_)
            | Self::SnoozeUntil(None) => {}
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        match self {
            Self::AddTag(tag) => format!("add tag {tag:?}"),
            Self::RemoveTag(tag) => format!("remove tag {tag:?}"),
            Self::SetPriority(priority) => format!("set priority {priority}"),
            Self::ClearPriority => "clear priority".into(),
            Self::SetReady(true) => "mark ready".into(),
            Self::SetReady(false) => "clear ready".into(),
            Self::SetDone(true) => "acknowledge done".into(),
            Self::SetDone(false) => "reopen".into(),
            Self::SnoozeUntil(Some(until)) => format!("snooze until {until}"),
            Self::SnoozeUntil(None) => "clear snooze".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBatchTarget {
    pub local_id: String,
    pub anchor: SourceRef,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBatchPlan {
    pub action: LocalBatchAction,
    pub targets: Vec<LocalBatchTarget>,
    pub planned_at_unix_ms: u64,
}

impl LocalBatchPlan {
    pub fn freeze(
        cards: &[&WorkCardProjection],
        action: LocalBatchAction,
        planned_at_unix_ms: u64,
    ) -> Result<Self> {
        action.validate(planned_at_unix_ms)?;
        anyhow::ensure!(!cards.is_empty(), "batch target set is empty");
        anyhow::ensure!(
            cards.len() <= MAX_BATCH_TARGETS,
            "batch target set exceeds {MAX_BATCH_TARGETS}"
        );

        let mut seen = BTreeSet::new();
        let mut targets = Vec::with_capacity(cards.len());
        for card in cards {
            anyhow::ensure!(
                seen.insert((card.anchor.clone(), card.local_id.clone())),
                "duplicate batch target {}",
                card.local_id
            );
            targets.push(LocalBatchTarget {
                local_id: card.local_id.clone(),
                anchor: card.anchor.clone(),
                title: card.title.clone(),
            });
        }

        Ok(Self {
            action,
            targets,
            planned_at_unix_ms,
        })
    }

    pub fn validate(&self) -> Result<()> {
        self.action.validate(self.planned_at_unix_ms)?;
        anyhow::ensure!(!self.targets.is_empty(), "batch target set is empty");
        anyhow::ensure!(
            self.targets.len() <= MAX_BATCH_TARGETS,
            "batch target set exceeds {MAX_BATCH_TARGETS}"
        );
        let mut seen = BTreeSet::new();
        for target in &self.targets {
            anyhow::ensure!(
                !target.local_id.trim().is_empty(),
                "batch target local_id must not be empty"
            );
            anyhow::ensure!(
                !target.anchor.value.trim().is_empty(),
                "batch target anchor must not be empty"
            );
            anyhow::ensure!(
                seen.insert((target.anchor.clone(), target.local_id.clone())),
                "duplicate batch target {}",
                target.local_id
            );
        }
        Ok(())
    }

    pub fn preview(&self) -> String {
        format!(
            "{} · {} frozen target{}",
            self.action.label(),
            self.targets.len(),
            if self.targets.len() == 1 { "" } else { "s" }
        )
    }
}

pub fn parse_priority(value: &str) -> Result<i32> {
    value
        .trim()
        .parse::<i32>()
        .with_context(|| format!("invalid priority: {value:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{
        PlanningAttention, SourceKind, WorkCardOverlay, WorkflowStage,
    };

    fn card(local_id: &str) -> WorkCardProjection {
        WorkCardProjection {
            local_id: local_id.into(),
            anchor: SourceRef {
                kind: SourceKind::ScratchWork,
                value: local_id.into(),
            },
            title: local_id.into(),
            workspace: None,
            branch: None,
            forge_provider: None,
            change_request_state: None,
            change_request_draft: false,
            stage: WorkflowStage::Inbox,
            stage_reason: "test".into(),
            attention: Vec::<PlanningAttention>::new(),
            links: vec![],
            overlay: WorkCardOverlay::default(),
            goal: None,
            snoozed: false,
            provenance: vec![],
        }
    }

    #[test]
    fn freeze_keeps_exact_visible_target_set() {
        let first = card("scratch:1");
        let second = card("scratch:2");
        let plan = LocalBatchPlan::freeze(
            &[&first, &second],
            LocalBatchAction::SetDone(true),
            100,
        )
        .expect("plan");
        assert_eq!(plan.targets.len(), 2);
        assert_eq!(plan.targets[0].local_id, "scratch:1");
        assert_eq!(plan.targets[1].local_id, "scratch:2");
    }

    #[test]
    fn malformed_batch_actions_fail_closed() {
        let one = card("scratch:1");
        assert!(
            LocalBatchPlan::freeze(&[&one], LocalBatchAction::AddTag(" ".into()), 100).is_err()
        );
        assert!(
            LocalBatchPlan::freeze(
                &[&one],
                LocalBatchAction::SnoozeUntil(Some(100)),
                100,
            )
            .is_err()
        );
        assert!(parse_priority("abc").is_err());
    }
}
