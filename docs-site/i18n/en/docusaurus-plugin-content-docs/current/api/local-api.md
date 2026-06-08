---
title: Local API
---

# Local API

CortexFS reserves a local API so OpenAI-compatible clients can enter the same
execution plane.

```text
127.0.0.1:6185
/run/user/<uid>/cortex/api.sock
```

Endpoints:

```text
GET  /v1/models
POST /v1/chat/completions
POST /v1/responses
POST /v1/messages
POST /v1/generateContent
```

Discover metadata:

```bash
cat "$CTX_HOME/api/endpoints"
cat "$CTX_HOME/api/http/listen"
cat "$CTX_HOME/api/http/localurl"
cat "$CTX_HOME/api/unix/path"
cat "$CTX_HOME/api/pipeline"
```

`localurl` is the local OpenAI-compatible base URL clients should read:

```bash
base_url="$(cat "$CTX_HOME/api/http/localurl")"
```

The current projected value is `http://127.0.0.1:6185/v1`. This is the discovery ABI; the actual HTTP listener belongs to the future daemon runtime and should not be confused with the FUSE file projection.

File paths, HTTP, and Unix sockets must enter the same normalize, route,
policy, secret, provider, store, audit, and export pipeline.
