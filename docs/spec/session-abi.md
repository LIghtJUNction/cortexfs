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
raw             read-only ABI alias of messages.jsonl; preserves original JSONL history
events.jsonl    tool calls, errors, and state changes
latest.md       latest assistant text
state           active, idle, done, error
state.json      structured non-secret runtime projection (optional for legacy sessions)
cwd             session working directory
created_at      creation time
updated_at      update time
meta.json       client, model, scope, and related metadata
AGENTS.md       optional run snapshot: effective merged AGENTS.md rules
SKILLS.md       optional run snapshot: discovered skill metadata only
context/        rebuildable prompt working set and derived context cache
```

`raw` is deliberately a file in the session object rather than a second history
store. Reading `/ctx/.../raw` returns the same durable message stream as
`messages.jsonl`; compaction, prompt compilation, and model changes must never
rewrite it. The projection is read-only and shares the backing stream, so it
does not double memory or disk usage.

For a session with model metadata, the read-only `raw` node also exposes:

```text
user.cortexfs.context_length           current raw token estimate
user.cortexfs.context_recommended      recommended Agent working window
user.cortexfs.context_compact_threshold compaction trigger
user.cortexfs.context_max              trusted model hard maximum
user.cortexfs.context_policy            model-metadata
```

The policy values come from `cortexfs-metadatas`; they describe the selected
model, while `agent/<name>.d/window` and `agent/<name>.d/compact` hold the
effective per-Agent choices. Missing model metadata is represented by
`unknown`, never by a guessed zero.

## Attach channel index

Attachable frontends are indexed as filenames below the existing session
index; this does not create a `/ctx/channel` root namespace:

```text
private  /ctx/home/<uid>/agent/<agent>/session/index/channel/<channel_name>
shared   /ctx/shared/<name>/agent/<agent>/session/index/channel/<channel_name>
```

`<channel_name>` is normalized as `transport[_instance]_<agent>_<session>`
with underscores only, for example `terminal_coder_default` or
`discord_primary_coder_discord_deadbeef`. A shared entry is prefixed with
`shared_`. The file contains provider-neutral JSON describing the target
agent/session and transport; credentials and external identities are never
stored there. `ctx attach` reads this index, accepts an exact name or unique
prefix, and uses the existing interaction socket for message channels or the
existing PTY socket for terminal channels. `ctx agent attach` remains the
explicit PTY operation.

Older durable sessions without an index entry receive a terminal entry when
`ctx attach` discovers them. The entry is written with the normal same-file
atomic replacement rules, so every channel shown by `ctx attach` has a real
filename that can also be listed with ordinary filesystem tools.

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
    channel/
      <channel_name>
  default/
    messages.jsonl
    events.jsonl
    latest.md
    state
    state.json
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
index/channel/<name>  provider-neutral JSON for an attachable frontend
```

`index/channel/` is a read-oriented discovery directory. Its regular files
are part of the durable session ABI and are visible through the existing
`home/<uid>` or `shared/<name>` tree. Runtime channel registration creates or
replaces them with the normal same-directory atomic rename rule. A channel
record is not a submission endpoint: clients use its filename to select the
existing agent/session socket, and must not put credentials or external
identity secrets in the record.

`index/by-cwd/<hash>`, `index/by-hash/<hash>`, and `index/by-uuid/<uuid>` are
not symlinks. That keeps the ABI identical across mounts and different backing
stores.

Session garbage collection defaults to a no-write preview. Applying it with
`--yes` archives each eligible live session by same-filesystem
`RENAME_NOREPLACE` to
`<CTX_HOME>/archived_sessions/<agent>/<session>` and removes exact
references to that session from `index/list`, `index/by-cwd/`,
`index/by-hash/`, `index/by-uuid/`, and `index/channel/`. The archive destination never
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

The short CLI exposes resume as a root-level client operation. `ctx resume`
selects the session whose `workspace` file matches the caller's current host
directory; `ctx resume <agent> --session <session>` selects an explicit
session. The durable session index remains the source for the agent's current
selection and fallback tooling:

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

Context-window limits, rebuildable prompt working sets, and context compaction
rules are defined in [agent-runtime.md](agent-runtime.md#context-window-control).
Child handoff channels and their durable result files are defined in
[ctx-coreutils.md](ctx-coreutils.md#core-commands).
