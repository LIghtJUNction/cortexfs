# CortexFS

[English](README.en.md)

本仓库现只保留文档与 ABI 参考。产品源码、测试、示例、构建/安装代码、评估运行器和
文档站点实现已从跟踪树中移除。

## 重建提示词

重建包位于 [`cortexfs-prompts` 子模块](docs/cortexfs-prompts/README.md)，入口是
[`BUILD_PROMPT.md`](docs/cortexfs-prompts/BUILD_PROMPT.md)。它提取自任务开始时的干净本地
checkout：`84f349c97bc2f6a611109686a02e21de3800042a`（`v0.1.21-10-g84f349c9`），描述的是
该版本；尚未通过独立重建验证。

## 文档版本

当前 `docs/` 保留的 canonical 文档已更新到上游提交
`7261cb399ba1f8efa387736938ea3d8eec1a0acb`（2026-09-30），描述了更新后的 hosted Agent CLI
方向，与提示词包提取的源码快照不同。重建 `84f349c` 时以提示词包内的规范快照为准；了解
后续文档和迁移状态时查看 [`docs/spec/README.md`](docs/spec/README.md)。

详细根 ABI 定义 8 个稳定条目，包括 `/ctx/channel/`。例如
[`docs/evaluation.md`](docs/evaluation.md) 的历史指南可能引用已从跟踪树删除的工具。
