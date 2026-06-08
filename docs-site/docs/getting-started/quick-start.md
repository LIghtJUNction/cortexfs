---
title: Quick Start
---

# Quick Start

CortexFS 当前以 Linux FUSE 文件系统形式运行。最小路径是构建 CLI、挂载树、通过文件 ABI 提交请求。

## Build

```bash
cargo build --locked --workspace
cargo run -p cortex-cli -- status
```

`status` 会输出推荐挂载点、ABI 名称、当前 live-test fixture 等发现信息。

## Mount

生产式本地路径推荐 `/ctx`：

```bash
sudo mkdir -p /ctx
sudo chown "$USER:$USER" /ctx
cargo run -p cortex-cli -- mount /ctx
export CTX_HOME="/ctx/home/$(id -u)"
```

仓库内集成测试固定挂载点：

```bash
mkdir -p tests/mounts/cortexfs
cargo run -p cortex-cli -- mount tests/mounts/cortexfs
```

`tests/mounts/cortexfs` 只作为测试挂载点，不要放源码、fixture 或持久化数据。

## Inspect

```bash
cat /ctx/status
cat /ctx/cap/format
cat /ctx/provider/list
cat "$CTX_HOME/model/list"
cat "$CTX_HOME/route/openai.chat/provider"
```

很多文件是运行时投影，类似 `/proc` 或 `sysfs`，不是普通落盘数据目录。
