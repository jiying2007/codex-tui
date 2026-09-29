# ADR-005: Code forge abstraction with GitLab first-class

Date: 2026-09-29
Status: Accepted

## Decision

The core domain does not depend on GitHub-specific concepts.

Use a ForgeProvider boundary and normalized concepts: ForgeWorkItem, ForgeBoard, ChangeRequest, PipelineSummary and ReviewState.

GitLab Self-Managed is the first forge integration for current internal use. GitHub follows behind the same contract.

## Initial GitLab transport

Prefer glab CLI / glab api initially so codex-tui does not store GitLab credentials and can inherit support for self-managed hosts and multiple authenticated instances.

All glab calls must request JSON/NDJSON output, set explicit repository/host context, be timeout/cancellation bounded, be isolated behind the provider, and report version/auth/host through doctor.

A native API transport may be added later behind the same provider interface.

## Consequence

Core UI uses generic terminology. Provider-specific UI can render Merge Request for GitLab and Pull Request for GitHub.