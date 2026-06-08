---
title: First Request
---

# First Request

Plain `write()` only writes bytes. A request is submitted only when a staged
file is atomically renamed to `inbox/*.req.json`.

```bash
api="$CTX_HOME/api/openai.chat"

printf '%s\n' '{"messages":[{"role":"user","content":"Reply with cortexfs-ok"}]}' \
  > "$api/inbox/001.tmp"

mv "$api/inbox/001.tmp" "$api/inbox/001.req.json"
printf '1\n' > /ctx/control/drain
```

Read the result:

```bash
cat "$api/outbox/001.route.json"
cat "$api/outbox/001.fingerprint"
cat "$api/outbox/001.resp.json"
```

On failure:

```bash
cat "$api/outbox/001.error"
```
