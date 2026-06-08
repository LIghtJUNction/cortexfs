---
title: Spaces and Security
---

# Spaces and Security

A space is the boundary for policy, audit, memory, and execution.

```text
home/<uid>/
  policy/
  route/
  model/
  api/
  thread/
  memory/
  export/
```

`home/<uid>` is the work entry. `space/` is a read-only security index, not a
second submission entry.

Security decisions should consider:

- `HostActor`: Linux `uid/gid/pid`.
- `Subject`: represented external user, such as `qq:user:123456`.
- `Object`: provider, model, tool, memory, thread, or file object.

Secrets, OAuth tokens, and session tokens do not enter the mounted tree.
