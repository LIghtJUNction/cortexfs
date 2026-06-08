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

`cortex start` prepares `/ctx`, fixes owner/mode, and sends the foreground mount process an exit signal when stopping. The default single-user deployment does not require manual FUSE permission configuration.

## Multi-user Mounts

Default mounts are single-user. Use explicit multi-user mode only when one mount must be shared across Linux users:

```bash
cortex mount --multi-user /ctx
```

Paths are namespaces, not security boundaries. Decisions should use host
credential, external subject, object context, and Cortex policy.
