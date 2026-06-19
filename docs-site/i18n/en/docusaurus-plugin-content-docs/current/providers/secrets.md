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
  inbox/
  outbox/
```

Rules:

- API keys, OAuth tokens, and session tokens are not plaintext FUSE files.
- `active` is a key id or secret reference, not the raw key.
- `rotate` is a daemon-side control node.
- `cortexd` resolves real secrets from a protected secret store.
- Audit must redact secret material.

## Import API keys

Import uses same-directory atomic rename. The request body is handled into the
system secret store; the mount tree keeps only status and a secret reference.

```bash
secrets=/ctx/provider/relay-openai/secrets
cat > "$secrets/inbox/api-key.tmp" <<'JSON'
{
  "op": "import",
  "kind": "bearer",
  "value": "sk-placeholder"
}
JSON
mv "$secrets/inbox/api-key.tmp" "$secrets/inbox/api-key.req.json"
cat "$secrets/outbox/api-key.resp.json"
cat "$secrets/status"
cat "$secrets/active"
```
