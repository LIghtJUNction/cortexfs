---
title: 快速开始
---

# 快速开始

CortexFS 当前以 Linux FUSE 文件系统形式运行。Arch Linux 用户推荐先从 AUR 安装 `cortexfs-git`，然后使用已安装的 `cortex` 命令挂载树并通过文件 ABI 提交请求。

## 安装

```bash
paru -S cortexfs-git
cortex status
```

`status` 会输出推荐挂载点、ABI 名称、当前 live-test fixture 等发现信息。

## 挂载

生产式本地路径推荐 `/ctx`：

```bash
sudo mkdir -p /ctx
sudo chown "$USER:$USER" /ctx
cortex mount /ctx
export CTX_HOME="/ctx/home/$(id -u)"
```

`cortex mount` 是前台进程。保持挂载进程运行，然后在另一个终端继续检查或提交请求。

开发者做仓库内集成测试时，固定挂载点是：

```bash
mkdir -p tests/mounts/cortexfs
cargo run -p cortex-cli -- mount tests/mounts/cortexfs
```

`tests/mounts/cortexfs` 只作为测试挂载点，不要放源码、fixture 或持久化数据。

## 检查

```bash
cat /ctx/status
cat /ctx/cap/format
cat /ctx/provider/list
cat "$CTX_HOME/model/list"
cat "$CTX_HOME/route/openai.chat/provider"
```

很多文件是运行时投影，类似 `/proc` 或 `sysfs`，不是普通落盘数据目录。
