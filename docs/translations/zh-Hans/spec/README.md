# ABI 规范目录

[文档目录](../README.md) · [English](../../../spec/README.md) · [架构](../architecture.md)

英文规范保留至源码快照 `7261cb3`。中文详细译文更新进度不一，部分仍保留较早行为。
下表同时提供两者入口，查询后续契约时以英文为准。当前仓库没有产品实现。
重建较早版本请使用配方的[包内规范](../../../../recipes/cortexfs/contracts/docs/spec/README.md)，
来源与边界见[版本说明](../README.md#版本与依据)。

## 理解接口

ABI 是客户端能够观察和依赖的约定，包括路径、文件操作、权限和消息格式。
CortexFS 用 `<name>` 表示可执行对象，用 `<name>.d/` 保存控制文件，
用 `<name>.sock` 处理有状态交互。从根 ABI 与对象 ABI 开始阅读即可。

`/ctx` 根固定为八项：`status`、`bin/`、`model/`、`agent/`、`tool/`、
`channel/`、`home/`、`shared/`。Provider、协议和内部存储选择沿用现有接口。

贯穿这些契约的三条规则：

- Unix 身份、权限、挂载可见性和 CortexFS policy 限定有效权限；提示词、技能和项目规则文本不能增加权限。
- `/ctx` 整体可读写，各路径可以进一步收窄。禁止通过可写 backing-store bind 绕过 FUSE 的权限检查。
- 托管 CLI 设计把模型认证、对话状态和工具循环交给 CLI；CortexFS 负责进程与资源边界，细节见[内部架构](../internal-architecture.md)。

## 查找契约

| 英文规范 | 内容 | 中文参考 |
| --- | --- | --- |
| [Root ABI](../../../spec/root-abi.md) | 八项根、参考树与文件规则 | [译文](root-abi.md) |
| [Paths](../../../spec/paths.md) | 文件系统、socket 与主机路径 | [译文](paths.md) |
| [FUSE](../../../spec/fuse.md) | 文件系统投影与操作 | [译文](fuse.md) |
| [Objects](../../../spec/object-abi.md) | 可执行文件、socket 与控制目录 | [译文](object-abi.md) |
| [Models](../../../spec/model-abi.md) | 兼容模型对象与推理事件 | [译文](model-abi.md) |
| [Sessions](../../../spec/session-abi.md) | 持久会话事实与索引 | [译文](session-abi.md) |
| [Agent security](../../../spec/agent-tool-security.md) | 身份、可见性、挂载与创建 | [译文](agent-tool-security.md) |
| [Agent runtime](../../../spec/agent-runtime.md) | 旧运行时行为与迁移约束 | [译文](agent-runtime.md) |
| [Tools and policy](../../../spec/tool-policy-abi.md) | 工具、MCP、权限与审计 | [译文](tool-policy-abi.md) |
| [Terminals](../../../spec/terminal-abi.md) | 终端资源、PTY 与连接 | [译文](terminal-abi.md) |
| [Terminal broker](../../../spec/terminal-broker.md) | broker 认证与描述符授权 | 暂无译文 |
| [Interaction](../../../spec/interaction-abi.md) | 前端通信帧与兼容性 | [译文](interaction-abi.md) |
| [Channels](../../../spec/channel-abi.md) | 通道实例、路由与主机边界 | [译文](channel-abi.md) |
| [Modules](../../../spec/module-abi.md) | module API 与通信契约 | [译文](module-abi.md) |
| [ctx coreutils](../../../spec/ctx-coreutils.md) | 命令行为与输出 | [译文](ctx-coreutils.md) |
| [Rolling upgrades](../../../spec/rolling-upgrades.md) | 参考树更新与存储切换 | [译文](rolling-upgrades.md) |

## 兼容性与历史差异

旧的 model、tool、session 契约记录兼容义务。托管 CLI 的目标与历史迁移记录见
[架构](../architecture.md)，产品当前状态需另行执行验证。

阅读 Agent runtime 时注意已知差异：后续源码快照通过
[#320](https://github.com/LIghtJUNction/cortexfs/pull/320)移除了隐式 `.git` 只读叠挂。
已授权的可读写工作区遵循普通 Unix 子树权限；显式 policy 可以收窄 `.git`，
指向工作区外的 linked-worktree 元数据仍需单独授权。

保留已识别的历史差异，并始终区分配方的固定基线与后续迁移设计。
