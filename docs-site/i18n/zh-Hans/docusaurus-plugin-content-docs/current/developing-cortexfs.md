---
id: developing-cortexfs
title: 扩展 CortexFS
sidebar_label: 扩展 CortexFS
---

# 扩展 CortexFS

先记住一条：CortexFS 是 Unix/FUSE 执行基底，不是第二套 Agent 框架。扩展稳定的文件、socket、进程、policy、channel 与 tool 边界；不要新增 root namespace、provider-specific Agent 分支或常驻编排层。

建议先读：

```text
AGENTS.md
architecture.md
internal-architecture.md
spec/README.md
spec/root-abi.md
spec/object-abi.md
spec/agent-tool-security.md
spec/tool-policy-abi.md
spec/terminal-abi.md
spec/terminal-broker.md
extensions.md
```

稳定根只有：

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

不要新增 `provider`、`workflow`、`job`、`hook`、`mcp`、`skill`、`memory`、`chan`、`audit` 或 `control` 等平行根。

## 开发思维模型

CortexFS 拥有权限与执行边界。托管 Agent CLI 自己拥有 model/provider auth、Agent loop、session/context、compaction、approval UX 与 CLI-native tool 行为。

最小启动契约保持普通进程形态：

```text
executable + argv
cwd + explicit environment
stdin/stdout/stderr 或 PTY
uid + gid + supplementary groups
umask/mode policy
已授权 mounts + sockets
policy 推导的 network namespace / egress
resource ceilings
exit status
signals + cancellation
```

Codex CLI、Claude Code、Pi、Antigravity 已经提供 text/JSON/JSONL/RPC/resume/MCP 等能力时，应直接使用它们，不要重新归一化为 CortexFS 私有 wire protocol。

## `/ctx` 与文件系统权限

宿主 FUSE `/ctx` 整体 RW，具体文件和目录可按 Unix/FUSE policy 收窄为只读。禁止以 writable bind 暴露 backing storage 绕过 FUSE。

当前旧 Agent sandbox 仍把 `/ctx` 投影为只读，因此 Agent 可见 writable `/ctx` 是 #318 的迁移目标，而不是当前 launch profile 可以假设的能力。

RW `/workspace` 遵循普通 Unix 子树语义；CortexFS 不再默认隐藏 `.git`。显式 policy 可将 `/workspace/.git` 设为 RO；工作区外 linked-worktree metadata 需要独立授权。

## 托管 Agent CLI

首批目标是 OpenAI Codex CLI、Anthropic Claude Code、`earendil-works/pi`；Antigravity CLI 是当前 Google 路线扩展目标。#318 完成前，这些是目标集成，不表示所有 profile 已随当前 release 提供。

Backend adapter 通常只应该描述：

```text
program + argv 拼写
明确需要的环境变量
明确需要投影的 config/executable path
interactive/headless 的 stdio/PT Y 选择
```

它不能长成 model router、provider registry、session manager、Agent loop、auth broker 或 workflow engine。

Omarchy 是首要桌面/开发验证环境，但实现应保持通用 Arch/Linux：处理 XDG、mise wrapper、systemd --user、Wayland terminal、FUSE、uid/gid/groups、signal 与 PTY，不要直接信任整个 home、所有 `~/.local/bin` 或宿主环境。

## Tool 与 MCP

Tool 是可执行能力端点。有效权限是路径可见性、Linux identity/mode、mount/path policy 与 CortexFS policy 的交集。

外部 tool server 已适合 MCP 时直接使用 MCP；`ctxmcp` 只是 adapter，不是 `/ctx/mcp` 根。

异步工具若需要持久结果，继续沿用 Unix 风格原子提交：先写临时文件，再在同目录 rename 为已提交请求，然后读结果并按 ABI 追加 audit 事实。

Prompt、AGENTS.md、CLAUDE.md、skills 和 schema 可以影响 CLI 行为，但永远不能提升权限。

## Agent object 与启动

Agent object 是受 policy 约束的执行 principal，不是 CortexFS-owned AI loop。兼容路径仍包括：

```text
/ctx/agent/<name>
/ctx/agent/<name>.sock
/ctx/agent/<name>.d/
/ctx/home/<uid>/agent/<name>/session/
```

其中一部分仍服务旧 runtime，直到迁移将其删除或收窄。

当前交互式进程链路复用：

```text
systemd-run --user
bwrap sandbox
ctxterm
child process
```

`ctxterm` 负责 PTY、attach/watch、child lifecycle 与 exit status，不是 Agent runtime。#318 Phase 2 应复用它，仅增加最小显式 child program/argv seam，不新增 runner 或 backend registry。

`tsh` 继续作为兼容 tool shell；新的 hosted CLI 集成不应要求 CortexFS 把 CLI 自己的 model/session/tool loop 吞进 `tsh`。

## Session 与 context

CortexFS 可以持久化自己真正拥有的事实：process receipt/status、terminal replay、policy/audit、object definition，以及 ABI 消费者仍需要的兼容 session 数据。

托管 CLI 对自己的 session/context 格式保持权威。不要为了统一外观，把每个 backend 的 conversation、model event 或 tool event 都复制进新的通用 CortexFS session schema。

旧 `messages.jsonl`、`events.jsonl`、context pack 等兼容文件可在真实消费者仍存在时保留；消费者消失后优先收窄或删除。

## Provider 与认证

Provider/model adapter、provider registry、CortexFS auth profile 与 `cortexfs-protocol` 是旧消费者的兼容代码，不是新 hosted CLI 的目标所有权模型。

Codex、Claude Code、Pi、Antigravity 及未来 CLI 应优先使用各自支持的 auth/provider 行为。只有明确集成需求无法通过 CLI 或 Unix/开放协议安全表达时，才新增 CortexFS 边界。

不要为了启动 CLI 把 secret 放进 `/ctx`、宽泛继承的环境变量或日志。

## 扩展包

[单文件扩展](extensions.md) 仍是现有 object/tool 兼容面的 authoring convenience。安装继续遵守校验、hash binding 与原子发布；package 不能变成第二套 `/ctx` namespace 或 hot-reload plugin control plane。

旧 executable Agent SDK package 是兼容面。若外部 hosted CLI 已经拥有相同 Agent-loop 行为，不要继续在 Agent SDK 中新增相同功能。

## 验证

常用检查：

```bash
scripts/test.sh cargo test --workspace
npm --prefix docs-site run build
```

进程边界改动优先使用 fake executable，验证 exact argv、cwd/env、identity/groups、umask、mount/socket、network default-deny/authorized egress、stdio/PT Y、exit status、signal/cancel/timeout 与无孤儿 child。

只有官方 CLI 在环境里真实可用且不需要暴露敏感凭据时，才做 live smoke test，并明确区分 fixture 与 live evidence。

## 常见错误

- 不要重造第二套 Agent runtime。
- 不要继承操作者完整权限、home、credential 或 host network。
- 不要用 writable backing bind 绕过 FUSE。
- Git commit 是唯一开发/配置激活边界；restart 只是生命周期动作。
- Core 不增加 backend enum；差异留在薄 launch profile。
- 禁止 `mod.rs` 与 `unsafe`；遵守 `AGENTS.md` 的分层、命名和 source-budget。
- FUSE 只能依赖 foundation object/path ABI，不能依赖 executor 或 runtime orchestration。