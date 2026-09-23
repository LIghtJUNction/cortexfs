# CortexFS 内部架构

本文件配套 [architecture.md](architecture.md)，约束 Rust 代码、FUSE 和进程边界如何分层。目标是托管现有 Agent CLI，而不是在 CortexFS 内重建模型、工具、session 和 provider runtime。

## 分层原则

依赖方向必须保持向下：

```text
frontends / channel adapters
        |
        v
CLI launch / execution boundary
        |
        v
foundation object ABI + path/policy contracts
        |
        v
FUSE + support primitives
```

Foundation 只包含稳定对象 ABI、路径、普通数据和通用 support contract。FUSE 可以依赖 foundation object/path contract，但不得依赖 executor、launch profile 或 runtime orchestration；`fuse -> executor` 仍然禁止。

旧 `cortexfs-protocol`、provider/model adapter、Agent SDK loop、object-runner loop、compaction/session orchestration 与 interaction runtime 都是迁移中的兼容层，不是新功能的目标层。

## 托管 CLI 的启动输入

同一套 backend-neutral process contract 应覆盖 Codex CLI、Claude Code、Pi 和 Antigravity：

```text
program + argv
cwd + explicit env
stdio 或 PTY
uid + gid + supplementary groups
umask/mode policy
policy-authorized mounts + sockets
policy-derived network namespace / egress
resource/cgroup ceilings
exit status
signal / cancel / timeout
```

这些权限必须在 spawn 前由 Agent object 与 CortexFS policy 推导。直接继承操作者 uid、完整 home、ambient credentials 或不受限制的宿主网络，不符合契约。

CLI 自己拥有：model/provider 选择与认证、session/context、compaction、approval UX、tool loop、CLI-native MCP 与 provider-specific behavior。CortexFS 不把这些事件重新编码成第二套通用 Agent 协议。

## FUSE 与 `/ctx`

宿主 `/ctx` FUSE 整体为 RW，并通过 Unix mode/uid/gid 与 CortexFS policy 对具体路径收窄。不可变事实、派生状态或内核独占维护的路径可以单独只读。

当前旧 Agent sandbox 仍将 `/ctx` 投影为只读；这是 #318 的已知迁移缺口。修复必须让授权写入经过 FUSE，不能把 backing storage 以 writable bind 暴露进 sandbox。

## 进程、PTY 与取消

`ctxterm` 负责 PTY、attach/watch、子进程生命周期与 exit status，不负责 Agent intelligence。新的 hosted-CLI seam 应优先复用 `ctxterm` 的已有 child-process 机制。

测试应覆盖 exact argv、cwd/env、identity/groups、umask、mount/socket 可见性、network default-deny 与显式 egress、stdio/PT Y、exit status、signal/cancel/timeout 和无孤儿子进程。

## Git 与激活语义

Git commit 是唯一开发/配置激活事件。进程 restart 是普通 lifecycle：它可以重新消费已经提交的 generation，但不能让未提交配置成为权威状态。

RW workspace 使用正常 Unix 子树语义；`.git` 不再被 CortexFS 隐式遮罩。显式 path policy 可以收窄 `.git`，工作区外 linked-worktree metadata 仍需独立授权。

## Backend adapter 约束

Backend 差异只能位于薄 launch profile：可执行程序、argv 拼写、明确需要的 config/executable path、interactive/headless stdio/PT Y 选择。不得在 core 新增 backend enum、provider registry、model router、session manager、workflow engine 或私有 wire protocol。

Omarchy 是首要验证环境，但实现必须保持普通 Arch/Linux 语义。对 mise wrapper、XDG、systemd --user、Wayland terminal 与 FUSE 的支持应通过显式最小路径/环境投影实现，而不是信任整个用户环境。

## 删除优先

修改旧 runtime 前先确认是否有独立消费者。仅为旧自研 Agent loop 服务、且现成 CLI 已覆盖的 wrapper、parser、registry、state machine、provider branch 与 public concept，应按小步迁移优先删除。每一步都应有边界测试，并保持 root ABI 不变，除非有明确迁移规范。