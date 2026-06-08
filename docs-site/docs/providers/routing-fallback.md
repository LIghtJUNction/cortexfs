---
title: Routing and Fallback
---

# Routing and Fallback

Use `priority` for fallback tiers and `weight` for traffic split inside the same tier.

```text
openai-main      priority=100 weight=80
openai-relay-a   priority=100 weight=20
kimi-main        priority=80  weight=100
local-vllm       priority=10  weight=100
```

## Semantics

1. Try the highest healthy priority tier first.
2. Split traffic inside that tier by weight.
3. Fall back to lower tiers only when the current tier is unavailable, disabled, rate limited, missing secrets, or circuit-broken.
4. Keep every decision visible through route metadata and audit events.

## Read Current Route

```bash
cat "$CTX_HOME/route/openai.chat/provider"
cat "$CTX_HOME/route/openai.chat/model"
cat "$CTX_HOME/route/openai.chat/reason"
```

Read selected provider URL:

```bash
p=$(cat "$CTX_HOME/route/openai.chat/provider")
cat "/ctx/provider/$p/url/effective"
```

The current implementation exposes simple route/default provider behavior. Weighted fallback belongs in `cortexd` while preserving the same route/policy/secret/provider/store/audit/export pipeline.
