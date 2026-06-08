---
title: UDS Control Plane
---

# UDS Control Plane

CortexFS uses a two-plane design:

```text
FUSE data plane     cat/echo/ls/mv, stable file ABI
UDS control plane   .sock bidirectional IPC, streaming output, telemetry, FD passing
```

The file tree is the auditable source of truth. Unix sockets are the low-latency fast path and must not bypass policy, route, secret, store, or audit.

## Socket Locations

Current ABI exposes:

```bash
cat /ctx/home/$(id -u)/api/unix/path
ls -l /ctx/home/$(id -u)/api/unix/api.sock
```

Future session-level stream sockets:

```text
home/<uid>/thread/<id>/io.sock
home/<uid>/job/<id>/stream.sock
```

## Requirements

- FUSE threads do not call remote APIs.
- Network requests enter a Tokio worker pool.
- FUSE and API workers communicate through queues.
- `.sock` RPC carries high-frequency control, status queries, and streaming tokens.
- UDS can accept external file descriptors with `SCM_RIGHTS`, avoiding large-file copies through FUSE.
- Stream sockets support bidirectional interaction: interrupt, append input, and return tool results.
- Telemetry exposes runtime latency, cache hit rate, quota, and queue depth.

## Future Streaming Experience

```bash
nc -U /ctx/home/$(id -u)/job/translate.zh/stream.sock
```

After the LLM returns the first token batch, CortexFS writes it to the socket immediately, so the terminal sees incremental output.

## Constraint

Development refresh still uses Git commits as the only event boundary. Do not add hot-reload, polling, or background watcher subcommands; socket listeners belong to the installed service or daemon runtime.
