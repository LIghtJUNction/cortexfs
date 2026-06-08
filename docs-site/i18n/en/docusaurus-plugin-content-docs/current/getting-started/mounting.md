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

## Multi-user Mounts

Default mounts are single-user. Multi-user mode must be explicit and must be
combined with the system FUSE `allow_other`, owner, group, and mode policy.

```bash
cortex mount --multi-user /ctx
```

On Arch Linux, first make sure `/etc/fuse.conf` enables `user_allow_other`. Without that setting, FUSE rejects the `allow_other` mount option.

Paths are namespaces, not security boundaries. Decisions should use host
credential, external subject, object context, and Cortex policy.
