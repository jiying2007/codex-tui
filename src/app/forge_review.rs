//! Review evidence is bounded by its own age and the requested change revision.
use super::*;
use crate::forge::{ForgeReviewResult, ForgeReviewSummary};

fn fresh(review: &ForgeReviewSummary) -> bool {
    review.error.is_none()
        && review.observed_at_unix_ms > 0
        && now_unix_ms().saturating_sub(review.observed_at_unix_ms) <= FORGE_REFRESH_TTL_MS
}

pub(super) fn retain(
    existing: &ForgeObservation,
    incoming: &ForgeObservation,
    cwd: &str,
    branch: Option<&str>,
) -> Option<ForgeReviewSummary> {
    if existing.identity.is_none() || existing.identity != incoming.identity {
        return None;
    }
    let review = existing
        .review
        .as_ref()
        .filter(|review| review.cwd == cwd && fresh(review))?;
    let previous = existing
        .change_requests
        .iter()
        .find(|change| change.iid == review.change_request_iid)?;
    let current = incoming.change_request_for_branch(branch?)?;
    (current.iid == review.change_request_iid && previous.head_sha == current.head_sha)
        .then(|| review.clone())
}

fn clear(observation: &mut ForgeObservation) -> bool {
    let removed = observation.review.take().is_some();
    for capability in [
        ForgeCapability::ApprovalSummary,
        ForgeCapability::Discussions,
    ] {
        observation
            .capabilities
            .insert(capability, CapabilityState::Unknown);
    }
    removed
}

pub(super) fn context_changed(state: &mut AppState, context: &GitContext) {
    let Some(observation) = state.forge_observations.get_mut(&context.thread_id.0) else {
        return;
    };
    if observation.review.is_none() {
        return;
    }
    let matches = context.cwd == observation.cwd
        && context.error.is_none()
        && context
            .branch
            .as_deref()
            .and_then(|branch| observation.change_request_for_branch(branch))
            .zip(observation.review.as_ref())
            .is_some_and(|(change, review)| {
                change.iid == review.change_request_iid && fresh(review)
            });
    if !matches {
        clear(observation);
    }
}

pub(super) fn probe(state: &mut AppState, thread_id: &ThreadId, cwd: &str) -> Option<Effect> {
    let context = state
        .git_context(thread_id)
        .filter(|context| context.cwd == cwd)?;
    let branch = context.branch.as_deref()?;
    let observation = state
        .forge_observation(thread_id)
        .filter(|observation| observation.cwd == cwd)?;
    let identity = observation.identity.as_ref()?;
    let change = observation.change_request_for_branch(branch)?;
    if observation.review.as_ref().is_some_and(|review| {
        review.change_request_iid == change.iid && review.cwd == cwd && fresh(review)
    }) {
        return None;
    }
    let target = ForgeReviewTarget {
        thread_id: thread_id.clone(),
        cwd: cwd.to_owned(),
        provider: identity.provider,
        host: identity.host.clone(),
        project_id: identity.project_id.clone(),
        project_path: identity.path_with_namespace.clone(),
        change_request_iid: change.iid,
        head_sha: change.head_sha.clone(),
    };
    // Expired counts must not appear alongside the freshly loaded Git/Forge header.
    if let Some(observation) = state.forge_observations.get_mut(&thread_id.0)
        && clear(observation)
    {
        state.planning_generation = state.planning_generation.wrapping_add(1);
    }
    Some(Effect::ProbeForgeReview(target))
}

pub(super) fn install(state: &mut AppState, result: ForgeReviewResult) {
    let crate::forge::ForgeReviewResult {
        target,
        summary: review,
    } = result;
    if state
        .thread_by_id(&target.thread_id)
        .is_none_or(|thread| thread.metadata.cwd != target.cwd)
        || target.thread_id != review.thread_id
        || target.cwd != review.cwd
        || target.change_request_iid != review.change_request_iid
    {
        return;
    }
    let Some(branch) = state
        .git_context(&target.thread_id)
        .filter(|context| context.cwd == target.cwd && context.error.is_none())
        .and_then(|context| context.branch.clone())
    else {
        return;
    };
    if let Some(observation) = state.forge_observations.get_mut(&review.thread_id.0)
        && observation.cwd == review.cwd
        && observation.identity.as_ref().is_some_and(|identity| {
            identity.provider == target.provider
                && identity.host == target.host
                && identity.project_id == target.project_id
                && identity.path_with_namespace == target.project_path
        })
        && observation
            .change_request_for_branch(&branch)
            .is_some_and(|change| {
                change.iid == review.change_request_iid && change.head_sha == target.head_sha
            })
    {
        observation.capabilities.insert(
            ForgeCapability::ApprovalSummary,
            if review.approvals_available {
                CapabilityState::Available
            } else {
                CapabilityState::Unavailable
            },
        );
        observation.capabilities.insert(
            ForgeCapability::Discussions,
            if review.discussions_available {
                CapabilityState::Available
            } else {
                CapabilityState::Unavailable
            },
        );
        observation.review = Some(review);
    }
}
