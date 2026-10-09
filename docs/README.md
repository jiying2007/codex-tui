<!-- docs-id: docs-index -->
<!-- docs-lang: en -->
# Documentation index — active guides and historical evidence
<!-- docs-section: overview -->

**Language / 语言:** [English](README.md) · [简体中文](README.zh-CN.md)

The **current operator and governance documentation** is available in English and Simplified Chinese. The existing implementation, research, release checkpoint, and ADR archives remain reviewable in their original language. Historical files are evidence, **not permission to publish** or promises that a feature is in use.

## Start here
<!-- docs-section: start -->

- [Project overview](../README.md) · [中文项目概览](../README.zh-CN.md)
- [Team quickstart](team-quickstart.md) · [Installation and first deployment](release/install-upgrade.md)
- [Daily operator handbook](guides/operator-guide.md) · [CLI command reference](guides/cli-reference.md) · [Troubleshooting](guides/troubleshooting.md)

## Runtime authorities and release qualification
<!-- docs-section: operators -->

- [Provider qualification](qualification/provider.md) · [First-deployment data decision](implementation/v1.4-first-deployment-baseline.md)
- [Release gates and human qualification](guides/release-qualification.md)
- [ADR-012: thin control plane](adr/012-upstream-convergence.md)
- [English detailed Release archive contract](release/install-upgrade.md)

## Repository collaboration and security
<!-- docs-section: governance -->

- [Contribution guide](../CONTRIBUTING.md) · [Security policy](../SECURITY.md)
- [Support and diagnostic scope](../SUPPORT.md) · [Community conduct](../CODE_OF_CONDUCT.md)
- [Bilingual maintenance policy](i18n/README.md) · [Documentation manifest](i18n/manifest.json)
- [Repository issue templates](../.github/ISSUE_TEMPLATE/bug_report.yml) · [PR checklist](../.github/pull_request_template.md)

## Historical engineering records
<!-- docs-section: archive -->

Implementation phases are in [implementation/](implementation/m0-bootstrap.md), design history in [design/](design/final-plan.md), review records in [reviews/](reviews/2026-09-29-final-architecture-review.md), and research in [research/](research/2026-09-29-long-term-maintainability.md). Old version plans under [release/](../release/v1.3-plan.json) and archival ADRs are intentionally not translated line by line; they remain immutable historical context.

Current **deployment/production claims** must come from exact-SHA qualified evidence, not an archived roadmap.

## Language and synchronization policy
<!-- docs-section: translation -->

Only documents registered as **active pairs** in [the manifest](i18n/manifest.json) have a bilingual parity commitment. A change to user-facing commands, safety rules or release boundaries must update **both** pages in the same PR. The Python 3.8-compatible checker validates section IDs, language switches, local links, UTF-8 and scope. Keep technical CLI tokens identical across languages.

