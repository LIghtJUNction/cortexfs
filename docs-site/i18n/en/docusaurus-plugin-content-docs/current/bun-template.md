---
title: Bun Template
---

# Bun CortexFS Client Template

This template is a zero-dependency Bun client for CortexFS.

It supports:

- `file`: submit through the stable CortexFS file ABI and write `control/drain`.
- `http`: call the local OpenAI-compatible API derived from `home/<uid>/api/http/listen`.

Run:

```bash
cd templates/bun-cortexfs-client
bun run route
bun run chat -- "Reply with exactly: cortexfs-ok"
```

Use a production mount:

```bash
export CTX_HOME=/ctx/home/$(id -u)
bun run models
bun run chat -- "hello"
```

Use local HTTP mode:

```bash
export CORTEXFS_TRANSPORT=http
bun run chat -- "hello"
```

Provider API keys are not read from this template. CortexFS should resolve real
provider secrets inside `cortexd`.
