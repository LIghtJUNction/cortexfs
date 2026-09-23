# CortexFS 架构

规范 ABI 细节位于 [spec/](spec/)。本文档定义新的工程边界：CortexFS 是承载现有 Agent CLI 的 Unix/FUSE 执行基底，而不是第二套 Agent 框架。

## 一页模型

```text
/ctx 是整体可读写的 FUSE 文件系统，具体路径可按 Unix/FUSE policy 收窄为只读
Agent 对象定义身份、权限、可见路径与进程启动边界
Codex CLI / Claude Code / Pi 等托管 CLI 自己拥有模型、认证、session/context、approval 与 tool loop
CortexFS 拥有进程、身份、挂载、网络、资源、socket、路径与 FUSE 权限上限
MCP 等已有开放协议直接复用，不再复制一套 CortexFS 私有协议
```

首批目标托管 CLI 是 OpenAI Codex CLI、Anthropic Claude Code 与 `earendil-works/pi`；Antigravity CLI 是当前 Google 路线的扩展目标。issue #318 完成之前，“目标”不等于所有 launch profile 已经实现。

## 冻结根

稳定根保持：

```text
/ctx/status
/ctx/bin
/ctx/model
/ctx/agent
/ctx/tool
/ctx/channel
/ctx/home
/ctx/shared
```

不得新增 `skill/`、`memory/`、`mcp/`、`workflow/`、`chan/`、`job/`、`hook/`、`audit/` 或 `control/` 等平行根。`channel/` 是唯一通信根。

## 权限与进程边界

权限来自 Linux 与 CortexFS policy，不来自 prompt：

```text
Agent 定义
  -> uid/gid/补充组
  -> mode/umask 与路径 policy
  -> 已授权 mount/socket
  -> policy 推导的 network namespace / egress 权限
  -> cgroup/资源上限
  -> child process
```

后端无关的最小启动契约是：

```text
executable + argv
cwd + environment
stdin/stdout/stderr 或 PTY
uid + gid + supplementary groups
umask/mode policy
已授权 mounts/sockets
policy 推导的 network namespace / egress
资源上限
exit status
signal + cancellation
```

托管 CLI 之间的差异只应留在薄 launch profile / adapter。核心不应出现 provider registry、backend enum、model router、session manager 或第二套 workflow engine。

`/ctx` 宿主 FUSE 整体为 RW；单个文件或目录可以只读。禁止把 backing storage 以可写 bind 暴露给 Agent 来绕过 FUSE。当前旧 Agent sandbox 仍把 `/ctx` 投影为只读，因此 Agent 可写 `/ctx` 仍是 #318 的迁移目标。

RW `/workspace` 遵循正常 Unix 子树语义；CortexFS 不再隐式隐藏 `.git`。显式 policy 仍可把 `/workspace/.git` 收窄为只读；linked worktree 指向工作区外部的 gitdir 需要独立授权。

## 进程形态

```text
user / frontend
      |
      v
ctx launch / supervisor
      |
      +-- 应用 identity / policy / mounts / network / resources
      +-- 需要时通过 ctxterm 提供 stdio/PTTY
      v
Codex | Claude Code | Pi | Antigravity | 其他兼容 CLI
```

`ctxterm` 只是 PTY/子进程机制，不是 Agent runtime。`ctxmcp` 是显式 MCP adapter，不是 `/ctx/mcp` 根。

## Omarchy / Linux

Omarchy 是首要桌面与开发验证环境，目前以 `omacom/omarchy` 为准；实现仍应保持通用 Arch/Linux。重点兼容 PATH/命令发现、XDG、mise wrapper、systemd --user、Wayland terminal、FUSE、uid/gid/groups、home/workspace 可见性、signal 与 PTY。不要为了 Omarchy 直接信任整个 `$HOME`、全部 `~/.local/bin` 或宿主所有环境变量。

## 旧 runtime 的迁移

仓库仍包含旧的自研 Agent runtime：`cortexfs-protocol`、provider/model adapter、Agent SDK loop、object-runner loop、compaction/session orchestration 与 interaction runtime。它们只作为兼容层保留，直到真实消费者消失。

新功能不得继续扩大旧 loop；优先删除只服务旧 runtime 的 wrapper、registry、parser、state machine 与重复抽象。稳定 root/path ABI 默认保持兼容。

Git commit 是唯一开发/配置激活边界。进程 restart 只是生命周期动作，不能让未提交配置成为权威状态。

## 迁移顺序

1. #320 已移除 RW workspace 上的隐式 `.git` 遮罩。
2. 复用现有 `ctxterm` 子进程/PTY 机制，不新增 runner。
3. 增加最小显式 executable/argv 选择入口。
4. 让被授权的 Agent 写入通过 FUSE 抵达 `/ctx`，禁止 writable backing bind。
5. 仅在 CLI 命令行差异确有必要时增加薄 launch profile。
6. 随真实消费者消失逐步删除旧 provider/model/tool/session runtime。

规范以 [spec/](spec/) 为准；开发约束以仓库 `AGENTS.md` 为准；Rust/进程分层见 [internal-architecture.md](internal-architecture.md)。