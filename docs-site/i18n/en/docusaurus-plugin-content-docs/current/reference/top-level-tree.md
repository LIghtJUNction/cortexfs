---
title: Top-level Tree
---

# Top-level Tree

Current top-level ABI:

```text
/
  status
  cap/
  format/
  provider/
  model/
  home/
  group/
  shared/
  ext/
  space/
  agent/
  cluster/
  mcp/
  skill/
  tool/
  memory/
  vector/
  db/
  audit/
  control/
```

## Meaning

```text
status        global status
cap/          global capability lists
format/       API protocol formats
provider/     backend providers and account instances
model/        global model index
home/         user entries similar to /home
group/        local group entries
shared/       shared project and collaboration entries
ext/          external platform entries
space/        policy view
agent/        agent definitions, runtime, and collaboration
cluster/      agent and worker clusters
mcp/          MCP servers, tools, resources, and prompts
skill/        skill registry and skill content projection
tool/         Cortex-native and external tool projection
memory/       global memory and index entry
vector/       vector database backends
db/           structured database backends such as PostgreSQL/SQLite
audit/        global audit view
control/      global control nodes
```

CortexFS does not expose `/ctx/chan`, `home/<uid>/job`, `home/<uid>/hook`, or
`workflow/`. Relays and account instances belong under `provider/`; routing
belongs under `home/<uid>/route/`; external tasks and triggers submit through
generic inboxes by rename. The mounted tree is not an extensible data
directory; `mkdir` on undeclared ABI directories returns EROFS.
