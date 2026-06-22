# Context Runtime ABI

Context is a working set, not full history. Raw session history is durable.
Prompt context is disposable and rebuildable.

This spec does not add root directories. Context state lives under the
owning session:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/context/
```

## Session Layout

A durable agent session may contain:

```text
messages.jsonl
events.jsonl
latest.md
state
cwd
created_at
updated_at
meta.json

context/
  budget
  pack.json
  pack.md
  summary.md
  facts.jsonl
  decisions.jsonl
  todo.md
  refs.jsonl
  pinned/
  swap/
  dedup/
  child/
```

Meaning:

```text
messages.jsonl   append-only logical conversation history
events.jsonl     tool calls, child agents, errors, and state changes
context/         rebuildable context working area for this session
```

`messages.jsonl` is truth. `context/` is derived state.

Rules:

```text
messages.jsonl MUST be append-only logical history.
events.jsonl SHOULD be append-only runtime history.
context/* MAY be rebuilt.
summary.md is derived.
pack.md is derived.
dedup indexes are derived.
swap chunks are derived or referenced.
```

Compression must not replace or delete raw messages. A summary is never the
only copy of history.

## Context Pack

Before a model call, an agent builds a context pack. The pack is the current
prompt working set and may be rebuilt at any time.

```text
context/pack.json
context/pack.md
```

Example `pack.json`:

```json
{
  "session": "default",
  "agent": "coder",
  "budget_tokens": 32000,
  "items": [
    {
      "kind": "system",
      "source": "context/pinned/system.md",
      "tokens": 1200
    },
    {
      "kind": "summary",
      "source": "context/summary.md",
      "tokens": 1800
    },
    {
      "kind": "facts",
      "source": "context/facts.jsonl",
      "tokens": 900
    },
    {
      "kind": "recent_messages",
      "source": "messages.jsonl",
      "range": "tail:40",
      "tokens": 6000
    },
    {
      "kind": "child_result",
      "source": "context/child/rev-123/result.md",
      "tokens": 1000
    }
  ]
}
```

Rules:

```text
pack is a working set.
pack is not ABI truth.
pack items SHOULD identify their source.
pack generation SHOULD respect context/budget when present.
```

The runtime or agent should make it possible to inspect what was sent to the
model by reading the generated pack.

## Compression

Compression creates typed derived files instead of replacing history.

```text
context/summary.md
context/facts.jsonl
context/decisions.jsonl
context/todo.md
context/refs.jsonl
```

Kinds:

```text
summary    current task summary
facts      stable facts
decisions  accepted decisions
todo       open work
refs       files, artifacts, tool outputs, and swap references
```

Example `facts.jsonl`:

```jsonl
{"id":"f1","text":"CortexFS root contains status, bin, model, agent, tool, home, shared.","source":"messages:12-18"}
{"id":"f2","text":"Model ABI must not expose provider API formats.","source":"messages:80-96"}
```

Example `decisions.jsonl`:

```jsonl
{"id":"d1","decision":"DESIGN.md is an index; normative details live under spec/.","source":"messages:200-230"}
{"id":"d2","decision":"Child agents are owned and die with the parent by default.","source":"user:latest"}
```

Example `refs.jsonl`:

```jsonl
{"id":"r1","path":"/work/DESIGN.md","kind":"file","summary":"current design draft"}
{"id":"r2","path":"context/swap/chunk/sha256-abc","kind":"swap","summary":"old discussion about model ABI"}
```

## Swap

Swap means content is out of the current prompt window, not deleted from
storage.

Recommended shape:

```text
context/swap/
  index.jsonl
  chunk/
    sha256-abc
    sha256-def
```

Example `index.jsonl`:

```jsonl
{"id":"sha256-abc","kind":"message_range","source":"messages.jsonl","range":"1-120","summary":"initial design discussion","tokens":18000}
{"id":"sha256-def","kind":"tool_output","source":"events.jsonl","event":"ev-991","summary":"full test log from failed build","tokens":45000}
```

Swap chunks SHOULD prefer references to raw history when a stable reference is
enough:

```json
{
  "source": "messages.jsonl",
  "start_event": "msg-001",
  "end_event": "msg-120",
  "hash": "sha256:..."
}
```

## Dedup

Dedup uses content-addressed chunks or references. It does not require a
database.

```text
context/dedup/
  blob/
    sha256-abc
    sha256-def
  index.jsonl
```

Example `index.jsonl`:

```jsonl
{"hash":"sha256-abc","refs":["messages:1-40","swap:old-design"],"bytes":12000,"tokens":3000}
{"hash":"sha256-def","refs":["tool:test-output-1","tool:test-output-2"],"bytes":80000,"tokens":20000}
```

Rules:

```text
large repeated tool output SHOULD be referenced instead of copied.
large repeated file content SHOULD be referenced instead of copied.
child results SHOULD be merged as result refs, summaries, and artifacts.
pack items MAY reference hashes and refs that are expanded only when selected.
```

## Context GC

Garbage collection may remove derived cache. It must not remove durable
history unless an explicit archive or delete policy says so.

Rules:

```text
MAY delete context/pack.*
MAY delete unreferenced context/dedup/blob entries.
MAY delete context/swap chunks that are rebuildable.
MUST NOT delete messages.jsonl unless explicit archive/delete policy allows it.
MUST NOT delete events.jsonl unless explicit archive/delete policy allows it.
MUST NOT delete a child result while parent context references it.
```

Optional policy file:

```text
context/gc.policy
```

Example:

```text
pack_ttl=1d
swap_ttl=30d
dedup_min_refs=1
keep_messages=1
keep_events=1
```

## Summary Rules

```text
1. Raw session history is durable and append-only.
2. Model context is a rebuildable working set.
3. Compression creates derived files, not replacement history.
4. Swap removes content from the prompt window, not from storage.
5. Dedup uses content-addressed chunks or refs.
6. Context files stay under the owning session, not the root ABI.
```
