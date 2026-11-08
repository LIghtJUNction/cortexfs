# CortexFS 架构

[文档目录](README.md) · [English](../../architecture.md)

> **适用范围：** 本文整理截至
> [`7261cb3`](https://github.com/LIghtJUNction/cortexfs/tree/7261cb399ba1f8efa387736938ea3d8eec1a0acb)
> 的托管 Agent CLI 设计。当前仓库保留文档与配方，产品源码已移除。
> [重建配方](../../../recipes/cortexfs/RECIPE.md) 使用较早的 `84f349c` 合同快照。

## 职责划分

CortexFS 为现有 Agent CLI 提供 Linux 执行边界。CLI 作为普通进程运行，
其身份、可见路径、socket、网络与资源上限由 Agent 定义和宿主策略决定；
模型交互与对话行为由 CLI 自己管理。

| 组件 | 职责 |
| --- | --- |
| 托管 Agent CLI | 模型与 provider 选择、认证、工具循环、审批、会话、上下文与压缩 |
| CortexFS 执行边界 | 身份、挂载、socket、网络策略、资源与子进程生命周期 |
| FUSE `/ctx` | 可检查的对象，以及逐路径的读、写、执行权限 |
| 前端与通道 | 通过已授权接口提供用户交互和传输 |

交互与无界面运行共用权限检查，I/O 可选 stdio 或 PTY。
[启动契约](internal-architecture.md#启动契约)统一描述输入与结果。

## 冻结根

根目录固定为八项，详细结构和兼容规则以英文 [root ABI](../../spec/root-abi.md) 为准。

| 路径 | 用途 |
| --- | --- |
| `/ctx/status` | 挂载状态 |
| `/ctx/bin/` | ABI 辅助命令 |
| `/ctx/model/` | 系统级模型可执行对象 |
| `/ctx/agent/` | 系统级 Agent 可执行对象 |
| `/ctx/tool/` | 系统级工具可执行对象 |
| `/ctx/channel/` | 通道驱动、工具与能力状态 |
| `/ctx/home/` | 按用户存放的对象与状态 |
| `/ctx/shared/` | 用户和 Agent 的共享文件 |

新增能力通过现有对象或外部适配器接入。技能、项目规则、提示词与 CLI 配置保持为
普通可见文件。`skill/`、`memory/`、`mcp/`、`workflow/`、`chan/`、`job/`、
`hook/`、`audit/`、`control/` 均不属于根 ABI。

## 权限模型

进程的操作范围取决于宿主暴露了什么资源、允许它怎样使用。
CortexFS 将 Linux 身份和 mode bits 与路径策略、已授权挂载和 socket、网络规则及
资源上限共同应用于进程。提示词和项目规则可以引导 CLI 的决策，但不增加权限。

`/ctx` 整体可读写，具体路径按权限收窄。已授权写入必须经过 FUSE 检查；
禁止可写挂载 backing storage 来绕过这层检查。

已授权的读写工作区遵循普通 Unix 语义，包含其中的 `.git` 目录。
显式策略可以收窄子路径。linked worktree 指向工作区外的 Git 元数据时，
需要单独授权和挂载。启动时的 FUSE 检查与网络隔离规则见
[文件系统与网络约束](internal-architecture.md#文件系统与网络约束)。

## 扩展点

每个适配器都应对应已有的进程或协议边界。

| 需求 | 接入方式 |
| --- | --- |
| 新 Agent CLI | 选择可执行文件与 argv，按需增加薄启动配置 |
| 工具服务器 | MCP 或可执行工具 ABI |
| 平台通道 | 进程隔离的传输适配器 |
| 项目指导 | 可见的规则与技能文件 |
| 新的硬权限 | Unix/FUSE 策略与明确的 ABI 审查 |
| 可观测性 | 边界事实与标准遥测 |

托管 CLI 保留原有认证、会话恢复、审批与协议行为。按接口需要复用 MCP、OAuth/OIDC、
OpenTelemetry 和外部 JSON-RPC/SSE/WebSocket 契约。`agent.sh` 保持薄客户端；
后端启动配置只适配命令行差异，不增加 provider registry 或第二套会话引擎。

## 快照状态

保留的设计将 Codex CLI、Claude Code、Pi 列为首批目标，Antigravity 为后续目标。
在该快照中，启动配置仍处于 [#318](https://github.com/LIghtJUNction/cortexfs/issues/318)
迁移阶段。

`7261cb3` 文档记录了以下进展：

| 记录 | 变更 |
| --- | --- |
| [#320](https://github.com/LIghtJUNction/cortexfs/pull/320) | 移除已授权读写工作区的隐式 `.git` 遮罩 |
| [#322](https://github.com/LIghtJUNction/cortexfs/pull/322)、[#325](https://github.com/LIghtJUNction/cortexfs/pull/325) | 复用 `ctxterm` 选择显式程序/argv，并让子进程一次性退出 |
| [#326](https://github.com/LIghtJUNction/cortexfs/pull/326)、[#328](https://github.com/LIghtJUNction/cortexfs/pull/328) | 将 `/ctx` 收敛为一个有效投影，向子进程转发已筛选的启动环境 |
| [#330](https://github.com/LIghtJUNction/cortexfs/pull/330) | 按策略与宿主条件门控托管命令的 pasta 网络出口 |

该快照的剩余工作包括最小可执行文件/配置投影、完整进程边界测试，以及在确认独立消费者后
移除旧运行时。旧 provider/model adapter、Agent SDK 与 object-runner 循环、上下文压缩、
会话和交互机制属于历史兼容内容，其源码已不在本 checkout 中。
产品运行与迁移完成状态需要单独验证。

## 继续阅读

- [英文规范](../../spec/README.md)：路径、格式与行为的权威定义。
- [内部架构](internal-architecture.md)：分层、启动约束与验证契约。
- [仓库说明](../../../AGENTS.md)：工作范围与贡献规则。
- [文档设计](../../DESIGN.md)：内容呈现与导航。
