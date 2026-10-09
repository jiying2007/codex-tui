<!-- docs-id: docs-policy -->
<!-- docs-lang: zh-CN -->
# 双语文档维护与防漂移规范
<!-- docs-section: overview -->

**Language / 语言:** [English](README.md) · [简体中文](README.zh-CN.md)

本规范约束 [配对清单](manifest.json) 中的**当前有效**英文/简体中文文档，不是第二套版本/发布权威，不要求翻译全部历史实验或 ADR。应优先保证用户可操作且事实准确，而不是逐字机械直译。

## 范围与权威
<!-- docs-section: scope -->

`manifest.json` 是唯一活跃双语配对表。每份文件必须声明正确语言、docs-id、双向链接和同一组语义章节键。既有英文架构、稳定资格 JSON、实施研究史料保持原路径；简体中文页面应准确说明同样的操作和安全边界。

## 修改与审阅要求
<!-- docs-section: changes -->

变更 CLI、配置、首次安装、发布权限、Forge 能力、SSH/TTY、隐私或安全要求时，必须在**同一个 PR**同时修改两种语言。自动脚本无法判断翻译含义是否准确，审查者需亲自对照；选项名、命令、版本/SHA 类型和 fail-closed 语义保持一致。

## 本地与 CI 检查
<!-- docs-section: ci -->

在仓库根目录执行：

~~~bash
python scripts/docs/check_docs.py
python -m unittest discover -s scripts/release -p "test_*.py"
~~~

脚本仅用 Python 3.8 标准库，检查配对、docs-id/语言/章节、本地 Markdown 路径、双向语言导航，且禁止遗漏当前新增的中文页。 Canonical CI 还会比较本次提交与检出提交的直接父版本：活跃英文或中文文档发生变化时，对应的另一语言必须在同一 PR/提交中变化。它只能检查文件成对修改，不能替代对翻译含义的人工审阅。负例检查丢失翻译、断链、缺章节和语言漂移，不访问网络、不把历史文档当成当前协议。

## 发行包与历史
<!-- docs-section: archive -->

从 v1.4 首次部署候选归档起，必须携带并校验清单内**全部当前双语文档**及原相对路径、根目录的简体安装/快速入门别名，并保留许可证与第三方 Notices。不能回写已公开历史发布；历史资料仍是历史证据。某历史页若升级为当前操作指南，先注册双语配对再提交。Stable 发布始终等待真实环境门禁和明确人工授权。

