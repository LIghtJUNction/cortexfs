---
title: Threads and Batch
---

# Threads and Batch

Threads are persistent conversation contexts. Batch queues handle multiple
requests.

```text
home/<uid>/thread/<id>/
  inbox/
  io.sock
  messages.jsonl
  latest.md
  state
  fingerprint
  control/
```

Submit a thread request:

```bash
thread="$CTX_HOME/thread/demo"
printf '%s\n' '{"messages":[{"role":"user","content":"continue"}]}' > "$thread/inbox/0001.tmp"
mv "$thread/inbox/0001.tmp" "$thread/inbox/0001.req.json"
```

The socket fast path must enter the same policy, route, store, audit, and export
pipeline as file submission.
