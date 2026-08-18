---
id: getting-started
title: 从安装开始
sidebar_label: 从安装开始
---

# 从安装开始

CortexFS 是一个 Linux 文件系统 ABI。首先安装它，确认 `/ctx` 可用，
然后转向代理、工具和扩展点。

## 安装

一键安装程序支持当前的 Arch、Debian/Ubuntu、Fedora/RHEL,
以及使用 systemd 启动的 openSUSE/SLES 系列 Linux 发行版，当它们的
已启用的存储库提供所需的软件包：

```bash
curl -fsSL https://raw.githubusercontent.com/LIghtJUNction/cortexfs/main/scripts/install.sh | sh
```

它检查发行版、systemd、FUSE、bubblewrap 0.10 和 Rust MSRV
在使用 `Cargo.lock` 本地构建下载的源代码快照之前。
每个持续的突变都有一个精确的类型确认。重新运行是
幂等：程序和单元文件被更新，同时存储、密钥、提供者
配置、现有的环境文件，以及 `/ctx` 用户状态是
已保留。真正的首次安装会从系统区域设置中选择一种语言
并提供可选的 OpenAI、Codex、Anthropic 或 Google 入职培训
通过
现有提供者命令。

Arch 用户也可以从 AUR 安装 `cortexfs-git`，启用
`cortexfs.service`，并运行`ctx doctor`。

`ctx doctor` 应该报告挂载点、基础目录和默认模型是否
别名和代理运行时状态可用。`ctx --help` 列出了
当前构建支持的子命令，包括 `ctx agent`、`ctx file`、
`ctx send`、`ctx exec` 和套接字便利功能。默认的实时测试不
依赖外部云 API。

## 认识 `/ctx`

安装后，检查根部：

```bash
ctx status
ctx ls
```

你应该会看到一小组稳定的条目：

```text
status
bin/
model/
agent/
tool/
home/
shared/
```

这就是核心 CortexFS 的权衡：根节点只保留稳定的对象类。
提供者、工作流程、数据库、MCP 注册表和技能注册表的详细信息不
成为新的顶级 ABI 条目。

## 设置 Shell 环境

大多数命令会自动推断这些默认值。请明确配置它们
在需要时：

```bash
eval "$(ctx env)"
```

这设置了 `CTX_ROOT`、`CTX_HOME`、`CTX_PATH`，并将 `/ctx/bin` 添加到常规
shell `PATH`。`CTX_PATH` 仅用于 CortexFS 工具查找；它不用于
寻找模型或代理人。

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

`model/debug/echo` 是最小的调试模型。它会回显输入，并且很有用
用于确认本地安装和 ABI 路径是否有效。

## 启动代理终端

`ctx agent start` 使用 `systemd-run --user` 在内部启动 `ctxterm -> tsh`
bwrap 沙箱。默认情况下，它以读写方式挂载调用者的当前目录
在 `/workspace`。如果当前目录包含 `.git`，则还会
在 `/workspace/.git` 上以只读方式超载。代理从 `pwd` 设置开始
`/workspace`，而 `HOME` 是沙盒自己的 `/home/agent`：

```bash
ctx agent start coder --session default
ctx agent watch coder --session default
ctx agent attach coder --session default
```

`watch` 仅观察终端输出；`attach` 加入终端并进行写入
stdin。请明确声明额外的挂载点：

```bash
ctx agent start coder --session docs \
  --mount "$PWD" /workspace rw \
  --mount "$PWD/docs" /docs ro \
  --cwd /workspace
```

`tsh` 不是主机外壳。它仅通过 `CTX_PATH` 查找 CortexFS 工具，
例如 `/ctx/tool` 一个
d `/ctx/home/<uid>/tool`。`bash`、`tmux` 和 `zellij`
在运行之前，它们也必须作为工具可见。

## 下一步

继续使用 [Daily Usage](using-cortexfs.md)：`ctx`，`agent.sh`，共享
目录和会话历史。
