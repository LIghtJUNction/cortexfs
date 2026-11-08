# CortexFS 内部架构

[文档目录](README.md) · [架构](architecture.md) · [English](../../internal-architecture.md)

> **适用范围：** 本文整理截至
> [`7261cb3`](https://github.com/LIghtJUNction/cortexfs/tree/7261cb399ba1f8efa387736938ea3d8eec1a0acb)
> 的工程契约。当前 checkout 不含产品源码与测试。本文描述托管 CLI 迁移，
> 较早的[重建配方](../../../recipes/cortexfs/RECIPE.md)使用独立固定基线。

[架构入口](architecture.md)说明职责与快照状态；本文规定代码分层和边界约束。
详细 ABI 以[英文规范](../../spec/README.md)为准。

## 分层原则

依赖从组合与执行层指向基础 ABI 契约。

| 层 | 职责与允许的依赖 |
| --- | --- |
| Foundation | 稳定路径/对象类型、普通数据、模块契约及文件系统、JSONL、布局、进程辅助；不导入 FUSE、runtime 或 binaries |
| FUSE | 基于基础对象/路径/support 契约完成投影与检查；不导入 executor、启动编排或 CLI |
| 协议转换 | 在已有外部边界转换数据；不导入 Agent/会话编排 |
| 执行 | 策略、启动、回执、资源、网络、PTY 与兼容运行时；依赖 foundation 和 FUSE 公开边界 |
| Binaries 与边缘适配器 | 组合库；通道适配器止于中立 socket/runtime-client 接口 |

库不得导入 `bin/*`。启动配置可依赖通用进程/ABI 类型，不得依赖其他后端配置。
保留的 Rust 约束禁止 `mod.rs`、`unsafe` 和新增库级 `Result<_, String>` API。

## 进程职责

启动器先推导权限，再创建子进程。文件系统检查、传输和 Agent 行为各有明确归属。

| 进程 | 职责 |
| --- | --- |
| `cortexfs-mount` | FUSE 投影与文件系统权限检查 |
| `ctx` | CLI 控制入口与启动请求 |
| `ctxterm` | PTY、子进程生命周期与 attach/watch |
| Root terminal broker | 认证后授予文件描述符，不转发 PTY 字节流 |
| `ctxmcp` | 显式 MCP 适配 |
| Channel adapter | 单个外部平台的传输 |
| 托管 Agent CLI | 原生模型、认证、工具循环、审批与会话行为 |

FUSE、PTY 和传输代码不承担 Agent 编排。独立消费者与稳定 API 义务消失后，
再审查移除兼容运行时依赖。

## 启动契约

交互与无界面执行共用以下进程契约：

| 类别 | 必需输入或结果 |
| --- | --- |
| 调用 | 可执行文件、精确 argv、cwd 与显式环境 |
| I/O | stdin/stdout/stderr 或 PTY |
| 身份 | UID、GID、补充组与 umask/有效 mode policy |
| 可访问资源 | 已授权挂载与 socket；由策略推导的网络命名空间和出口 |
| 上限 | 资源/cgroup 限额 |
| 生命周期 | 子进程退出状态、信号与取消 |

**spawn 前**必须根据 Agent 对象和有效 CortexFS 策略推导身份、资源访问与网络权限。
不得默认继承操作者身份、凭据或无限制网络。后端参数拼写由使用同一契约的薄启动配置处理。

## 文件系统与网络约束

`/ctx` 整体可读写，Unix mode、身份、挂载语义与策略逐路径收窄访问。
已授权写入必须经过 FUSE 或其他明确授权的 Unix 对象。禁止可写挂载 backing
generation/storage 树绕过 FUSE。

显式托管命令使用规范 `/ctx` 根时，每次启动都要确认有效挂载是读写 CortexFS FUSE，
随后才暴露一个可写投影。保留的迁移契约中，无命令的 `/ctx/bin/tsh` 启动与
非规范/自定义根仍为只读。详见[运行时契约](../../spec/agent-runtime.md)。

显式托管命令保留 `--unshare-net`，直到三个条件同时满足：策略允许
`network:default connect`、`/usr/bin/pasta` 可执行、`/dev/net/tun` 可用。
权限被拒或前提缺失时继续隔离；旧的无命令 `tsh` 路径不启用 pasta。
完整传输规则见[网络策略 ABI](../../spec/tool-policy-abi.md)。

读写工作区保持 `.git`、`.jj` 与编辑器元数据的普通语义；显式策略可收窄任意子路径。
工作区外的 linked-worktree 元数据需要单独授权和挂载。

原子状态更新使用同目录临时文件加 rename，并遵守对应 ABI 的持久化要求。
保持单一状态发布机制。

## 环境与 home

启动边界通过 bwrap `--clearenv` 和 `--setenv` 构造允许的环境，`ctxterm` 将其转发给
子进程。只传入 CLI 与策略需要的变量和路径；默认投影不包含宿主偶然存在的 provider
密钥、完整用户 home 或全部 `~/.local/bin`。

保留 XDG config/data/cache 语义，通过显式可执行文件/配置路径解析 mise 等 wrapper。
Provider 凭据仍由托管 CLI 的认证流程管理，不复制到 CortexFS 密钥或对话状态中。

快照以 Omarchy 为主要验证目标，兼容工作仍遵循普通 Arch/Linux 语义：命令发现、
XDG、`systemd --user`、Wayland 终端环境、FUSE 权限、身份/组以及 PTY/信号。
其他 Linux 环境使用同样的最小投影原则。

## 进程生命周期与激活

保留的设计复用 `ctxterm` 的 PTY 和子进程机制。交互与无界面模式共用权限，
仅 I/O 不同。取消必须到达所属子进程/进程组，不留下孤儿，并记录边界结果。
保留子进程的普通退出状态。

Git commit 是唯一开发/配置激活边界。Restart 可使用已提交 generation，
不得激活未提交改动。不要新增后台 watcher、反向连接控制面或 hot-reload manager
作为另一条激活路径。

## 持久事实与适配器

CortexFS 持久化自己拥有的事实：Agent/对象定义、有效策略、启动回执、进程状态与退出码、
终端元数据/回放、审计事实，以及稳定兼容 API 要求的会话状态。
托管 CLI 保留原生对话格式和会话恢复行为。

在边界记录进程启动/退出、信号、挂载或身份拒绝、资源失败、PTY 连接与 FUSE 拒绝。
统一所有后端模型/工具事件的 schema 需要明确的外部消费者。
适配器将已有外部契约映射到 CortexFS 原语，协议选择见[扩展点](architecture.md#扩展点)。

## 错误策略

错误明确指出失败边界：路径/布局、身份/策略、挂载、网络/命名空间、spawn、PTY/broker、
资源限额、信号/取消或外部适配器/协议。Rust 库使用实现 `Display` 与 `Error` 的窄枚举，
保留底层错误；binary 可加 `ExecError` 一类封装。不同后端的同类 Unix 失败共用表示。

## 验证契约

使用确定性假可执行程序与边界测试检查以下行为：

| 边界 | 必须观察到的结果 |
| --- | --- |
| 调用与环境 | 精确 argv/cwd/env；仅已授权变量和路径进入子进程 |
| 身份与限额 | 正确 UID/GID/组、umask/mode policy 与资源上限 |
| Stdio 与 PTY | 两种传输都保留子进程退出状态 |
| 取消 | 信号到达所属进程组，且无孤儿子进程 |
| 挂载与 FUSE | 未授权挂载默认拒绝；规范 `/ctx` 每次启动均需确认读写 FUSE 才能写入 |
| 路径权限 | 写入服从逐路径策略；读写 `.git` 正常；外部 worktree 元数据单独授权 |
| 网络 | 策略拒绝或前提缺失时保持隔离；允许的出口仍受策略约束 |
| 适配器 | 符合 MCP/开放协议，核心不新增按后端区分的分支 |

已安装 CLI 且无需私有凭据时，可做真实 CLI 冒烟测试，补充确定性契约测试。
当前 checkout 不包含执行这些测试所需的运行时或测试运行器。

## 迁移纪律

每个实现切片先检查已有 issue/helper、独立消费者、公开 API 与稳定 ABI 义务。
托管 CLI 已拥有的行为优先用 Unix 直接组合或删除重复实现。
条件允许时只推进一个切片，保持 root/path 兼容，增加边界回归测试，并记录概念数量变化。

[快照状态](architecture.md#快照状态)汇总保留文档记录的已完成和待办工作。
新增 manager、registry、provider 分支或私有协议，需要已有原语无法表达的具体需求。
