---
title: Secrets
---

# Secrets

Provider secrets do not enter the mount tree.

## Exposed Files

```text
provider/<id>/secrets/
  status
  active
  rotate
  last_rotated
  next_rotation
```

These files expose state, not secret values.

## Rules

- API keys, OAuth tokens, and session tokens are not writable plaintext files in FUSE.
- `active` is a key id or secret reference, not the raw key.
- `rotate` is a control node for daemon-side rotation.
- `cortexd` resolves real secrets from a protected secret store.
- Audit must redact secret material.

## Rotate

```bash
printf '1\n' > /ctx/provider/openai-main/secrets/rotate
cat /ctx/provider/openai-main/secrets/status
cat /ctx/provider/openai-main/secrets/active
```
