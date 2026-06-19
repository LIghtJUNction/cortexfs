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

The recommended path is the systemd-backed background service:

```bash
cortex start
```

`cortex start` manages the systemd unit `cortexfs@$USER.service`. This is a system service operation, so the CLI requests admin authorization when needed. The service loads FUSE, clears a broken `/ctx` mount, creates `/ctx`, and sets owner/mode automatically. The default deployment does not require manually creating `/ctx` or manually configuring mount permissions.

The service reads this local environment file:

```text
~/.config/cortexfs/.env
```

Write the OpenAI-compatible provider configuration there:

```bash
CORTEXFS_OPENAI_BASE_URL=https://api.example.com/
CORTEXFS_OPENAI_API_KEY=...
CORTEXFS_OPENAI_MODEL=gpt-4o-mini
```

This file is a local secret and must not be committed to Git. `CORTEXFS_OPENAI_BASE_URL` may be either the service root or the `/v1` base path; the runtime normalizes it to OpenAI-compatible endpoints.

For foreground debugging, mount manually:

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
cortex stop
```

## systemd Background Mount

The AUR package installs the systemd template unit `cortexfs@.service`. `/ctx` is a system path, so the unit uses an internal prep step to load FUSE, clear broken mounts, create the directory, then runs the FUSE process as the selected user.

Enable and start the background mount for the current user:

```bash
cortex start
```

Inspect it:

```bash
systemctl status "cortexfs@$USER.service"
findmnt /ctx
cat /ctx/status
```

Stop and unmount:

```bash
cortex stop
```

If a foreground `cortex mount` was killed manually, a broken FUSE endpoint may remain. The background service cleans it before startup:

```bash
cortex restart
```

## Multi-user Deployment

The default `cortex start` path manages one systemd template instance and mounts
the single `/ctx` tree in multi-user FUSE mode. Do not start one `/ctx` mount per
Linux user on the same machine; choose one owner instance for the mount and let
other local users access that same `/ctx`.

Use the same multi-user option for foreground debugging:

```bash
cortex mount --multi-user /ctx
```

Paths are namespaces, not security boundaries. Multi-user deployments should
still rely on host credential, external subject, object context, and Cortex
policy for access decisions.

## Build from Source

Source builds are for developers and CI, not the default end-user installation path:

```bash
cargo build --locked --workspace
cargo run -p cortex-cli -- status
```

Repository FUSE integration tests always use `tests/mounts/cortexfs`. Do not put source, fixtures, or persistent data there.
