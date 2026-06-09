---
title: Provider/Route Instead of Channel
---

# Provider/Route Instead of Channel

CortexFS does not expose `/ctx/chan`. The old channel idea is split into two existing file objects:

```text
provider/<id>/          backend instance: URL, account type, formats, health, models, secret status
home/<uid>/route/<fmt>/ current user's route result for an API format
```

This avoids inventing a second abstraction beside provider and route.

## Backend instances

```bash
cat /ctx/provider/list
cat /ctx/provider/openai-main/format
cat /ctx/provider/openai-main/url/effective
cat /ctx/provider/openai-main/secrets/status
```

During development, runtime provider views can be adjusted through small control files:

```bash
printf 'https://relay.example.com/v1\n' > /ctx/provider/openai-main/url/current
printf '1\n' > /ctx/provider/openai-main/enabled/current
```

Raw secrets never enter the mounted tree; the tree only exposes secret status and key IDs.

## Route observation

```bash
CTX_HOME=/ctx/home/$(id -u)
cat "$CTX_HOME/route/openai.chat/provider"
cat "$CTX_HOME/route/openai.chat/model"
cat "$CTX_HOME/route/openai.chat/reason"
```

## Request submission

External gateways, workflows, and bot bridges use the same file submission rule:

```bash
api="$CTX_HOME/api/openai.chat"
printf '%s\n' '{"messages":[{"role":"user","content":"hello"}]}' > "$api/inbox/001.tmp"
mv "$api/inbox/001.tmp" "$api/inbox/001.req.json"
cat "$api/outbox/001.fingerprint"
```

Local HTTP/UDS entry points must enter the same provider, route, policy, store, and audit pipeline. They must not bypass the file ABI and create another set of facts.
