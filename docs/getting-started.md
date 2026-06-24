---
id: getting-started
title: 从安装开始
sidebar_label: 从安装开始
---

# 从安装开始

CortexFS 是一个 Linux 文件系统 ABI。先把它装起来，确认 `/ctx` 可用，再继续看
agent、tool 和二次开发。

## 安装

Arch Linux 用户可以直接安装 AUR 包：

```bash
paru -S cortexfs-git
sudo systemctl enable --now cortexfs.service
ctx doctor
ctx --help
```

`ctx doctor` 应该告诉你挂载点、基础目录、默认模型 alias 和 agent runtime 状态是否
可用。`ctx --help` 会列出当前构建支持的子命令，包括 `ctx agent`、`ctx file`、
`ctx send`、`ctx exec` 和 socket 相关便利命令。默认 live test 不依赖外部云 API。

## 认识 `/ctx`

安装后先看根目录：

```bash
ctx status
ctx ls
```

你会看到少量稳定入口：

```text
status
bin/
model/
agent/
tool/
home/
shared/
```

这就是 CortexFS 的核心取舍：根目录只保留稳定对象类，不把 provider、workflow、
database、MCP registry 或 skill registry 做成新的顶层 ABI。

## 设置 shell 环境

大多数命令会自动推导默认值。需要显式配置时使用：

```bash
eval "$(ctx env)"
```

这会设置 `CTX_ROOT`、`CTX_HOME`、`CTX_PATH`，并把 `/ctx/bin` 放入普通 shell
`PATH`。`CTX_PATH` 只用于 CortexFS tool 查找，不用于查找 model 或 agent。

## 第一次检查对象

```bash
ctx ls model
ctx ls agent
ctx ls tool
ctx which model debug/echo
ctx which tool fs.read
ctx file classify tool/fs.read
```

`model/debug/echo` 是最小调试模型，只回显输入，适合确认本机安装和 ABI 路径都正常。

## 启动一个 agent 终端

`ctx agent start` 会通过 `systemd-run --user` 启动 `ctxterm -> tsh`，并放进 bwrap
sandbox。默认会把当前目录以读写方式挂到 agent 看到的 `/workspace`：

```bash
ctx agent start coder --session default
ctx agent watch coder --session default
ctx agent attach coder --session default
```

`watch` 只观察终端输出；`attach` 会加入终端并写入 stdin。需要额外挂载时显式声明：

```bash
ctx agent start coder --session docs \
  --mount "$PWD" /workspace rw \
  --mount "$PWD/docs" /docs ro \
  --cwd /workspace
```

`tsh` 不是 host shell。它只按 `CTX_PATH` 查找 `/ctx/tool`、`/ctx/home/<uid>/tool`
等 CortexFS tool。`bash`、`tmux`、`zellij` 也必须作为 tool 可见后才可执行。

## 下一步

继续读 [日常使用](using-cortexfs.md)，从 `ctx`、`agent.sh`、共享目录和 session
历史开始。
