---
title: New API Replacement
---

# New API Replacement

CortexFS should replace New API-style local gateway control planes, not expose provider demos.

The default state must have zero backends:

```text
/ctx/provider/list   empty
/ctx/model/list      empty
```

Channels, models, and routes appear only after explicit user config.

## Local Gateway

Expose one local standard endpoint:

```text
http://127.0.0.1:6185/v1
```

Compatible endpoints:

```text
GET  /v1/models
POST /v1/chat/completions
POST /v1/responses
POST /v1/messages
POST /v1/generateContent
```

Every endpoint enters the same pipeline:

```text
parse
norm
route
policy
key
send
store
log
bill
```

No bypass may skip policy, key, log, or billing.

## Short Names

The public ABI uses short names:

```text
chan     one upstream url + keyref + fmt + model set
url      upstream address
keyref   secret reference, not raw key
fmt      protocol format
mod      model name
grp      user group
tok      local access token
quota    quota
ratio    billing ratio
prio     fallback priority
wt       same-priority weight
fb       fallback policy
log      request log
```

## Control Plane

The file ABI maps to these New API-equivalent features:

```text
chan/
  count
  list
  <id>/
    url
    fmt
    keyref
    mod/
    grp
    ratio
    prio
    wt
    state
    health/

tok/
  count
  list
  <id>/
    name
    grp
    quota
    state

route/
  openai.chat/
    fb
    chan
    mod
    why

log/
  req.jsonl
  bill.jsonl
```

The old `provider/` tree may remain as a compatibility view, but the core control plane should converge on `chan/`.
