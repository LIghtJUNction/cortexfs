---
title: Mounting
---

# Mounting

CortexFS exposes a stable mount tree. External programs should discover
providers, models, routes, and capabilities through files rather than
hard-coding ids.

## Recommended Mount

```text
/ctx
```

```bash
export CTX_HOME="/ctx/home/$(id -u)"
```

Common entries:

```text
$CTX_HOME/api
$CTX_HOME/thread
$CTX_HOME/model
$CTX_HOME/route
$CTX_HOME/memory
$CTX_HOME/export
```

## Background Mount

The default production mount uses systemd:

```bash
cortex start
```

`cortex start` requests system authorization to manage one
`cortexfs@<owner>.service` instance, prepares `/ctx`, fixes owner/mode, and
sends the foreground mount process an exit signal when stopping. The systemd
mount uses multi-user FUSE mode by default; do not start one `/ctx` mount per
Linux user on the same machine.

## Multi-user Mounts

Use multi-user mode for foreground debugging too:

```bash
cortex mount --multi-user /ctx
```

Paths are namespaces, not security boundaries. Decisions should use host
credential, external subject, object context, and Cortex policy.
