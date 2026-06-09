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
- Same-directory rename to `inbox/*.req.json` submits the request.
- Rename only queues work and writes derived facts; FUSE callbacks must not call
  remote providers.
- The request id comes from the filename stem.
- Requests and responses keep the native API JSON shape.
- Each request gets a fingerprint.
- Each request writes an audit event.

## Outbox Files

```text
outbox/<id>.route.json
outbox/<id>.fingerprint
outbox/<id>.resp.json
outbox/<id>.error
```

`route.json` exposes route metadata such as provider, model, format, reason,
and fingerprint.

CortexFS does not expose second submission semantics under `job/`, `hook/`, or
`chan/`; external tasks and triggers should write these generic inboxes.
