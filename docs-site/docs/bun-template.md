# Bun CortexFS Client Template

This template is a zero-dependency Bun client for CortexFS.

It supports two transports:

- `file`: submits through the stable CortexFS file ABI and writes `control/drain`.
- `http`: calls the local OpenAI-compatible API derived from `home/<uid>/api/http/listen`.

The file transport is the default because it works with the current CortexFS projection. The HTTP transport is for a running local API daemon.

## Run

```bash
cd templates/bun-cortexfs-client
bun run chat -- "Reply with exactly: cortexfs-ok"
```

With a mounted repo test mount:

```bash
export CORTEXFS_MOUNT=../../tests/mounts/cortexfs
export CORTEXFS_UID=1000
bun run route
bun run chat -- "Reply with exactly: cortexfs-ok"
```

With a production mount:

```bash
export CTX_HOME=/ctx/home/$(id -u)
bun run models
bun run chat -- "hello"
```

Use the local HTTP API:

```bash
export CORTEXFS_TRANSPORT=http
bun run chat -- "hello"
```

## Environment

```text
CTX_HOME             Explicit CortexFS user home, for example /ctx/home/1000.
CORTEXFS_MOUNT      Mount root. Default: /ctx.
CORTEXFS_UID        User id under home/. Default: process uid, then 1000.
CORTEXFS_FORMAT     API format. Default: openai.chat.
CORTEXFS_TRANSPORT  file or http. Default: file.
CORTEXFS_BASE_URL   Optional local API base URL override.
CORTEXFS_API_KEY    Optional HTTP bearer token.
CORTEXFS_PROMPT     Prompt used when no CLI prompt is provided.
```

Provider API keys are not read from this template. CortexFS should resolve real provider secrets inside `cortexd`; the mount tree only exposes route and secret status.
