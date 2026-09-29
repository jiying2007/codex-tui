use crate::domain::ThreadId;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

impl GoalStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Paused => "PAUSED",
            Self::Blocked => "BLOCKED",
            Self::UsageLimited => "USAGE-LIMITED",
            Self::BudgetLimited => "BUDGET-LIMITED",
            Self::Complete => "COMPLETE",
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Blocked => "blocked",
            Self::UsageLimited => "usageLimited",
            Self::BudgetLimited => "budgetLimited",
            Self::Complete => "complete",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalObservation {
    pub thread_id: ThreadId,
    pub objective: String,
    pub status: GoalStatus,
    pub token_budget: Option<u64>,
    pub tokens_used: u64,
    pub time_used_seconds: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub observed_at_unix_ms: u64,
}

pub fn parse_goal_get(result: Value, observed_at_unix_ms: u64) -> Result<Option<GoalObservation>> {
    let Some(goal) = result.get("goal") else {
        anyhow::bail!("thread/goal/get response missing goal");
    };
    if goal.is_null() {
        return Ok(None);
    }
    parse_goal(goal, observed_at_unix_ms).map(Some)
}

pub fn parse_goal_set(result: Value, observed_at_unix_ms: u64) -> Result<GoalObservation> {
    let goal = result
        .get("goal")
        .context("thread/goal/set response missing goal")?;
    parse_goal(goal, observed_at_unix_ms)
}

pub fn parse_goal_updated(params: &Value, observed_at_unix_ms: u64) -> Result<GoalObservation> {
    let goal = params
        .get("goal")
        .context("thread/goal/updated notification missing goal")?;
    parse_goal(goal, observed_at_unix_ms)
}

pub fn parse_goal_cleared_thread(params: &Value) -> Result<ThreadId> {
    params
        .get("threadId")
        .and_then(Value::as_str)
        .map(ThreadId::new)
        .context("thread/goal/cleared notification missing threadId")
}

fn parse_goal(value: &Value, observed_at_unix_ms: u64) -> Result<GoalObservation> {
    let thread_id = value
        .get("threadId")
        .and_then(Value::as_str)
        .map(ThreadId::new)
        .context("goal missing threadId")?;
    let objective = value
        .get("objective")
        .and_then(Value::as_str)
        .context("goal missing objective")?
        .to_string();
    let status = parse_status(
        value
            .get("status")
            .and_then(Value::as_str)
            .context("goal missing status")?,
    )?;

    Ok(GoalObservation {
        thread_id,
        objective,
        status,
        token_budget: value.get("tokenBudget").and_then(Value::as_u64),
        tokens_used: value
            .get("tokensUsed")
            .and_then(Value::as_u64)
            .context("goal missing tokensUsed")?,
        time_used_seconds: value
            .get("timeUsedSeconds")
            .and_then(Value::as_u64)
            .context("goal missing timeUsedSeconds")?,
        created_at: value
            .get("createdAt")
            .and_then(Value::as_i64)
            .context("goal missing createdAt")?,
        updated_at: value
            .get("updatedAt")
            .and_then(Value::as_i64)
            .context("goal missing updatedAt")?,
        observed_at_unix_ms,
    })
}

fn parse_status(value: &str) -> Result<GoalStatus> {
    match value {
        "active" => Ok(GoalStatus::Active),
        "paused" => Ok(GoalStatus::Paused),
        "blocked" => Ok(GoalStatus::Blocked),
        "usageLimited" => Ok(GoalStatus::UsageLimited),
        "budgetLimited" => Ok(GoalStatus::BudgetLimited),
        "complete" => Ok(GoalStatus::Complete),
        _ => anyhow::bail!("unknown Goal status: {value}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_goal_projection_without_creating_local_authority() {
        let goal = parse_goal_get(
            json!({
                "goal": {
                    "threadId": "thread-1",
                    "objective": "Ship M4",
                    "status": "active",
                    "tokenBudget": 10000,
                    "tokensUsed": 2500,
                    "timeUsedSeconds": 90,
                    "createdAt": 1700000000,
                    "updatedAt": 1700000100
                }
            }),
            123,
        )
        .expect("goal")
        .expect("present");
        assert_eq!(goal.thread_id.0, "thread-1");
        assert_eq!(goal.status, GoalStatus::Active);
        assert_eq!(goal.token_budget, Some(10_000));
        assert_eq!(goal.tokens_used, 2_500);
        assert_eq!(goal.observed_at_unix_ms, 123);
    }

    #[test]
    fn absent_goal_is_a_supported_empty_observation() {
        assert_eq!(
            parse_goal_get(json!({"goal": null}), 1).expect("parse"),
            None
        );
    }

    #[test]
    fn stable_goal_statuses_round_trip_to_wire_names() {
        for (wire, status) in [
            ("active", GoalStatus::Active),
            ("paused", GoalStatus::Paused),
            ("blocked", GoalStatus::Blocked),
            ("usageLimited", GoalStatus::UsageLimited),
            ("budgetLimited", GoalStatus::BudgetLimited),
            ("complete", GoalStatus::Complete),
        ] {
            assert_eq!(parse_status(wire).expect("status"), status);
            assert_eq!(status.wire(), wire);
        }
    }
}
