---
sidebar_position: 2
title: Routing and Fallback
---

# Routing and Fallback

CortexFS treats a provider as a backend/account instance, not a vendor name.
Multiple base URLs and keys should be represented as multiple provider ids:

```text
provider/
  openai-main/
  openai-relay-a/
  kimi-main/
  local-vllm/
```

## Priority and Weight

Use `priority` for fallback tiers and `weight` for traffic split inside the same
tier.

```text
openai-main      priority=100 weight=80
openai-relay-a   priority=100 weight=20
kimi-main        priority=80  weight=100
local-vllm       priority=10  weight=100
```

Route semantics:

1. Try the highest healthy priority tier first.
2. Split traffic inside that tier by weight.
3. Fall back to lower tiers only when the current tier is unavailable,
   disabled, rate limited, missing secrets, or circuit-broken.
4. Keep every decision visible through route metadata and audit events.

## Read the Current Route

```bash
CTX_HOME="/ctx/home/$(id -u)"

cat "$CTX_HOME/route/openai.chat/provider"
cat "$CTX_HOME/route/openai.chat/model"
cat "$CTX_HOME/route/openai.chat/reason"
```

Read the selected provider's effective upstream URL:

```bash
p=$(cat "$CTX_HOME/route/openai.chat/provider")
cat "/ctx/provider/$p/url/effective"
```

Read the local OpenAI-compatible API endpoint:

```bash
cat "$CTX_HOME/api/http/listen"
```

The current ABI exposes simple provider selection. Weighted fallback should be
implemented in `cortexd` while keeping route, policy, secret resolve, provider
call, store, audit, and export on the same pipeline.
