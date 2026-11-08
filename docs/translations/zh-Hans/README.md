# 文档目录

[项目首页](../../../README.zh-CN.md) · [English](../../README.md) · [规范目录](spec/README.md)

CortexFS 用 Linux 文件系统接口描述 Agent 的执行边界。本仓库保存文档与重建配方，
产品和网站实现已移除。重建产品从配方开始，查阅已记录的行为从历史指南开始。

## 使用当前仓库

| 任务 | 入口 |
| --- | --- |
| 准备重建 | [配方](../../../recipes/cortexfs/RECIPE.md)、[环境](../../../recipes/cortexfs/ENVIRONMENT.md)、[构建任务](../../../recipes/cortexfs/BUILD_PROMPT.md) |
| 确定验证范围 | [验收](../../../recipes/cortexfs/ACCEPTANCE.md)、[性能](../../../recipes/cortexfs/PERFORMANCE.md)、[媒体](../../../recipes/cortexfs/ASSETS.md) |
| 查阅改进依据 | [研究记录](../../../recipes/cortexfs/RESEARCH.md)、[改进要求](../../../recipes/cortexfs/IMPROVEMENTS.md) |

## 理解设计

| 问题 | 阅读 |
| --- | --- |
| CortexFS 负责什么？ | [架构](architecture.md) |
| 进程与权限如何约束？ | [内部架构](internal-architecture.md) |
| 哪些路径和通信格式保持稳定？ | [ABI 规范目录](spec/README.md) |
| 如何命名路径和对象？ | [路径指南](paths.md)、[命名指南](naming-guide.md) |
| 文档如何排版与组织？ | [文档设计（英文）](../../DESIGN.md) |

## 历史指南

以下页面描述已移除实现的行为。命令需要对应版本的历史源码或独立构建的产品；
使用示例前，先阅读该页的适用范围。

| 任务 | 指南 |
| --- | --- |
| 安装与打包 | [安装](getting-started.md)、[Linux 软件包](packaging.md) |
| 操作运行时 | [日常使用](using-cortexfs.md)、[`ctx`](CTX.md)、[`agent.sh`](agent-sh.md) |
| 接入前端 | [通道](channels.md)、[Chat UI](chatui.md) |
| 扩展行为 | [开发](developing-cortexfs.md)、[扩展包](extensions.md) |
| 评估行为 | [评估](evaluation.md)、[Harness（英文）](../../harness.md)、[aimock](aimock-testing.md)、[FutureAGI（英文）](../../futureagi.md) |
| 查阅历史证据 | [Harness 报告（英文）](../../reports/2026-09-05-harness-validation.md)、[基准数据](../../benchmarks/20260714-agent-summary.json)、[Verus 证明范围](proofs/verus.md) |

## 版本与依据

| 来源 | 用途 |
| --- | --- |
| [`84f349c`](https://github.com/LIghtJUNction/cortexfs/tree/84f349c97bc2f6a611109686a02e21de3800042a) | 重建基线（`v0.1.21-10-g84f349c9`），以配方的[包内契约](../../../recipes/cortexfs/contracts/docs/README.md)为准。 |
| [`7261cb3`](https://github.com/LIghtJUNction/cortexfs/tree/7261cb399ba1f8efa387736938ea3d8eec1a0acb) | `docs/` 所保留的最后上游源码快照，包含后续托管 CLI 迁移。 |

`docs/` 内的详细[英文规范](../../spec/README.md)优先于摘要和较早译文。
当前仓库的工作规则见 [AGENTS.md](../../../AGENTS.md)。配方检查和历史报告分别有
自己的验证范围；产品的新结论需要新的执行证据。

本目录只链接真实中文译文。没有译文的页面直接链接英文原文，并标明语言。
