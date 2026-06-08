---
title: Quick Start
---

# Quick Start

CortexFS currently runs as a Linux FUSE filesystem. The minimum path is:
build the CLI, mount the tree, then submit a request through the file ABI.

```bash
cargo build --locked --workspace
cargo run -p cortex-cli -- status
```

Production-style local mount:

```bash
sudo mkdir -p /ctx
sudo chown "$USER:$USER" /ctx
cargo run -p cortex-cli -- mount /ctx
export CTX_HOME="/ctx/home/$(id -u)"
```

Repository test mount:

```bash
mkdir -p tests/mounts/cortexfs
cargo run -p cortex-cli -- mount tests/mounts/cortexfs
```

`tests/mounts/cortexfs` is only a local test mountpoint. Do not put source,
fixtures, or persistent data there.

Inspect the mounted ABI:

```bash
cat /ctx/status
cat /ctx/cap/format
cat /ctx/provider/list
cat "$CTX_HOME/model/list"
cat "$CTX_HOME/route/openai.chat/provider"
```
