---
id: getting-started
title: 从安装开始
sidebar_label: 从安装开始
---

> 历史操作参考 · 文档截至 [`7261cb3`](https://github.com/LIghtJUNction/cortexfs/commit/7261cb399ba1f8efa387736938ea3d8eec1a0acb)。
> 产品命令需要对应的历史源码或已安装版本；当前检出只包含文档与配方。
> [文档目录](README.md) · [重建任务](../../../recipes/cortexfs/BUILD_PROMPT.md)

# 从安装开始

本文保留历史版本的安装与首次检查流程。使用当前仓库时，请从
[重建配方](../../../recipes/cortexfs/RECIPE.md) 开始。

## 安装

源码安装器面向使用 systemd、且发行版仓库提供所需依赖的 Arch、Debian/Ubuntu、
Fedora/RHEL 与 openSUSE/SLES。它检查 FUSE、bubblewrap 0.10+、Rust 1.91+，
再按 `Cargo.lock` 构建。安装器及其下载的源码都固定到本文快照：

```bash
cortexfs_ref=7261cb399ba1f8efa387736938ea3d8eec1a0acb
curl -fsSL "https://raw.githubusercontent.com/LIghtJUNction/cortexfs/$cortexfs_ref/scripts/install.sh" \
  | CORTEXFS_REF="$cortexfs_ref" sh
```

持久变更需要输入指定文字确认。重复运行会更新程序和单元，保留存储、密钥、
provider 配置、环境文件及 `/ctx` 用户状态。首次引导可通过已有 provider 命令
配置模型服务。

用 `ctx doctor` 检查挂载、基础目录、默认模型别名及运行时状态；`ctx --help`
列出已安装版本的命令。原生包见 [Linux 安装包](packaging.md)，IM 接入见
[多平台通道](channels.md)。

## 更新主机

先检查计划，再应用同一历史源码修订：

```bash
cortexfs_ref=7261cb399ba1f8efa387736938ea3d8eec1a0acb
ctx update --ref "$cortexfs_ref"
ctx update --ref "$cortexfs_ref" --yes
```

更新器会构建原生包、保留配置与数据、只重启此前已处于活动状态的 CortexFS 单元，
并在健康检查失败时恢复精确的旧包。第一次按 ref 更新成功后，裸 `ctx update`
会复用已记录的跟踪 ref。

以普通用户运行 `ctx update`，需要 `sudo`、控制终端和 `flock`。不要以 root 运行。
第一次计划必须带 `--ref` 或 `--source`；`--yes` 才应用该钉死提交。约束与打包二进制见
[Linux 安装包](packaging.md)。

## 开始会话

短交互命令把当前目录当作工作区：

```bash
ctx                 # 在此创建新会话并打开聊天
ctx resume          # 恢复此目录的会话
ctx status          # 显示 CortexFS 状态
ctx help            # 显示简洁 CLI 帮助
```

`ctx resume executor --session SESSION` 恢复显式会话。默认交互代理是 `executor`；
用 `CTX_DEFAULT_AGENT` 选择其他代理。更长的 `ctx agent ...` 命令仍用于生命周期、
原始 socket、PTY、历史和诊断。

## 认识 `/ctx`

用 `ctx status` 和 `ctx ls` 检查挂载。[根 ABI](spec/root-abi.md) 恰好有八项：

```text
status
bin/
model/
agent/
tool/
channel/
home/
shared/
```

Provider 配置、工作流、数据库、MCP 注册表和技能保留在根命名空间之外。

## 设置 Shell 环境

多数命令会自动推断这些默认值。需要时再显式配置：

```bash
eval "$(ctx env)"
```

这会设置 `CTX_ROOT`、`CTX_HOME`、`CTX_PATH`，并把 `/ctx/bin` 加入普通 shell
`PATH`。`CTX_PATH` 只用于 CortexFS 工具查找，不用于查找模型或代理。

## 第一个对象检查

```bash
ctx ls model
ctx ls agent
ctx ls tool
ctx which model debug/echo
ctx which tool fs.read
ctx file type tool/fs.read
ctx file tool/fs.read
```

`model/debug/echo` 是最小调试模型。它回显输入，适合确认本地安装和 ABI 路径是否可用。

## 启动代理终端

`ctx agent start` 用 `systemd-run --user` 在 bwrap 沙箱里启动 `ctxterm -> tsh`。
默认把调用者当前目录以读写方式挂到 `/workspace`。普通 Git 元数据随该 workspace 保持可写；
CortexFS 不再隐式把 `.git` 只读叠挂。显式 path policy 仍可把 `/workspace/.git` 收窄为只读，
工作区外 linked-worktree gitdir 仍需单独挂载。代理的 `pwd` 是 `/workspace`，`HOME` 是沙箱自己的
`/home/agent`。`-- COMMAND` 可把 `tsh` 换成托管 Agent CLI：

```bash
ctx agent start executor --session default
ctx agent start executor --session default -- /usr/bin/codex
ctx agent watch executor --session default
ctx agent attach executor --session default
```

`watch` 只观察终端输出；`attach` 加入终端并写入 stdin。额外挂载必须显式声明：

```bash
ctx agent start executor --session docs \
  --mount "$PWD" /workspace rw \
  --mount "$PWD/docs" /docs ro \
  --cwd /workspace
```

`tsh` 不是主机 shell。它只通过 `CTX_PATH` 查找 CortexFS 工具，例如 `/ctx/tool` 和
`/ctx/home/<uid>/tool`。`bash`、`tmux` 和 `zellij` 也必须作为工具可见才能运行。

## 排查安装问题

`ctx doctor` 报告每项 ABI 检查，失败时以非零状态退出。根据报错路径或组件
选择下一项检查：

| 现象 | 处理 |
| --- | --- |
| `missing root /ctx` 或 `ctx status` 失败 | 启动挂载：`sudo systemctl start cortexfs.service`。原生包会启用该单元，但首次安装不会启动它。 |
| FUSE / `fusermount3` / `/dev/fuse` 错误 | 加载内核模块（`sudo modprobe fuse`），并安装 `fuse3` 与 `pkg-config fuse3`。容器需要 FUSE 设备。 |
| bubblewrap 过旧 | 安装器与更新器要求 `/usr/bin/bwrap` 0.10+。通过发行版升级；CortexFS 不会覆盖主机 `bwrap`。 |
| Hosted CLI 没有网络 | 默认沙箱是 `--unshare-net`。显式 `ctx agent start NAME -- COMMAND` 仅在 policy 允许 `network:default connect`、存在 `/usr/bin/pasta` 与 `/dev/net/tun` 时启用 pasta。Arch 包装 `passt`；Debian/RPM 元数据目前没有。缺少 pasta 或 tun 则保持隔离。 |
| Hosted CLI 不能写 `/ctx` | 可写 `/ctx` 仅适用于规范 `/ctx` 根上的显式 hosted 命令，且 `findmnt` 确认读写 `fuse`/`fuse.cortexfs` CortexFS 挂载。无命令的 `tsh` 启动仍只读。 |
| `stale` / `stale-user` 的 `agent/coder` 或 `agent/worker` | 这些名字是退役残留。受管树是 `architect`、`executor`、`product-manager`。`agent/main` 别名指向 `executor`。先 `ctx bootstrap --check`，需要当前树时再 `ctx bootstrap`。不要设置 `agent/coder.d/model`。 |
| 旧版安装器在 `agent/coder.d/model` 处退出 | 旧版可选引导使用已退役的 `coder`，全新安装时可能在验证前退出。固定快照的安装器使用 `executor`。重新运行该安装器，用 `ctx set agent/executor.d/model PROVIDER/MODEL` 绑定已配置的模型，再执行 `ctx doctor`。已有频道应指向 `/ctx/agent/main.sock` 或 `/ctx/agent/executor.sock`。 |
| `ctx update` 之后缺少 `/ctx` | 更新器只重启事务前已活动的单元，并包含精确的 `cortexfs.service` 名字。如果挂载在事务前就是未活动，请执行 `sudo systemctl start cortexfs.service`。 |
| `ctx agent start` 连不上 user systemd | 终端路径使用 `systemd-run --user`。请登录到有 user bus 的用户会话；linger 是主机策略，不是 CortexFS ABI。 |
| `ctx update` 拒绝运行 | 用非 root 登录，并具备 `sudo`、`/dev/tty` 和 `flock`。已安装 helper 是 `/usr/lib/cortexfs/update-linux`，必须保持 root 所有。 |
| 交互代理碰到内存/CPU/任务上限 | 终端使用 `MemoryMax=1G` / `CPUQuota=200%` / `TasksMax=256`。socket 激活的 runtime 使用 `512M` / `100%` / `128`。见 [Agent Runtime](spec/agent-runtime.md)。 |
| 默认聊天连到错误代理 | 默认是 `executor`。用 `CTX_DEFAULT_AGENT` 覆盖。 |

`ctx doctor` 检查 ABI 形态，不检查 provider 凭据。`unconfigured` 模型仍需要
`ctx provider preset` 和 `ctx auth login`。

## 下一步

[日常使用](using-cortexfs.md) 介绍 `ctx`、`agent.sh`、共享目录与会话历史；
其他主题见 [文档目录](README.md)。
