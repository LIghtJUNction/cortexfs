---
title: External Triggers
---

# External Triggers

CortexFS does not expose `home/<uid>/hook`, and external triggers are not filesystem ABI names. systemd timers, cron, git hooks, CI jobs, webhook bridges, and other external triggers should write the generic inbox directly and submit by atomic rename.

Recommended split:

```text
systemd timer / cron / CI / webhook   decides when to trigger
CortexFS inbox/outbox                  receives requests and exposes results
cortexd / control/drain                executes the queue
CortexFS audit                         records facts
```

## systemd timer example

```bash
systemd-run --user --on-calendar='daily' \
  sh -lc 'CTX_HOME=/ctx/home/$(id -u); api=$CTX_HOME/api/openai.chat; id=daily-$(date +%Y%m%d); printf %s "{\"messages\":[{\"role\":\"user\",\"content\":\"Summarize ~/todo.txt\"}]}" > "$api/inbox/$id.tmp"; mv "$api/inbox/$id.tmp" "$api/inbox/$id.req.json"'
```

Read the result through the same outbox and audit files:

```bash
cat /ctx/home/$(id -u)/api/openai.chat/outbox/<id>.resp.json
cat /ctx/audit/events.jsonl
```
