---
title: New API Replacement
---

# New API Replacement

CortexFS should replace New API-style local gateway control planes, not expose provider demos.

The default state must have zero backends:

```text
/ctx/provider/list   empty
/ctx/model/list      empty
/ctx/chan/list       empty
```

Channels, models, and routes appear only after explicit user config.

## File-based Channel Setup

Connect a relay through mounted-tree file operations. Example for `https://api.fengying.xin`:

First put the real key in a local `.env` file. Do not commit it:

```bash
mkdir -p ~/.config/cortexfs
printf 'FENGYING_API_KEY=sk-...\n' > ~/.config/cortexfs/.env
chmod 600 ~/.config/cortexfs/.env
cortex restart
```

For repository-local temporary tests, the root `.env` file is also ignored by Git:

```bash
printf 'FENGYING_API_KEY=sk-...\n' > .env
```

Then create the channel through `/ctx` file operations:

```bash
mkdir /ctx/chan/fengying
printf 'https://api.fengying.xin\n' > /ctx/chan/fengying/url
printf 'env:FENGYING_API_KEY\n' > /ctx/chan/fengying/keyref
printf 'openai.chat\nopenai.responses\n' > /ctx/chan/fengying/fmt
printf '*\n' > /ctx/chan/fengying/mod
printf '1\n' > /ctx/chan/fengying/enabled

cat /ctx/chan/fengying/status
cat /ctx/chan/list
```

`keyref` is a secret reference, not a raw API key. The example means the real key is resolved from `FENGYING_API_KEY` or a future daemon secret store; the mounted tree does not expose the raw key.

Read the local standard API URL from the tree:

```bash
cat /ctx/chan/fengying/localurl
cat /ctx/home/$(id -u)/api/http/localurl
```

Current value:

```text
http://127.0.0.1:6185/v1
```

The current implementation provides the file ABI and local endpoint discovery. The long-running HTTP daemon that listens on `6185` and forwards requests upstream is still future runtime work. User scripts should discover the address through `localurl`, not hard-code it.

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

## Fallback

`fb` is a configurable policy, not a hard-coded branch.

Base policy pieces:

```text
prio    pick the highest available priority first
wt      split same-priority traffic by weight
health  skip down, rate-limited, over-quota, and missing-key channels
retry   retry same-priority or lower-priority channels by policy
```

Example:

```text
chan/openai-a prio=100 wt=80
chan/openai-b prio=100 wt=20
chan/kimi-a   prio=80  wt=100
```

## Persistence

Channels written through `/ctx/chan/<id>` are written through to local persistent storage:

```text
~/.config/cortexfs/chan.d/*.conf
~/.config/cortexfs/tok.d/*.conf
~/.config/cortexfs/.env
```

`chan.d/*.conf` is implemented now: the mount loads it at startup, so channels survive unmount and restart. Development refresh still uses Git commits and remounts as the boundary; CortexFS does not add background watchers, polling, or hot-reload subcommands.

`chan.d` stores `keyref`, not plaintext keys. Resolve the real secret through the systemd `EnvironmentFile=~/.config/cortexfs/.env`, keyring, pass, sops, or the future daemon secret store.
