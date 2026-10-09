<!-- docs-id: security -->
<!-- docs-lang: en -->
# Security policy
<!-- docs-section: overview -->

**Language / 语言:** [English](SECURITY.md) · [简体中文](SECURITY.zh-CN.md)

Security is part of the first-deployment qualification. This project has not declared a paid bug-bounty program or guaranteed response SLA. Avoid public exploit details until maintainers can triage responsibly.

## Supported and historical branches
<!-- docs-section: scope -->

The active development code is `main` (v1.4 candidate; not yet deployed). Public v1.0.0 and v1.1–v1.3 checkpoint branches are preserved **for history**, not advertised as currently patched deployments. A security fix is qualified on the exact protected main SHA and does not imply an automatic stable release.

## Reporting a vulnerability
<!-- docs-section: report -->

Use GitHub's private **Report a vulnerability** flow under the repository Security tab **if enabled**. If that private flow is unavailable, contact the repository maintainer privately through an established channel; **do not post secrets or exploit payloads in a public Issue, PR, discussion or CI log**. Include an impact summary, affected commit/OS/provider, a minimal sanitized reproduction, and any safe mitigation. Do not access another person's accounts or internal data to obtain proof.

## Sensitive data and diagnostic boundaries
<!-- docs-section: data -->

Do not upload API keys, `auth.json`, environment variables, Git remote credential URLs, raw `git/gh/glab` stderr, prompts/transcripts or comment bodies. `doctor bundle` and provider receipts are designed to minimize such data, but reporters must inspect artifacts before sharing. Protect local SQLite backups/WAL/SHM and any internal GitLab identifiers. Synthetic negative fixtures belong in tests, not real credentials.

## Triage and coordinated disclosure
<!-- docs-section: response -->

The maintainer evaluates reproducibility, blast radius, whether a fix can be shipped without violating source/protocol authority, and regression tests for failure modes. Fixes go through a protected PR and exact-source CI. Disclosure timing is coordinated based on severity and impact; no unsupported promises about timelines, incident monitoring or automatic remote revocation.

