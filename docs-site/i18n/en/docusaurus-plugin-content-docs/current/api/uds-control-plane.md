---
title: UDS Control Plane
---

# UDS Control Plane

CortexFS uses a two-plane design:

```text
FUSE data plane     cat/echo/ls/mv, stable file ABI
UDS control plane   .sock bidirectional IPC, streaming output, telemetry, FD passing
```

The file tree is the auditable source of truth. Unix sockets are low-latency fast paths and cannot bypass policy, route, secrets, store, or audit.

## Socket locations

Current ABI exposes:

```bash
cat /ctx/home/$(id -u)/api/unix/path
ls -l /ctx/home/$(id -u)/api/unix/api.sock
ls -l /ctx/home/$(id -u)/thread/demo/io.sock
```

CortexFS will not add `home/<uid>/job/<id>/stream.sock`. Continuous sessions use `thread/<id>/io.sock`; one-shot tasks use inbox/outbox.

## Design requirements

- FUSE threads do not perform remote API calls.
- Network requests enter the daemon queue or worker pool.
- FUSE and API workers communicate through queues.
- `.sock` RPC may carry high-frequency control, status queries, and streaming tokens.
- UDS may receive file descriptors with `SCM_RIGHTS` to avoid copying large files through FUSE.
- sockets must write the same store, audit, and export rows; they cannot become bypasses.

## Constraints

During development, refresh uses Git commits as the only boundary. Do not add hot reload, polling, or background watcher subcommands; socket listeners belong to installed services or the daemon runtime plane.
