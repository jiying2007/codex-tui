# Deep review: personal-first, team reuse, GitLab compatibility

Date: 2026-09-29
Status: accepted review findings

## Executive conclusion

The current final plan is directionally strong, but three structural corrections are required before implementation:

1. Product priority must be explicitly personal-first; team value is mainly reuse and projection over existing shared systems, not collaborative session management.
2. GitHub-specific concepts must be removed from the core domain and replaced with a code-forge abstraction. GitLab Self-Managed must be a first-class target because it is the current internal platform.
3. Attention and workflow stage are currently mixed. Needs You must be an orthogonal attention overlay, not a canonical board stage.

After these corrections, the design becomes simpler and more aligned with actual use.

## 1. Product priority: individual-first

The primary product persona is one developer operating many Codex threads across many repositories.

Team use is mainly:
- reusable repository instructions
- reusable skills
- reusable project configuration
- reusable launch/review conventions
- shared Git repository
- shared issue/MR/CI state
- common project structure

It is not primarily:
- shared live Codex sessions
- team presence
- team chat
- shared drafts
- shared local attention state
- central codex-tui server

Priority:

    Individual workflow quality
            first
            |
    Team reuse through repository / forge
            second
            |
    Optional live collaboration
            last

## 2. Team reuse model

Preferred shared assets:
- AGENTS.md
- .codex/config.toml
- project skills/plugins
- repository scripts / Makefile / justfile / package scripts
- CI configuration
- GitLab/GitHub issue and change-request conventions

Only codex-tui-specific information should ever require a codex-tui repository manifest.

If a future repo-local codex-tui manifest is introduced, keep it deliberately small:
- optional display name
- optional saved-view templates
- references to existing build/test/review commands
- optional worktree setup hints

Do not duplicate Codex policy or team task state.

## 3. Code forge, not GitHub, is the domain concept

Current design hard-codes GitHub Issue, GitHub Project, Pull Request and GitHub CI. This is wrong for the core architecture.

Use normalized concepts:
- ForgeWorkItem
- ForgeBoard
- ChangeRequest
- Pipeline
- ReviewState

Provider examples:
- GitLabIssue / GitLabWorkItem
- GitLabIssueBoard
- GitLabMergeRequest
- GitLabPipeline
- GitHubIssue
- GitHubProject
- GitHubPullRequest
- GitHubChecks

Core UI and WorkCard code must not branch on GitHub vs GitLab.

## 4. GitLab must be first-class

Recommended integration order:
1. local Git — core
2. GitLab Self-Managed — first forge provider
3. GitHub — second forge provider
4. other forges only if demand exists

GitLab support must handle custom hostname, self-managed instances, groups/subgroups, SSH/HTTPS remotes, Issues/Work Items, Issue Boards, Merge Requests, Pipelines, approvals/review state where available, and instance version/edition differences.

## 5. GitLab authentication strategy

Do not make codex-tui a credential manager.

Initial preferred transport:

    codex-tui
       |
    GitLabForgeProvider
       |
    glab CLI / glab api
       |
    GitLab Self-Managed

Why:
- glab supports GitLab.com and Self-Managed
- glab supports multiple authenticated hosts
- glab detects GitLab hosts from Git remotes
- glab supports OAuth/PAT without codex-tui storing the token
- glab list commands provide JSON
- glab api supports pagination and NDJSON

codex-tui should detect glab, detect the matching authenticated host, call it with explicit repo/host context, parse JSON rather than terminal text, bound every invocation with timeout/cancellation, and expose glab version/auth/host in doctor.

Later, a native REST/GraphQL transport can be added behind the same provider interface if performance or functionality requires it.

## 6. Generic forge provider

Suggested internal contract:

    ForgeProvider
      detect(remote)
      health()
      current_identity()
      list_work_items(...)
      get_work_item(...)
      list_boards(...)
      get_board(...)
      list_change_requests(...)
      get_change_request(...)
      get_pipeline_summary(...)
      get_review_state(...)
      open_web(...)

Mutation APIs should be added later and explicitly: create_work_item, update_work_item, create_change_request, comment, approve and merge.

Read-only projection should come first.

## 7. Normalized forge references

ForgeIdentity: provider, host, namespace, repository, optional project_id.

ForgeWorkItemRef: forge identity, stable remote id, display iid/number, type, title, state, labels, assignee summary and web URL.

ChangeRequestRef is the generic name for GitLab Merge Request and GitHub Pull Request. It carries forge identity, remote id, display iid/number, source branch, target branch, state, draft, mergeability, review state, pipeline summary and web URL.

PipelineSummary carries status, ref/sha, pipeline id, web URL and optional failed-jobs summary.

Provider-specific payload remains outside the UI domain.

## 8. Critical inconsistency: workflow stage vs attention

The previous board was:

    Inbox -> Ready -> Working -> Needs You -> Review -> Done

This conflicts with the rule that attention is independent from workflow state.

A thread can be Working and simultaneously need approval. Moving the card from Working to Needs You loses the real workflow stage.

Correct model:

    Workflow:
    Inbox -> Ready -> Working -> Review -> Done

    Attention overlay:
            ! Needs You

Board rendering can show a virtual attention swimlane or badge:

    NEEDS YOU
      ! KWS decoder        [Working]
      ! Audio i026         [Review]

    BOARD
    Inbox | Ready | Working | Review | Done

The Needs You view is a saved/virtual view, not source workflow state.

Blocked can be represented as workflow still Working plus attention=Blocked, or as an optional derived lane in a specific saved view, but the domain model remains orthogonal.

## 9. WorkCard identity needs strengthening

The current WorkCard can reference issue/thread/goal/worktree/change request, but its identity rules are underspecified.

Use:

    WorkCard
      local_id
      anchor
      links[]
      overlays
      derived_stage
      attention[]

Exactly one anchor is primary: Scratch, Forge Work Item, or Codex Thread. Goal/worktree/change-request normally become links, not competing identity roots.

This prevents duplicate cards when a scratch item becomes a GitLab issue and later gains a thread/MR.

## 10. Board source precedence

Suggested stage derivation order:
1. explicit terminal delivery state: merged/closed -> Done
2. review/change-request state -> Review
3. active execution/goal -> Working
4. selected but not started -> Ready
5. scratch/imported/unplanned -> Inbox

Attention is evaluated separately after stage.

Do not derive Done merely from an idle thread. Do not derive Review solely from thread completion without checking code/change state.

## 11. Goal should be UX-core but capability-optional

Goal is valuable, but the baseline must not depend on it being available or stable in every Codex version.

Correct rule:
- Goal-aware UX is part of the mature product.
- Native Codex Goal is preferred when capability exists.
- If absent, do not emulate a second full Goal engine.
- Board/Attention continue with thread/Git/forge data.

The same rule applies to experimental Thread Queue APIs.

## 12. Persistence refinement

The state-file-first approach remains correct, but introduce a LocalStore abstraction from day one so persistence is replaceable.

Initial implementation: atomic JSON/TOML state.
Mature implementation: SQLite when Saved Views, Scratch and relationship mappings justify it.

This avoids leaking JSON file shape into the application domain.

## 13. Revised product layers

Core A — Personal Codex control: Mission Control, Attention, Thread, Goal-aware UI, Quick Prompt, Search, Doctor.

Core B — Personal engineering workspace: Git, worktree, diff/review, notes/bookmarks, hot slots, saved views, local board/scratch.

Reuse layer — Repository shared assets: AGENTS.md, .codex config, skills, existing project commands, optional tiny codex-tui view/preset manifest later.

Forge layer — GitLab first, GitHub second.

Optional collaboration layer — remote/web/presence, never required for the personal core.

## 14. Revised terminology

Replace core documentation terms:
- GitHub -> Code Forge where generic
- GitHub Issue -> Forge Work Item
- GitHub Project -> Forge Board / external planning view
- Pull Request -> Change Request
- PR -> MR/PR only in provider-specific UI
- GitHub CI -> Pipeline / Checks
- GitHub integration -> Forge integration

Provider UI may still use native language: GitLab uses Issue/Merge Request/Pipeline; GitHub uses Issue/Pull Request/Checks.

## 15. Priority changes

Previous M6 GitHub-specific milestone becomes M6 — Forge integration.

M6a:
- GitLab Self-Managed read-only
- remote/host detection
- glab health/auth
- work items/issues
- issue boards
- merge requests
- pipeline/review summary

M6b:
- explicit GitLab mutations needed by workflow

M6c:
- GitHub provider with the same normalized contract

## 16. Final reviewed positioning

codex-tui is a personal-first, local-first Codex engineering workbench. It helps one developer plan, monitor, control and review many Codex tasks across repositories. Teams gain value primarily by reusing repository-owned instructions, skills, conventions and code-forge state; no shared codex-tui service is required. GitLab Self-Managed and GitHub are integrations behind a generic forge boundary, with GitLab first-class for current internal use.