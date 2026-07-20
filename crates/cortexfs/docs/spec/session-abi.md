# Session ABI

Socket requests must include `session`. If the client omits it, runtime uses
`default`.

Request:

```jsonl
{"op":"send","id":"client-msg-id","session":"default","scope":"private","cwd":"/workspace","input":"hello"}
```

## Durable Run IDs

Each production durable `send`, `chat`, and `repl` run ID is independently
generated from 128 bits of Linux system entropy and encoded as `ctx-` followed
by exactly 32 lowercase hexadecimal characters. The probability of accidental
reuse or collision is negligible. Short `r1`, `run-1`, and `msg-1` values in
examples and tests are illustrative or local labels only, not
production-generated durable IDs.

Within one session, retrying `send` with the same client `id`, input, scope,
and effective `cwd` replays the original `start` or recorded final `done`.
Replay does not execute the agent and appends no message, event, or index fact.
Reusing an `id` with a different payload returns `EINVAL`. Malformed JSONL or
a final line without its terminating newline returns `EIO`; an implementation
must not append to or reuse an unprovable history claim.

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

Tool results must carry a run and a call id matching a canonical `tool_call`
event. Projection drops unmatched results and never synthesizes a tool call or
chat message.

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
session names such as `list`, `current`, `by-cwd`, `by-hash`, or `by-uuid`.

```text
session/
  index/
    list
    current
    by-cwd/
      <hash>
    by-hash/
      <hash>
    by-uuid/
      <uuid>
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
index/by-hash/<hash>  single value, session name for that external hash
index/by-uuid/<uuid>  single value, session name for that external uuid
```

`index/by-cwd/<hash>`, `index/by-hash/<hash>`, and `index/by-uuid/<uuid>` are
not symlinks. That keeps the ABI identical across mounts and different backing
stores.

Session garbage collection defaults to a no-write preview. Applying it with
`--yes` archives each eligible live session by same-filesystem
`RENAME_NOREPLACE` to
`<CTX_HOME>/archived_sessions/<agent>/<session>` and removes exact
references to that session from `index/list`, `index/by-cwd/`,
`index/by-hash/`, and `index/by-uuid/`. The archive destination never
overwrites an existing entry. Permanent deletion is opt-in and requires
`--delete --yes`; `--delete` without `--yes` only changes the preview mode.
`--archive-dir <absolute-path>` replaces the default archive root, must not
overlap the live session tree, and is invalid with `--delete`.

`ctx agent session archive <agent> <session> [--archive-dir <absolute-path>]`
applies the same lock, index claim, source claim, no-replace rename, and
rollback rules immediately to exactly one eligible session. The archived
directory preserves the complete original session tree, including raw
`messages.jsonl` and `events.jsonl`, without reserialization.

`default`, `index/current`, explicit `--keep` names, and sessions whose plain,
bounded `state` value is `active` are protected. A missing `state` remains
compatible with legacy sessions; unsafe or unreadable state entries are
conservatively protected. GC selects only live session directories and never
selects archived entries for a second operation. `archived_sessions` is an
external home directory, not a new root ABI namespace. Destination conflicts
or cross-filesystem renames fail without removing the live source, and no
recursive copy fallback is allowed. This phase defines no restore command.

Resume is not a root-level feature. Clients read the session index for the
current agent:

```text
/ctx/home/1000/agent/coder/session/index/list
/ctx/home/1000/agent/coder/session/index/current
/ctx/home/1000/agent/coder/session/index/by-cwd/<hash>
/ctx/home/1000/agent/coder/session/index/by-hash/<hash>
/ctx/home/1000/agent/coder/session/index/by-uuid/<uuid>
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

See `context-abi.md` for context packs, compression, swap, and dedup rules.
