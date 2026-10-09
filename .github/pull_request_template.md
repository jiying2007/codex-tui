## Purpose / 目的

What user-visible or release-governance problem does this solve?
本 PR 解决什么可观察问题？为什么要改？

## Change scope and authority / 改动边界与权威

- [ ] Codex / Git / GitLab-GitHub remain the source of truth; no duplicate Agent/Queue/Forge authority
- [ ] v1.4 complete-source LOC ratchet and upstream-convergence policy respected
- [ ] No unrelated feature expansion or destructive historical branch/tag changes

## Evidence / 测试证据

- [ ] Exact head SHA and changed files reviewed
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --locked --all-targets --all-features -- -D warnings`
- [ ] `cargo test --locked --all-targets --all-features`
- [ ] `python scripts/docs/check_docs.py` and release Python tests if docs/scripts changed
- [ ] Protected PR checks successful before merge (mark a command N/A only with rationale)

## Docs and safety / 中英文文档与安全

- [ ] English + 简体中文 active docs updated together, or no user-facing contract changed
- [ ] No credentials, internal repo paths, raw provider stderr, prompts or transcripts
- [ ] Failure modes, rollbacks, and unknown external write outcomes considered

## Release impact / 发布影响

- [ ] Stable publishing remains unauthorized (unless a separate explicit authorized release action)
- [ ] Real Codex/Internal GitLab/Windows→SSH Ubuntu TTY/admin gates are **not** claimed from synthetic CI
- [ ] Relevant issue/run/artifact references and remaining human work recorded

**Notes / 说明:** Include the exact source SHA and link CI runs; do not paste secrets.
