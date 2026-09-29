# Core user flows

Date: 2026-09-29

These flows define the product before implementation details.

## Flow A — Find what needs attention

1. Start codex-tui.
2. Registry loads thread metadata, not full transcripts.
3. Needs You is visible first.
4. User presses the attention-jump command.
5. Exact thread opens.

Success means the user does not need to remember terminal windows or thread IDs.

## Flow B — Continue an existing conversation

1. Select a thread.
2. Resume/read exact thread identity through App Server.
3. Recent turns/items render.
4. Draft is restored if present.
5. User submits.
6. Streaming output appears.
7. Completion updates attention state.
8. Back returns to the same registry selection.

## Flow C — Work across several projects

1. Registry groups threads by derived workspace.
2. Filter narrows by workspace/title/status.
3. Switching threads preserves each draft and scroll anchor.
4. No background refresh steals the selected thread.

## Flow D — Handle an approval

1. Thread becomes Needs You.
2. Approval view identifies operation, cwd/path/host, reason and escalation.
3. User approves/denies.
4. Thread status returns to Working or Ready.
5. Attention is cleared only after the request is resolved.

## Flow E — Detect unsafe parallel editing

1. Two active editing threads resolve to the same mutable Git checkout.
2. Registry/thread view shows a collision warning.
3. User may continue intentionally or move/fork later into a worktree.
4. v1 does not silently create or delete worktrees.

## Flow F — Review changes

1. Open Review from a selected thread/workspace.
2. Inspect changed files and diff.
3. Jump back to conversation or open external editor/browser.
4. Git remains authoritative; codex-tui does not own merge state.

## Flow G — Small-team usage

1. Team commits AGENTS.md / .codex project configuration as appropriate.
2. Each developer runs their own codex-tui and Codex sessions.
3. GitHub issues/PRs/CI remain shared collaboration surfaces.
4. codex-tui local pins/drafts/scroll state remain private.

No central codex-tui service is required.

## Flow H — Degraded compatibility

1. App Server lacks an optional experimental feature.
2. Capability adapter marks it unavailable.
3. Core registry/thread interaction continues using fallbacks.
4. Doctor explains the missing capability.

An optional feature must never make the baseline product unusable.
