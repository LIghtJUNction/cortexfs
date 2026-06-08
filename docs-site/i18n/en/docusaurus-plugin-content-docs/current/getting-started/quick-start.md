---
title: Quick Start
---

# Quick Start

CortexFS currently runs as a Linux FUSE filesystem. On Arch Linux, install the AUR package `cortexfs-git`, mount the tree with the installed `cortex` command, then submit a request through the file ABI.

```bash
paru -S cortexfs-git
cortex status
```

Production-style local mount:

```bash
cortex start
export CTX_HOME="/ctx/home/$(id -u)"
```

`cortex start` starts `cortexfs@$USER.service` through systemd.

Developer repository test mount:

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
