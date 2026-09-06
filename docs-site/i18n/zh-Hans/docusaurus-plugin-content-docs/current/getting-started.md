---
id: getting-started
title: 从安装开始
sidebar_label: 从安装开始
---

# 从安装开始

CortexFS 是一个 Linux 文件系统 ABI。先安装它，确认 `/ctx` 可用，
再转向代理、工具和扩展点。

## 安装

一键安装程序支持当前 Arch、Debian/Ubuntu、Fedora/RHEL 与
openSUSE/SLES 系列、以 systemd 启动且仓库能提供所需软件包的 Linux：

```bash
curl -fsSL https://raw.githubusercontent.com/LIghtJUNction/cortexfs/main/scripts/install.sh | sh
```

原生 `.deb`、`.rpm`、Arch 或便携 tar 包见 [Linux 安装包](packaging.md)。

要把代理接到 Telegram、Discord、Slack 或飞书/Lark，见
[多 IM 通道指南](channels.md)。该指南使用现有 agent socket 与耐久会话 ABI，
因此每次对话仍是多轮的。

安装器会检查发行版、systemd、FUSE、bubblewrap 0.10+ 和 Rust MSRV，
再用 `Cargo.lock` 在本地构建下载的源码快照。
每次持久变更都需要精确的类型确认。重复运行是幂等的：更新程序与单元文件，
同时保留存储、密钥、provider 配置、已有环境文件和 `/ctx` 用户状态。
真正的首次安装会按系统 locale 选语言，并可通过现有 provider 命令可选接入
OpenAI、Codex、Anthropic、Google、OpenRouter、DeepSeek、Groq 或任意
OpenAI 兼容端点。也可用 `cortexfs-channel list|show|preset` 打印 IM 宿主模板。

Arch 用户也可以从 AUR 安装 `cortexfs-git`，启用 `cortexfs.service`，再运行
`ctx doctor`。

`ctx doctor` 应报告挂载点、基础目录、默认模型别名和代理运行时状态是否可用。
`ctx --help` 列出当前构建支持的子命令。默认 live test 不依赖外部云 API。

## 更新主机

先对某个分支、标签或提交做计划，再应用同一钉死修订：

```bash
ctx update --ref main
ctx update --ref main --yes
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

安装后检查根目录：

```bash
ctx status
ctx ls
```

应看到一小组合稳定条目：

```text
status
bin/
model/
agent/
tool/
home/
shared/
```

这就是核心权衡：根只保留稳定对象类。provider、workflow、数据库、MCP 注册表和
技能注册表细节不会成为新的顶级 ABI 条目。

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
默认把调用者当前目录以读写方式挂到 `/workspace`。若当前目录包含 `.git`，还会把
`.git` 只读叠挂到 `/workspace/.git`。代理的 `pwd` 是 `/workspace`，`HOME` 是沙箱自己的
`/home/agent`：

```bash
ctx agent start executor --session default
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

`ctx doctor` 为每项 ABI 检查打印一行状态，任一失败则以非零退出。常见行：

```text
ok root /ctx
ok status
ok agent
ok agent/executor
ok bootstrap tree_version=9 migrations=retired-agents,rolling-tree,agent-update,current-models,agent-permissions,initial-agents
stale agent/coder (retired reference agent; run ctx bootstrap --check)
missing bootstrap state (run ctx bootstrap)
```

| 现象 | 处理 |
| --- | --- |
| `missing root /ctx` 或 `ctx status` 失败 | 启动挂载：`sudo systemctl start cortexfs.service`。原生包会启用该单元，但首次安装不会启动它。 |
| FUSE / `fusermount3` / `/dev/fuse` 错误 | 加载内核模块（`sudo modprobe fuse`），并安装 `fuse3` 与 `pkg-config fuse3`。容器需要 FUSE 设备。 |
| bubblewrap 过旧 | 安装器与更新器要求 `/usr/bin/bwrap` 0.10+。通过发行版升级；CortexFS 不会覆盖主机 `bwrap`。 |
| `stale` / `stale-user` 的 `agent/coder` 或 `agent/worker` | 这些名字是退役残留。受管树是 `architect`、`executor`、`product-manager`。`agent/main` 别名指向 `executor`。先 `ctx bootstrap --check`，需要当前树时再 `ctx bootstrap`。不要设置 `agent/coder.d/model`。 |
| 旧版安装器在 `agent/coder.d/model` 处退出 | 旧版可选引导使用已退役的 `coder`，全新安装时可能在验证前退出。当前安装器已使用 `executor`。重新运行当前安装器，用 `ctx set agent/executor.d/model PROVIDER/MODEL` 绑定已配置的模型，再执行 `ctx doctor`。已有频道应指向 `/ctx/agent/main.sock` 或 `/ctx/agent/executor.sock`。 |
| `ctx update` 之后缺少 `/ctx` | 更新器只重启事务前已活动的单元，并包含精确的 `cortexfs.service` 名字。如果挂载在事务前就是未活动，请执行 `sudo systemctl start cortexfs.service`。 |
| `ctx agent start` 连不上 user systemd | 终端路径使用 `systemd-run --user`。请登录到有 user bus 的用户会话；linger 是主机策略，不是 CortexFS ABI。 |
| `ctx update` 拒绝运行 | 用非 root 登录，并具备 `sudo`、`/dev/tty` 和 `flock`。已安装 helper 是 `/usr/lib/cortexfs/update-linux`，必须保持 root 所有。 |
| 交互代理碰到内存/CPU/任务上限 | 终端使用 `MemoryMax=1G` / `CPUQuota=200%` / `TasksMax=256`。socket 激活的 runtime 使用 `512M` / `100%` / `128`。见 [Agent Runtime](spec/agent-runtime.md)。 |
| 默认聊天连到错误代理 | 默认是 `executor`。用 `CTX_DEFAULT_AGENT` 覆盖。 |

`ctx doctor` 检查 ABI 形态，不检查 provider 凭据。`unconfigured` 模型仍需要
`ctx provider preset` 和 `ctx auth login`。

## 下一步

继续看 [日常使用](using-cortexfs.md)：`ctx`、`agent.sh`、共享目录和会话历史。
