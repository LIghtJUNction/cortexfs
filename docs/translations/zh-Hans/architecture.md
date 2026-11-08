# CortexFS 架构（历史归档）

[历史文档目录](README.md) · [English](../../architecture.md) · [当前范围](../../../PROJECT_SCOPE.md)

> **已退役设计：不要作为当前产品实现。** 本文记录截至
> [7261cb3](https://github.com/LIghtJUNction/cortexfs/tree/7261cb399ba1f8efa387736938ea3d8eec1a0acb)
> 的托管 Agent CLI 设计，仅为历史追溯保留。当前产品没有获批的替代架构。

## 历史职责划分

旧设计由 CortexFS 为现有 Agent CLI 提供 Linux 执行边界。CLI 作为普通进程运行，
其身份、可见路径、socket、网络和资源上限由 Agent 定义与宿主策略决定；
模型交互与对话行为由 CLI 自己管理。

| 组件 | 历史职责 |
| --- | --- |
| 托管 Agent CLI | 模型与 provider 选择、认证、工具循环、审批、会话、上下文与压缩 |
| CortexFS 执行边界 | 身份、挂载、socket、网络策略、资源与子进程生命周期 |
| FUSE /ctx | 可检查对象，以及逐路径的读、写和执行权限 |
| 前端与通道 | 通过已授权接口提供用户交互和传输 |

## 历史根 ABI

旧合同定义了八个根目录项：status、bin、model、agent、tool、channel、home 和 shared。
该 ABI 随旧设计一起退役，不构成下一代产品的兼容性要求。

## 历史实现边界

旧设计将 FUSE 投影、协议转换、执行和边缘适配器分层，并在适用处复用 MCP、
OAuth/OIDC、OpenTelemetry 与 JSON-RPC/SSE/WebSocket。模型认证、审批和对话行为
由托管 CLI 负责。

这些内容只记录旧设计，不是恢复它们的建议。来源见[历史内部架构](internal-architecture.md)
和 [ABI 归档](../spec/README.md)。

## 状态

当前仓库不含产品源码。这里记录的运行时、挂载、进程边界和适配器均未在当前 checkout
中重新构建或验证。新工作必须从获批的用户问题开始，而不是继续扩展本架构。
