# CortexFS 规范

本目录是 CortexFS ABI 的规范入口。CortexFS 向现成 Agent CLI 暴露小型 Linux 文件系统与执行边界；路径是 ABI，可执行对象仍是文件，控制状态放在 `<name>.d/`，需要状态交互时使用 `<name>.sock`。

稳定根：

```text
/ctx/
  status
  bin/
  model/
  agent/
  tool/
  channel/
  home/
  shared/
```

核心原则：

```text
root 已冻结，只包含稳定对象分类
/ctx 整体可读写，单独路径可按 Unix/FUSE policy 只读
Agent CLI 自己拥有 model/provider auth、session/context、approval 与 tool loop
CortexFS policy 是硬权限上限
CortexFS 拥有进程、路径、挂载、身份、网络、policy 与 FUSE 边界
agent 对象描述 principal 与可执行边界，不是第二套 AI runtime
tool 是可执行能力端点
prompt / skill / project rule 永不授予权限
provider/API 格式细节不进入 root ABI
禁止 writable backing bind 绕过 FUSE
```

后端无关执行契约：

```text
executable/argv + cwd + env + stdio/PTY
uid + gid + supplementary groups + umask/mode policy
授权 mounts/sockets
policy 推导的 network namespace / egress
有界资源 + exit status + signal/cancellation
```

身份、mount/socket 与 network authority 都必须由 Agent object 与 CortexFS policy 在 spawn 前推导。Backend-specific 参数只留在薄 launch profile / adapter。

Codex CLI、Claude Code 与 Pi 是首批目标托管 CLI；Antigravity CLI 是当前 Google 路线扩展目标。MCP 等开放协议应直接复用，不要再包装一套新的 CortexFS wire protocol。

## 当前实现状态

```text
目标不等于已经实现：通用 hosted-CLI launch profile 仍在 #318 迁移
当前 executable agent 仍可能使用旧 sdk-envelope-v1 runtime
宿主 FUSE `/ctx` 整体 RW，但当前旧 Agent sandbox 仍把 `/ctx` 投影为 RO
Agent 可见的 writable `/ctx` 仍是迁移目标，授权写入必须经过 FUSE
```

旧 CortexFS model/tool/session/provider runtime 只在兼容需要时保留。新工作不得继续扩大与托管 CLI 重复的 provider/model/tool-loop/session authority。

以下不是根 ABI：

```text
provider registry
API format registry
database / vector database backend
workflow/job/hook DSL
spawn/factory/agent-template root
cluster scheduler DSL
MCP registry root
skill registry root
平行通信别名根，例如 chan/
audit root
control root
```

规范文件：

```text
root-abi.md             冻结 /ctx 根与基础文件规则
fuse.md                 FUSE 投影
object-abi.md           executable/socket/.d 对象三元组
model-abi.md            兼容 model object ABI 与事件流
session-abi.md          持久 session 兼容事实
agent-tool-security.md  Agent identity/view/mount/creation
agent-runtime.md        旧 hosted-runtime 兼容与迁移边界
module-abi.md           静态 module API 与外部 wire contract
terminal-abi.md         terminal/PTY/attach
terminal-broker.md      root broker auth 与 descriptor grant
tool-policy-abi.md      tool/MCP/shared/policy/logs
ctx-coreutils.md        ctx 命令契约
rolling-upgrades.md     reference-tree 更新与 storage switch
channel-abi.md          channel/host 边界
interaction-abi.md      兼容 interaction frame
paths.md                公共路径常量与 ABI
```

`agent-runtime.md` 记录的是当前兼容实现，不是新 Agent 功能的目标架构。#320 已移除默认 RW workspace 的隐式 `.git` 只读遮罩；其中仍保留的旧描述属于迁移清单，不是必须保留的不变量。

工程入口见 [../architecture.md](../architecture.md) 与 [../internal-architecture.md](../internal-architecture.md)。

## 外部参考

- [Model Context Protocol](https://modelcontextprotocol.io/specification/)
- [Linux FUSE 文档](https://www.kernel.org/doc/html/latest/filesystems/fuse/fuse.html)