---
title: Secrets
---

# Secrets

Provider secrets do not enter the mount tree.

```text
provider/<id>/secrets/
  status
  active
  rotate
  last_rotated
  next_rotation
```

Rules:

- API keys, OAuth tokens, and session tokens are not plaintext FUSE files.
- `active` is a key id or secret reference, not the raw key.
- `rotate` is a daemon-side control node.
- `cortexd` resolves real secrets from a protected secret store.
- Audit must redact secret material.
