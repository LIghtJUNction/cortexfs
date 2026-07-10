# Session ABI

Socket requests must include `session`. If the client omits it, runtime uses
`default`.

Request:

```jsonl
{"op":"send","id":"client-msg-id","session":"default","scope":"private","cwd":"/workspace","input":"hello"}
```

`cwd` must be a path inside the agent chroot. If omitted, runtime uses
`agent/<name>.d/cwd`. If `cwd` does not exist, return `ENOENT`. If it exists
but is outside the visible mount/chroot, return `EACCES`. A client must not pass
a host absolute path to bypass the agent root.

`scope` has three values:

```text
private  default, private to the current Linux uid, resumable
shared   stored in shared space, visible to multiple agents or users when allowed
temp     temporary session, not required to survive socket close or agent exit
```

Agent session locations:

```text
private  /ctx/home/<uid>/agent/<agent>/session/<session>/
shared   /ctx/shared/<name>/agent/<agent>/session/<session>/
temp     no durable path required; may live only in process memory
```

Model session locations:

```text
private  /ctx/home/<uid>/model/<model>.d/session/<session>/
shared   /ctx/shared/<name>/model/<model>.d/session/<session>/
temp     no durable path required
```

## Session Directory

Session directories use ordinary files:

```text
messages.jsonl  conversation messages
events.jsonl    tool calls, errors, and state changes
latest.md       latest assistant text
state           active, idle, done, error
cwd             session working directory
created_at      creation time
updated_at      update time
meta.json       client, model, scope, and related metadata
AGENTS.md       optional run snapshot: effective merged AGENTS.md rules
SKILLS.md       optional run snapshot: discovered skill metadata only
context/        rebuildable prompt working set and derived context cache
```

`AGENTS.md` and `SKILLS.md` under the session directory are observability
snapshots written when the agent runtime builds the prompt for a run. They are
not required session layout files and must not grant authority.

```text
AGENTS.md  merged project + global AGENTS.md text injected as {{rules}}
SKILLS.md  skill catalog metadata only (name, description, SKILL.md path)
```

Full skill bodies stay in the original `SKILL.md` paths listed in `SKILLS.md`.
Snapshots are ordinary files replaced atomically on each run; older runs are not
versioned inside the session directory.

`ctx agent trajectory <agent> [--session <session>]` projects
`messages.jsonl` and `events.jsonl` to validated ATIF JSON on stdout. Event
`run` and tool-call ids remain the correlation authority for tool calls,
observations, and usage. The projection is derived output, not a second durable
history or submission path.

Legacy tool results may reference a call id that has no corresponding
`tool_call` event. Projection preserves non-empty result content, clears that
unproven `source_call_id`, drops empty unmatched results, and records their
count as `extra.legacy_unmatched_tool_results`. It does not synthesize a tool
call or chat message.

History is session files. Do not add `/ctx/history`.
Context runtime state stays under the session directory. Do not add
`/ctx/memory`, `/ctx/context`, `/ctx/swap`, or `/ctx/task`.

Users can inspect history with ordinary file operations:

```bash
ctx agent history coder
ctx agent output coder
less /ctx/home/$(id -u)/agent/coder/session/default/messages.jsonl
cat /ctx/home/$(id -u)/agent/coder/session/default/AGENTS.md
cat /ctx/home/$(id -u)/agent/coder/session/default/SKILLS.md
```

If `--session` is omitted, client commands resolve `session/index/current`
first and fall back to `default`. There is no separate `ctx latest` command.

## Session Index

Reserved index files live under `session/index/` to avoid colliding with user
session names such as `list`, `current`, or `by-cwd`.

```text
session/
  index/
    list
    current
    by-cwd/
      <hash>
  default/
    messages.jsonl
    events.jsonl
    latest.md
    state
    cwd
    created_at
    updated_at
    meta.json
```

Index file formats are fixed:

```text
index/list            one session name per line, newest updated_at first
index/current         single value, current session name
index/by-cwd/<hash>   single value, session name for that cwd
```

`index/by-cwd/<hash>` is not a symlink. That keeps the ABI identical across
mounts and different backing stores.

Resume is not a root-level feature. Clients read the session index for the
current agent:

```text
/ctx/home/1000/agent/coder/session/index/list
/ctx/home/1000/agent/coder/session/index/current
/ctx/home/1000/agent/coder/session/index/by-cwd/<hash>
```

Shared resume reads the matching index under `shared`. Temp sessions do not
appear in resume lists.

Durable sessions do not live in the chroot root:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/
```

The chroot root is only the runtime environment:

```text
/ctx/home/<uid>/agent/<agent>/root/
```

Rebuilding the root, cleaning it, or switching runtime environment must not
destroy session history.

See `16-context.md` for context packs, compression, swap, and dedup rules.
