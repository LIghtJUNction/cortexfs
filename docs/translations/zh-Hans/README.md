# 历史文档归档

[项目首页](../../../README.zh-CN.md) · [English](../../README.md) · [当前范围](../../../PROJECT_SCOPE.md)

**本目录中的内容全部描述已经退役的 CortexFS 设计或历史源码快照。** 保留它们是为了追溯历史，不代表它们仍是当前产品架构、路线图或实现合同。

旧设计曾把 Agent 运行时、Linux FUSE /ctx 接口、进程与网络边界、模型和会话兼容层、消息通道及 MCP 适配组合在一起。该设计已经退出当前范围。当前仓库不包含可构建的产品实现。

## 归档索引

| 历史主题 | 资料 |
| --- | --- |
| 产品职责与旧架构 | [架构](architecture.md)、[内部架构](internal-architecture.md) |
| 旧路径和通信格式 | [ABI 规范](spec/README.md) |
| 安装和打包 | [安装](getting-started.md)、[打包](packaging.md) |
| 运行时与命令行 | [使用说明](using-cortexfs.md)、[ctx](CTX.md)、[agent.sh](agent-sh.md) |
| 消息通道与用户界面 | [Channels](channels.md)、[Chat UI](chatui.md) |
| 扩展与开发 | [开发](developing-cortexfs.md)、[扩展](extensions.md) |
| 评估和历史证据 | [评估](evaluation.md)、[Harness](../../harness.md)、[历史报告](../../reports/2026-09-05-harness-validation.md)、[基准数据](../../benchmarks/20260714-agent-summary.json) |
| 文档呈现 | [设计指南（英文）](../../DESIGN.md) |

## 历史快照

| 来源 | 历史含义 |
| --- | --- |
| [84f349c](https://github.com/LIghtJUNction/cortexfs/tree/84f349c97bc2f6a611109686a02e21de3800042a) | 由旧重建配方描述的早期产品快照。 |
| [7261cb3](https://github.com/LIghtJUNction/cortexfs/tree/7261cb399ba1f8efa387736938ea3d8eec1a0acb) | 后期文档快照，包含托管 Agent CLI 迁移。 |

不要混用这些快照，也不要把它们当作未来产品的设计规范。新设计必须从获批的用户问题和 [PROJECT_SCOPE.md](../../../PROJECT_SCOPE.md) 中的最小范围重新开始。