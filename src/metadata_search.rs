use crate::domain::{CwdLocality, ThreadSummary};
use crate::forge::ForgeObservation;
use crate::goal::GoalObservation;
use crate::planning::{LinkRole, SourceKind, WorkCardProjection};

#[derive(Clone, Copy)]
pub struct MetadataSearchContext<'a> {
    pub thread: &'a ThreadSummary,
    pub locality: Option<CwdLocality>,
    pub goal: Option<&'a GoalObservation>,
    pub forge: Option<&'a ForgeObservation>,
    pub card: Option<&'a WorkCardProjection>,
}

pub fn matches_metadata_query(context: MetadataSearchContext<'_>, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    query
        .split_whitespace()
        .all(|token| token_matches(&context, token))
}

fn token_matches(context: &MetadataSearchContext<'_>, token: &str) -> bool {
    if is_locality_filter_token(token) {
        return context
            .locality
            .is_some_and(|locality| token == locality.label());
    }

    if let Some((field, value)) = token.split_once(':') {
        if value.is_empty() {
            return false;
        }
        return match field {
            "thread" => thread_fields_match(context.thread, value),
            "cwd" => fuzzy_subsequence(value, &context.thread.metadata.cwd),
            "project" => project_fields_match(context, value),
            "goal" => context
                .goal
                .is_some_and(|goal| goal_fields_match(goal, value)),
            "forge" => context
                .forge
                .is_some_and(|forge| forge_fields_match(forge, value)),
            "card" | "work" => context
                .card
                .is_some_and(|card| work_card_fields_match(card, value)),
            "link" | "related" => context
                .card
                .is_some_and(|card| work_card_links_match(card, value)),
            "worktree" => context.card.is_some_and(|card| {
                card.links.iter().any(|link| {
                    link.role == LinkRole::Worktree && fuzzy_subsequence(value, &link.source.value)
                })
            }),
            "stage" => context
                .card
                .is_some_and(|card| fuzzy_subsequence(value, card.stage.label())),
            "tag" => context.card.is_some_and(|card| {
                card.overlay
                    .tags
                    .iter()
                    .any(|tag| fuzzy_subsequence(value, tag))
            }),
            _ => false,
        };
    }

    thread_fields_match(context.thread, token)
        || context
            .goal
            .is_some_and(|goal| goal_fields_match(goal, token))
        || context
            .forge
            .is_some_and(|forge| forge_fields_match(forge, token))
        || context
            .card
            .is_some_and(|card| work_card_fields_match(card, token))
}

pub fn filter_requires_locality(query: &str) -> bool {
    query.split_whitespace().any(is_locality_filter_token)
}

fn is_locality_filter_token(token: &str) -> bool {
    matches!(
        token,
        "local" | "stale" | "foreign-windows" | "foreign-unix" | "relative" | "empty"
    )
}

fn thread_fields_match(thread: &ThreadSummary, token: &str) -> bool {
    [
        thread.id.0.as_str(),
        thread.display_title(),
        thread.title.as_str(),
        thread.workspace.as_str(),
        thread.metadata.cwd.as_str(),
        thread.metadata.source.as_str(),
        thread.metadata.workspace_key.as_str(),
        thread.metadata.model.as_deref().unwrap_or_default(),
        thread.metadata.project_id.as_deref().unwrap_or_default(),
    ]
    .into_iter()
    .any(|field| fuzzy_subsequence(token, field))
}

fn project_fields_match(context: &MetadataSearchContext<'_>, token: &str) -> bool {
    fuzzy_subsequence(token, &context.thread.workspace)
        || context
            .thread
            .metadata
            .project_id
            .as_deref()
            .is_some_and(|value| fuzzy_subsequence(token, value))
        || context.forge.is_some_and(|forge| {
            forge.identity.as_ref().is_some_and(|identity| {
                fuzzy_subsequence(token, &identity.path_with_namespace)
                    || fuzzy_subsequence(token, &identity.project_id)
                    || fuzzy_subsequence(token, &identity.host)
            })
        })
        || context.card.is_some_and(|card| {
            card.workspace
                .as_deref()
                .is_some_and(|value| fuzzy_subsequence(token, value))
        })
}

fn goal_fields_match(goal: &GoalObservation, token: &str) -> bool {
    fuzzy_subsequence(token, &goal.objective)
        || fuzzy_subsequence(token, goal.status.label())
        || fuzzy_subsequence(token, goal.status.wire())
}

fn forge_fields_match(forge: &ForgeObservation, token: &str) -> bool {
    if let Some(identity) = &forge.identity
        && [
            identity.provider.label(),
            identity.host.as_str(),
            identity.project_id.as_str(),
            identity.path_with_namespace.as_str(),
            identity.default_branch.as_deref().unwrap_or_default(),
        ]
        .into_iter()
        .any(|field| fuzzy_subsequence(token, field))
    {
        return true;
    }

    forge.issues.iter().any(|issue| {
        fuzzy_subsequence(token, &issue.title)
            || fuzzy_subsequence(token, &issue.state)
            || fuzzy_subsequence(token, &issue.iid.to_string())
    }) || forge.change_requests.iter().any(|change| {
        fuzzy_subsequence(token, &change.title)
            || fuzzy_subsequence(token, &change.state)
            || fuzzy_subsequence(token, &change.source_branch)
            || fuzzy_subsequence(token, &change.target_branch)
            || fuzzy_subsequence(token, &change.iid.to_string())
    }) || forge.pipelines.iter().any(|pipeline| {
        fuzzy_subsequence(token, &pipeline.status)
            || fuzzy_subsequence(token, &pipeline.reference)
            || fuzzy_subsequence(token, &pipeline.id.to_string())
    })
}

fn work_card_fields_match(card: &WorkCardProjection, token: &str) -> bool {
    if [
        card.local_id.as_str(),
        card.title.as_str(),
        card.workspace.as_deref().unwrap_or_default(),
        card.branch.as_deref().unwrap_or_default(),
        card.stage.label(),
        card.stage_reason.as_str(),
        card.overlay.title_override.as_deref().unwrap_or_default(),
        card.overlay.note.as_deref().unwrap_or_default(),
        source_kind_label(&card.anchor.kind),
        card.anchor.value.as_str(),
        card.forge_provider
            .map(|provider| provider.label())
            .unwrap_or_default(),
        card.change_request_state.as_deref().unwrap_or_default(),
    ]
    .into_iter()
    .any(|field| fuzzy_subsequence(token, field))
    {
        return true;
    }

    card.overlay
        .tags
        .iter()
        .any(|tag| fuzzy_subsequence(token, tag))
        || card
            .attention
            .iter()
            .any(|reason| fuzzy_subsequence(token, reason.label()))
        || work_card_links_match(card, token)
}

fn work_card_links_match(card: &WorkCardProjection, token: &str) -> bool {
    card.links.iter().any(|link| {
        fuzzy_subsequence(token, link.role.label())
            || fuzzy_subsequence(token, source_kind_label(&link.source.kind))
            || fuzzy_subsequence(token, &link.source.value)
    })
}

fn source_kind_label(kind: &SourceKind) -> &'static str {
    match kind {
        SourceKind::ScratchWork => "scratch-work",
        SourceKind::CodexThread => "codex-thread",
        SourceKind::ForgeWorkItem => "forge-work-item",
        SourceKind::Goal => "goal",
        SourceKind::Worktree => "worktree",
        SourceKind::ChangeRequest => "change-request",
    }
}

fn fuzzy_subsequence(needle: &str, haystack: &str) -> bool {
    if needle.is_empty() {
        return true;
    }

    let mut remaining = needle.chars();
    let mut current = remaining.next();
    for raw in haystack.chars() {
        for candidate in raw.to_lowercase() {
            if current == Some(candidate) {
                current = remaining.next();
                if current.is_none() {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{CodexBackend, FakeBackend};
    use crate::forge::{
        CapabilityState, ChangeRequestSummary, ForgeCapability, ForgeFreshness, ForgeIdentity,
        ForgeProviderKind,
    };
    use crate::goal::GoalStatus;
    use crate::planning::{
        Freshness, PlanningAttention, Provenance, SourceRef, WorkCardLink, WorkCardOverlay,
        WorkflowStage,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn fixture() -> (
        ThreadSummary,
        GoalObservation,
        ForgeObservation,
        WorkCardProjection,
    ) {
        let thread = FakeBackend::seeded().snapshot().threads[0].clone();
        let goal = GoalObservation {
            thread_id: thread.id.clone(),
            objective: "Ship acoustic search pipeline".into(),
            status: GoalStatus::Blocked,
            token_budget: None,
            tokens_used: 0,
            time_used_seconds: 0,
            created_at: 0,
            updated_at: 0,
            observed_at_unix_ms: 1,
        };
        let forge = ForgeObservation {
            thread_id: thread.id.clone(),
            cwd: thread.metadata.cwd.clone(),
            remote_name: Some("origin".into()),
            remote_url: None,
            identity: Some(ForgeIdentity {
                provider: ForgeProviderKind::GitHub,
                host: "github.com".into(),
                project_id: "42".into(),
                path_with_namespace: "team/audio-pipeline".into(),
                web_url: "https://github.com/team/audio-pipeline".into(),
                default_branch: Some("main".into()),
            }),
            capabilities: BTreeMap::from([(
                ForgeCapability::MergeRequests,
                CapabilityState::Available,
            )]),
            issues: vec![],
            change_requests: vec![ChangeRequestSummary {
                iid: 17,
                title: "Close search gap".into(),
                state: "open".into(),
                source_branch: "feature/search".into(),
                target_branch: "main".into(),
                web_url: String::new(),
                updated_at: None,
                draft: false,
                detailed_merge_status: None,
                blocking_discussions_resolved: None,
            }],
            pipelines: vec![],
            review: None,
            observed_at_unix_ms: 1,
            freshness: ForgeFreshness::Fresh,
            error: None,
        };
        let card = WorkCardProjection {
            local_id: format!("thread:{}", thread.id.0),
            anchor: SourceRef::codex_thread(&thread.id),
            title: "Search closure".into(),
            workspace: Some("audio".into()),
            branch: Some("feature/search".into()),
            forge_provider: Some(ForgeProviderKind::GitHub),
            change_request_state: Some("open".into()),
            change_request_draft: false,
            stage: WorkflowStage::Review,
            stage_reason: "change request".into(),
            attention: BTreeSet::from([PlanningAttention::ChangeRequested]),
            snoozed: false,
            overlay: WorkCardOverlay {
                tags: BTreeSet::from(["research".into()]),
                note: Some("customer metadata".into()),
                ..WorkCardOverlay::default()
            },
            links: vec![
                WorkCardLink {
                    role: LinkRole::Goal,
                    source: SourceRef {
                        kind: SourceKind::Goal,
                        value: thread.id.0.clone(),
                    },
                },
                WorkCardLink {
                    role: LinkRole::Worktree,
                    source: SourceRef {
                        kind: SourceKind::Worktree,
                        value: "/repo/audio-search".into(),
                    },
                },
            ],
            goal: Some(goal.clone()),
            provenance: vec![Provenance {
                source: "fixture".into(),
                observed_at_unix_ms: Some(1),
                source_revision: None,
                freshness: Freshness::Fresh,
                degraded_reason: None,
            }],
        };
        (thread, goal, forge, card)
    }

    #[test]
    fn unified_query_matches_goal_forge_workcard_and_relationship_metadata() {
        let (thread, goal, forge, card) = fixture();
        let context = MetadataSearchContext {
            thread: &thread,
            locality: Some(CwdLocality::LocalDirectory),
            goal: Some(&goal),
            forge: Some(&forge),
            card: Some(&card),
        };

        for query in [
            "acoustic blocked",
            "project:audio-pipeline",
            "goal:acoustic",
            "forge:github",
            "work:research",
            "link:worktree",
            "worktree:audio-search",
            "stage:review",
            "local customer",
        ] {
            assert!(
                matches_metadata_query(MetadataSearchContext { ..context }, query),
                "{query}"
            );
        }
    }

    #[test]
    fn metadata_search_does_not_need_or_accept_transcript_content() {
        let (mut thread, goal, forge, card) = fixture();
        thread.title = "visible metadata only".into();
        let context = MetadataSearchContext {
            thread: &thread,
            locality: None,
            goal: Some(&goal),
            forge: Some(&forge),
            card: Some(&card),
        };
        assert!(!matches_metadata_query(
            context,
            "private-transcript-sentinel"
        ));
    }

    #[test]
    fn unknown_scoped_field_fails_closed() {
        let (thread, goal, forge, card) = fixture();
        assert!(!matches_metadata_query(
            MetadataSearchContext {
                thread: &thread,
                locality: None,
                goal: Some(&goal),
                forge: Some(&forge),
                card: Some(&card),
            },
            "unknown:value",
        ));
    }
}
