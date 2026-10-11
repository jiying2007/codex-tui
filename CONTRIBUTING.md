<!-- docs-id: contributing -->
<!-- docs-lang: en -->
# Contributing to codex-tui
<!-- docs-section: overview -->

**Language / 语言:** [English](CONTRIBUTING.md) · [简体中文](CONTRIBUTING.zh-CN.md)

Thanks for improving a personal-first, Linux Tier 1 terminal workbench. Read [the bilingual documentation index](docs/README.md) first. All work follows protected PRs; this document does not authorize Stable publication.

## Change scope and architecture
<!-- docs-section: scope -->

Codex owns conversations, agent runtime and queue; Git owns worktrees; GitLab/GitHub own delivery; codex-tui owns the local projections and small operator metadata. Maintain ADR-012 and `release/v1.5-convergence.json`. Do not create another agent orchestrator, remote daemon, task database or general terminal manager. Existing conversation/queue/drawer modules are maintenance-only. `src/app.rs` owns the AppState projection and supporting helpers; `src/app/reducer.rs` is the single, publicly re-exported `app::reduce` Action-to-Effect transition boundary. Keep user-intent ordering and effects unchanged during maintenance-only refactors; all source modules must remain in the single v1.4 LOC ratchet.

## Local development
<!-- docs-section: setup -->

Requires Rust >= 1.88, Cargo with checked-in `Cargo.lock`, Python 3.8+ for repository release helpers, and optional authenticated `codex`, `gh` or `glab` for real provider smoke. Use a feature branch; never amend published tags or bypass protected main.

~~~bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
python scripts/docs/check_docs.py
python -m unittest discover -s scripts/release -p "test_*.py"
~~~

The docs checker uses only Python's standard library. Host-only tests are useful but never substitute for exact-SHA CI.

## Branches, commits, and PR review
<!-- docs-section: branch -->

Use focused branches and reviewable commits (Conventional Commit prefix such as `fix:`, `docs:`, `refactor:`, `test:` or `ci:`; Chinese/English subjects are both acceptable). A PR should identify the user-observable problem, bounds, security implications, exact files changed, reproducible test output, any new source module under the **single** `release/v1.4-plan.json` LOC ratchet, and any remaining manual/environment gates. Do not auto-merge with failing checks. Delete a merged work branch only with exact-ref/hygiene evidence; never force-delete divergent historical branches.

## CI and exact-source evidence
<!-- docs-section: gates -->

Canonical CI includes Linux/macOS/Windows, Rust MSRV, `cargo fmt`/`clippy`/`test` and Python release guards. Development Qualification and Release Gate have distinct authority. Hosted PTY, 10k/50k scale and soak are **synthetic**. Real Codex account, internal GitLab, Windows→SSH Ubuntu controlling-TTY, GitHub admin state and a stable `publish=false` qualification must be separately observed; no fake PASS.

## Bilingual documentation and repository hygiene
<!-- docs-section: docs -->

For changes to user CLI, config, safety, or release rules, update the English and Chinese pages listed in `docs/i18n/manifest.json` together. Run `python scripts/docs/check_docs.py`. Fix broken local links and use identical literal command names in both languages. Current guides are bilingual; historical research/design documents remain English originals, not auto-generated translations. Keep UTF-8 LF, no secrets, no state DB, no arbitrary generated logs or unsafe license copies. See `.editorconfig` and `.gitattributes`.

## Security and privacy
<!-- docs-section: security -->

Do not include real credentials, internal repository paths, raw provider stderr, conversation text, or production terminal transcripts in issues, tests or artifacts. Use isolated synthetic fixtures and negative tests for redaction, size bounds and failure behavior. Follow [SECURITY](SECURITY.md) for confidential reports.

## Release and ownership boundaries
<!-- docs-section: release -->

Do not dispatch Stable publication as part of a contribution. The v1.4 first-deployment candidate is not released and cannot import unused v1.0 JSON/v1-v3 SQLite state. Release requires exact-source protected CI, immutable GitHub Releases, real environment receipts and explicit authorization. See [release qualifications](docs/guides/release-qualification.md). A green PR is **not** stable-ready.

