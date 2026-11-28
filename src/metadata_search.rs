use crate::domain::{CwdLocality, ThreadSummary};
use crate::forge::ForgeObservation;
use crate::git::GitContext;
use crate::goal::GoalObservation;

#[derive(Clone, Copy)]
pub struct MetadataSearchContext<'a> {
    pub thread: &'a ThreadSummary,
    pub locality: Option<CwdLocality>,
    pub goal: Option<&'a GoalObservation>,
    pub forge: Option<&'a ForgeObservation>,
    pub git: Option<&'a GitContext>,
    pub note: Option<&'a str>,
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
            "branch" => context
                .git
                .and_then(|git| git.branch.as_deref())
                .is_some_and(|branch| fuzzy_subsequence(value, branch)),
            "worktree" => context
                .git
                .and_then(|git| git.worktree.as_ref())
                .is_some_and(|tree| fuzzy_subsequence(value, &tree.canonical_path)),
            "runtime" => fuzzy_subsequence(value, context.thread.runtime.label()),
            "note" => context
                .note
                .is_some_and(|note| fuzzy_subsequence(value, note)),
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
        || context.git.is_some_and(|git| git_fields_match(git, token))
        || context
            .note
            .is_some_and(|note| fuzzy_subsequence(token, note))
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
        thread
            .metadata
            .lineage
            .model_provider
            .as_deref()
            .unwrap_or_default(),
        thread
            .metadata
            .lineage
            .session_id
            .as_deref()
            .unwrap_or_default(),
        thread
            .metadata
            .lineage
            .parent_thread_id
            .as_deref()
            .unwrap_or_default(),
        thread
            .metadata
            .lineage
            .attached_to_thread_id
            .as_deref()
            .unwrap_or_default(),
        thread
            .metadata
            .lineage
            .forked_from_id
            .as_deref()
            .unwrap_or_default(),
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
        || context.git.is_some_and(|git| {
            git.repo
                .as_ref()
                .is_some_and(|repo| fuzzy_subsequence(token, &repo.primary_root))
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

fn git_fields_match(git: &GitContext, token: &str) -> bool {
    git.branch
        .as_deref()
        .is_some_and(|branch| fuzzy_subsequence(token, branch))
        || git
            .repo
            .as_ref()
            .is_some_and(|repo| fuzzy_subsequence(token, &repo.primary_root))
        || git
            .worktree
            .as_ref()
            .is_some_and(|tree| fuzzy_subsequence(token, &tree.canonical_path))
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
    use crate::domain::{LocalRepoIdentity, WorktreeIdentity};
    use crate::forge::{
        CapabilityState, ChangeRequestSummary, ForgeCapability, ForgeFreshness, ForgeIdentity,
        ForgeProviderKind,
    };
    use crate::goal::GoalStatus;
    use std::collections::BTreeMap;

    fn fixture() -> (ThreadSummary, GoalObservation, ForgeObservation, GitContext) {
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
                head_sha: None,
                web_url: String::new(),
                updated_at: None,
                draft: false,
                detailed_merge_status: None,
                blocking_discussions_resolved: None,
            }],
            pipelines: vec![],
            overview_source_page_saturated: false,
            review: None,
            observed_at_unix_ms: 1,
            freshness: ForgeFreshness::Fresh,
            error: None,
        };
        let mut git = GitContext::pending(thread.id.clone(), thread.metadata.cwd.clone());
        git.is_repository = true;
        git.observed_at_unix_ms = 1;
        git.branch = Some("feature/search".into());
        let repo = LocalRepoIdentity {
            git_common_dir: "/repo/audio/.git".into(),
            primary_root: "/repo/audio".into(),
        };
        git.repo = Some(repo.clone());
        git.worktree = Some(WorktreeIdentity {
            repo,
            canonical_path: "/repo/audio-search".into(),
            branch: git.branch.clone(),
            managed_by_codex_tui: false,
        });
        (thread, goal, forge, git)
    }

    #[test]
    fn native_query_matches_session_goal_forge_git_and_local_note() {
        let (thread, goal, forge, git) = fixture();
        let context = MetadataSearchContext {
            thread: &thread,
            locality: Some(CwdLocality::LocalDirectory),
            goal: Some(&goal),
            forge: Some(&forge),
            git: Some(&git),
            note: Some("customer metadata"),
        };

        for query in [
            "acoustic blocked",
            "project:audio-pipeline",
            "goal:acoustic",
            "forge:github",
            "branch:feature/search",
            "worktree:audio-search",
            "note:customer",
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
        let (mut thread, goal, forge, git) = fixture();
        thread.title = "visible metadata only".into();
        let context = MetadataSearchContext {
            thread: &thread,
            locality: None,
            goal: Some(&goal),
            forge: Some(&forge),
            git: Some(&git),
            note: Some("customer metadata"),
        };
        assert!(!matches_metadata_query(
            context,
            "private-transcript-sentinel"
        ));
    }

    #[test]
    fn unknown_scoped_field_fails_closed() {
        let (thread, goal, forge, git) = fixture();
        let context = MetadataSearchContext {
            thread: &thread,
            locality: None,
            goal: Some(&goal),
            forge: Some(&forge),
            git: Some(&git),
            note: Some("customer metadata"),
        };
        for retired in [
            "unknown:value",
            "card:thread",
            "work:research",
            "link:goal",
            "stage:ready",
            "tag:research",
        ] {
            assert!(!matches_metadata_query(context, retired));
        }
    }
}
