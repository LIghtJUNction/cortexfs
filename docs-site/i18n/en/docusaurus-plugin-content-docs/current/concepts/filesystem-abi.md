---
title: Filesystem ABI
---

# Filesystem ABI

CortexFS is a stable filesystem ABI, not an application UI. Scripts, agent
runtimes, workflow engines, and local tools should depend on path and file
semantics.

## Rules

- Top-level directories use short singular nouns, such as `provider/` and `model/`.
- Small configuration values are small text files.
- Native API requests and responses are JSON.
- Messages, audit streams, and exports are JSONL.
- Sockets are low-latency fast paths, not the source of truth.
- Slow work enters the daemon/execution plane.

## File Kinds

```text
no extension    small text attribute or control node
*.req.json      native API request
*.resp.json     native API response
*.error         error object
*.jsonl         append-only logs, messages, audit, training data
*.md            human-readable view
*.sock          Unix domain socket fast path
schema.json     large schema
manifest.json   large manifest
```
