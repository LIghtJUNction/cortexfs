# ABI 规范归档（历史）

[历史文档目录](../README.md) · [English](../../../spec/README.md) · [当前范围](../../../../PROJECT_SCOPE.md)

> **已退役合同。** 本目录描述截至源码快照 7261cb3 的旧 CortexFS FUSE /ctx 和 Agent
> runtime 设计，仅为历史追溯保留。它们不是当前接口，也不构成兼容性要求。
> 早期 84f349c 重建快照有独立的包内规范。

## 旧 ABI 描述的内容

旧 ABI 定义路径、文件操作、权限、可执行对象、控制目录、socket 和消息格式。
其 /ctx 根包含 status、bin、model、agent、tool、channel、home 和 shared 八项。

旧设计把模型认证、对话状态和工具循环交给托管 CLI，把进程/资源边界和 FUSE 投影交给
CortexFS。这些职责均不是下一代产品的已批准要求。

## 历史合同索引

| 历史文档 | 原有主题 |
| --- | --- |
| [Root ABI](../../../spec/root-abi.md) | 八项根目录与文件规则 |
| [Paths](../../../spec/paths.md) | 文件系统/socket 名称与主机路径 |
| [FUSE](../../../spec/fuse.md) | 文件系统投影与操作 |
| [Objects](../../../spec/object-abi.md) | 可执行对象、socket 和控制目录 |
| [Models](../../../spec/model-abi.md) | 模型对象与推理事件 |
| [Sessions](../../../spec/session-abi.md) | 持久会话事实与索引 |
| [Agent security](../../../spec/agent-tool-security.md) | 身份、可见性、挂载与创建 |
| [Agent runtime](../../../spec/agent-runtime.md) | 旧运行时行为 |
| [Tools and policy](../../../spec/tool-policy-abi.md) | 工具、MCP 投影、权限、共享状态与审计 |
| [Terminals](../../../spec/terminal-abi.md)、[Terminal broker](../../../spec/terminal-broker.md) | PTY 事件、连接和文件描述符授权 |
| [Interaction](../../../spec/interaction-abi.md) | 前端/运行时帧 |
| [Channels](../../../spec/channel-abi.md) | 通道实例与路由 |
| [Modules](../../../spec/module-abi.md) | 模块 API 与外部 wire 合同 |
| [ctx coreutils](../../../spec/ctx-coreutils.md) | 旧命令行为与输出 |
| [Rolling upgrades](../../../spec/rolling-upgrades.md) | 参考树和存储代际更新 |

不要为了假设中的产品扩展这些接口。未来任务若确实需要协议，应为该任务编写新的最小合同。

## 历史参考

- [MCP 规范](https://modelcontextprotocol.io/specification/)
- [Linux FUSE 文档](https://www.kernel.org/doc/html/latest/filesystems/fuse/fuse.html)
