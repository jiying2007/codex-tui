<!-- docs-id: security -->
<!-- docs-lang: zh-CN -->
# 安全披露与数据保护政策
<!-- docs-section: overview -->

**Language / 语言:** [English](SECURITY.md) · [简体中文](SECURITY.zh-CN.md)

安全属于首次部署资格的一部分。本仓库未承诺付费漏洞奖励或固定响应 SLA。在维护者完成安全分级前，避免公开漏洞利用细节。

## 支持范围与历史代码
<!-- docs-section: scope -->

当前维护目标是 `main` 上的 v1.4 候选（尚未实际部署）。公开 v1.0.0 和 v1.1–v1.3 检查点仅用于历史审计，不宣称持续修补或在生产部署。安全补丁必须基于受保护主线精确 SHA 验证，不能自动当作 Stable 发布。

## 如何私下报告漏洞
<!-- docs-section: report -->

优先使用仓库 Security 页的 **Report a vulnerability** 私下通道（若已启用）。若未开放该通道，通过现有可信方式私下联系维护者；**切勿在公开 Issue、PR、讨论区或 CI 日志发布令牌、漏洞利用细节或内部数据**。报告仅需影响概述、受影响 SHA/平台/Provider、经过脱敏的最小复现和可行缓解措施；不得访问他人账号或数据收集证据。

## 敏感数据与诊断边界
<!-- docs-section: data -->

不得上传密钥、`auth.json`、环境变量、嵌入凭据的 Git URL、未经脱敏的 `git/gh/glab` stderr、Prompt/对话、评论正文。`doctor bundle` 和能力回执虽有隐私约束，分享前仍需人工复核。SQLite 备份、WAL/SHM 和内部 GitLab 标识亦需受控保管。测试使用合成负例，不使用真实秘密。

## 修复流程与披露
<!-- docs-section: response -->

维护者应确认最小可复现性、影响范围、外部权威边界、负例与回归测试，并通过受保护 PR/精确 SHA 的 CI 修复。披露时间按风险与影响协商，不承诺未经证实的响应时间、事件监控或远程自动撤销能力。

