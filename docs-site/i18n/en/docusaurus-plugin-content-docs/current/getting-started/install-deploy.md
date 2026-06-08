---
title: Installation and Deployment
---

# Installation and Deployment

For end users, CortexFS starts from the installed `cortex` command. Arch Linux users can install the AUR package `cortexfs-git`; daily usage should not require `cargo run`.

## Arch Linux

Install with an AUR helper:

```bash
paru -S cortexfs-git
```

Verify the CLI:

```bash
cortex status
```

`status` prints the recommended `/ctx` mountpoint, ABI name, default test mountpoint, and live-test fixture information.

## Single-user Deployment

Create the recommended mountpoint:

```bash
sudo mkdir -p /ctx
sudo chown "$USER:$USER" /ctx
```

Start the mount:

```bash
cortex mount /ctx
```

`cortex mount` runs in the foreground. Keep that terminal open, then inspect the mounted tree from another terminal:

```bash
export CTX_HOME="/ctx/home/$(id -u)"
cat /ctx/status
cat /ctx/provider/list
cat "$CTX_HOME/model/list"
```

Unmount:

```bash
fusermount3 -u /ctx
```

## Multi-user Deployment

Multi-user mounts must explicitly enable CortexFS multi-user mode and must allow FUSE `allow_other`.

1. Edit `/etc/fuse.conf` and enable:

```text
user_allow_other
```

2. Prepare mountpoint permissions for the local users that should enter `/ctx`:

```bash
sudo mkdir -p /ctx
sudo chmod 755 /ctx
```

3. Mount with the installed `cortex` command:

```bash
cortex mount --multi-user /ctx
```

Paths are namespaces, not security boundaries. Multi-user deployments should still rely on host credential, external subject, object context, and Cortex policy for access decisions.

## Build from Source

Source builds are for developers and CI, not the default end-user installation path:

```bash
cargo build --locked --workspace
cargo run -p cortex-cli -- status
```

Repository FUSE integration tests always use `tests/mounts/cortexfs`. Do not put source, fixtures, or persistent data there.
