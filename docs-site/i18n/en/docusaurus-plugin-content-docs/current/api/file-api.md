---
title: File API
---

# File API

Each user entry exposes APIs by format.

```text
home/<uid>/api/
  openai.chat/
    inbox/
    outbox/
  openai.responses/
    inbox/
    outbox/
  anthropic.messages/
    inbox/
    outbox/
  google.generate_content/
    inbox/
    outbox/
```

## Contract

- `write()` does not trigger an API call.
- Rename to `inbox/*.req.json` submits the request.
- The request id is the filename stem.
- Requests and responses remain native API JSON.
- Each request gets a fingerprint and an audit event.

Outbox files:

```text
outbox/<id>.route.json
outbox/<id>.fingerprint
outbox/<id>.resp.json
outbox/<id>.error
```
