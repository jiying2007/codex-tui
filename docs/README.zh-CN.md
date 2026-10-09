<!-- docs-id: docs-index -->
<!-- docs-lang: zh-CN -->
# 文档导航 — 当前指南与历史证据
<!-- docs-section: overview -->

**Language / 语言:** [English](README.md) · [简体中文](README.zh-CN.md)

**当前有效的使用与治理文档**提供英文、简体中文两个入口；历史实施、研究、版本检查点和其他 ADR 保留原语言作为证据，不得把历史记录理解为当前发布许可或实际部署事实。

## 从这里开始
<!-- docs-section: start -->

- [中文项目概览](../README.zh-CN.md) · [English overview](../README.md)
- [团队快速入门](zh-CN/team-quickstart.md) · [安装与首次部署](zh-CN/release/install-upgrade.md)
- [日常操作指南](zh-CN/guides/operator-guide.md) · [CLI 速查](zh-CN/guides/cli-reference.md) · [故障排查](zh-CN/guides/troubleshooting.md)

## 运行时权威与发布资格
<!-- docs-section: operators -->

- [GitLab/GitHub 资格](zh-CN/qualification/provider.md) · [首次部署数据决策](zh-CN/implementation/v1.4-first-deployment-baseline.md)
- [发布门禁、真实验收与授权](zh-CN/guides/release-qualification.md)
- [ADR-012：薄控制平面](zh-CN/adr/012-upstream-convergence.md)
- 历史发布审计：[原版安装/归档契约](release/install-upgrade.md)

## 仓库协作、安全与文档维护
<!-- docs-section: governance -->

- [中文贡献流程](../CONTRIBUTING.zh-CN.md) · [安全披露](../SECURITY.zh-CN.md)
- [支持与诊断](../SUPPORT.zh-CN.md) · [协作行为规范](../CODE_OF_CONDUCT.zh-CN.md)
- [双语维护约定](i18n/README.md) · [配对清单](i18n/manifest.json)
- [Bug Issue 模板](../.github/ISSUE_TEMPLATE/bug_report.yml) · [PR 审查表](../.github/pull_request_template.md)

## 历史工程证据
<!-- docs-section: archive -->

历次实施节点见 [implementation](implementation/m0-bootstrap.md)、设计方案见 [design](design/final-plan.md)、架构评审见 [reviews](reviews/2026-09-29-final-architecture-review.md)、研究记录见 [research](research/2026-09-29-long-term-maintainability.md)；旧版本的 [计划 JSON](../release/v1.3-plan.json) 和过往 ADR 均属历史资料，不逐页翻译以免造成两个相互漂移的历史副本。

当前**部署/生产事实**以精确 SHA 的真实证据为准，不能从历史路线图推断。

## 中英文同步规则
<!-- docs-section: translation -->

只有 [配对清单](i18n/manifest.json) 登记的**当前有效文档**要求两种语言同步。用户命令、安全约束和发布边界变更必须在同一 PR 同时更新两种语言。兼容 Python 3.8 的检查器验证章节键、语言切换、本地链接、UTF-8 和受限信息，不翻译 CLI 字面标识。

