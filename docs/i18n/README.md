<!-- docs-id: docs-policy -->
<!-- docs-lang: en -->
# Bilingual documentation maintenance policy
<!-- docs-section: overview -->

**Language / 语言:** [English](README.md) · [简体中文](README.zh-CN.md)

This policy governs **active** English/Simplified Chinese pages listed in [manifest.json](manifest.json). It is not another version/release authority and does not require translating every historical experiment or ADR. Keeping content useful and truthful takes precedence over word-for-word mirroring.

## Scope and ownership
<!-- docs-section: scope -->

The manifest is the only active-pair list. A page must have its declared language, matching docs-id, counterpart link and exactly the listed semantic section keys. The canonical English architecture, release-criteria and implementation history remains where it originally lived. Paired Chinese pages explain the same operational and security constraints in idiomatic Chinese.

## Change discipline
<!-- docs-section: changes -->

For changes to CLI syntax, settings, first-deployment state, release permissions, Forge capabilities, SSH/TTY behavior, privacy or security, edit **both** active languages in one PR. The reviewer must assess technical equivalence manually: an automated section/link checker cannot certify translation quality. Preserve literal flags, command names, version/SHA types and fail-closed semantics.

## Local and CI checks
<!-- docs-section: ci -->

From the repository root, run:

~~~bash
python scripts/docs/check_docs.py
python -m unittest discover -s scripts/release -p "test_*.py"
~~~

The checker is standard-library Python 3.8 compatible and verifies exact pairs, docs-id/locale/section structure, reciprocal language links, local Markdown targets and no untracked current Chinese pages. Canonical CI compares each PR to its exact GitHub event Base SHA (or a main push to its prior SHA), fetching only that missing commit if shallow checkout omitted it. When an active English or Chinese page changes, its registered counterpart must change in the same PR/commit. This is a *changed-file* check, not automatic translation quality verification. Negative tests exercise missing pairs, broken links, missing topics and language drift. It does **not** fetch websites or reinterpret historical markdown.

## Packaging and history
<!-- docs-section: archive -->

Starting with the v1.4 first-deployment candidate, release archives must retain and validate all manifest-listed English/Simplified Chinese current guides, their canonical paths and localized top-level install/quickstart aliases, as well as license/notices. Newly introduced bilingual assets are required during native archive verification; neither old public releases nor retained historical versions are retroactively modified. If a historical document is made active again, first register it as a paired current page. Stable remains non-publishing until all external release gates and explicit authorization are satisfied.

