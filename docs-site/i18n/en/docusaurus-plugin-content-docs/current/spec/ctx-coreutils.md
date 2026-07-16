# ctx Coreutils

`ctx` is the thin userland client for the `/ctx` ABI.

```text
ctx = CortexFS coreutils
```

It is not an AI chat product. It is not a daemon. It is not a runtime. It is
not a provider SDK. Its first job is to prove that the CortexFS ABI is usable
with ordinary Unix-shaped operations.

`ctx` operates on `/ctx`. It does not have to live inside `/ctx`. Install it in
the normal system `PATH`, for example `/usr/bin/ctx` or `~/.local/bin/ctx`.

## v1 Commands

Keep v1 small:

```text
ctx status
ctx abi
ctx env
ctx root
ctx bootstrap
ctx mount

ctx ls
ctx ls model
ctx ls agent
ctx ls tool
ctx ls home
ctx ls shared/project-a

ctx which model openai/gpt-5.6
ctx which agent coder
ctx which tool fs.read

ctx path shared project-a
ctx agent history coder
ctx agent output coder
ctx agent resume coder --session default
ctx agent wait coder work-123 --session default

ctx agent new reviewer --model openai/gpt-5.6 --tool fs.read
ctx agent new reviewer --label reviewer_t --shared project-a:read --mount /work /work ro
ctx agent start reviewer
ctx agent stop reviewer
ctx agent status reviewer
ctx agent env reviewer
ctx agent ps

ctx cat agent/coder.d/policy
ctx set agent/coder.d/cwd /work
ctx append agent/coder.d/path /ctx/tool
ctx file agent/coder.d/mount
ctx file type tool/fs.read
ctx file check agent/coder.d/mount
ctx schedule status home/1000/agent/coder/session/default/context/plan.json --done plan
ctx schedule advance home/1000/agent/coder/session/default/context/plan.json --done plan
ctx schedule claim home/1000/agent/coder/session/default/context/plan.json work-123
ctx schedule result home/1000/agent/coder/session/default/context/plan.json work-123 done "implemented"

ctx validate-name coder
ctx doctor
```

Do not add:

```text
ctx provider
ctx mcp registry
ctx workflow
ctx memory
ctx vector
ctx cluster
```

Socket conveniences such as `ctx send`, `ctx chat`, `ctx connect`, `ctx ping`,
and `ctx cancel` may exist, but they must be thin wrappers over the same socket
ABI.

`ctx bootstrap [SOURCE]` updates the reference source tree only; it does not
remount `/ctx`, start a watcher, or add a second refresh boundary.

Optional flags:

```text
ctx bootstrap --check [SOURCE]     report tree_version, missing agents, retired leftovers
ctx bootstrap --dry-run [SOURCE]   show would_ensure / would_skip / would_write (no writes)
```

Default bootstrap materializes `architect` / `coder` / `reviewer` and
writes `bin/cortexfs.bootstrap.json` (`schema`, `tree_version`,
`managed_agents`, `applied_migrations`) only when state differs. Retired
`base` / `worker` / `executor` objects are reported and retained for
manual review because legacy trees have no manifest proving ownership and full
control-tree integrity.

Top-level agent session shortcuts follow the same current-session default as
their `ctx agent ...` forms:

```text
ctx history AGENT
ctx history AGENT --session SESSION
ctx resume AGENT
ctx resume AGENT --session SESSION
ctx send AGENT INPUT
ctx send AGENT --session SESSION INPUT
ctx agent wait AGENT CHILD [--session SESSION]
```

Omitting the session reads `session/index/current` first and falls back to
`default`.
`ctx send` and `ctx resume` render assistant events the same way as
`ctx agent send` and `ctx agent resume`; raw socket JSONL is reserved for lower
level socket commands and explicit raw agent modes.

`ctx agent wait` is a non-blocking waitpid-shaped reader for a parent-owned
child result channel. It reads `context/child/<child>/status`; `pending` and
`active` fail with service unavailable, while terminal `done`, `error`, and
`cancelled` print
`child<TAB>status<TAB>agent<TAB>session<TAB>model<TAB>life` followed by
`result.md`. Its process exit status follows the child status: `done` exits 0,
`error` exits 1, and `cancelled` exits 130. It does not poll, start runtimes,
reap history, or delete child state.

Hybrid parent schedules use an explicit single-step command:

```text
ctx schedule status PATH [--done NODE]...
ctx schedule advance PATH [--done NODE]...
ctx schedule claim PATH CHILD
ctx schedule result PATH CHILD done|error|cancelled RESULT [--refs-jsonl JSONL]
```

`PATH` must be an agent session `context/plan.json`. The command reads the
parent agent label and policy, derives completed delegated nodes from
`context/child/<child>/status`, applies any explicit local `--done` node ids,
and materializes newly ready delegated handoffs under `context/child/<child>/`.
`ctx schedule status` is a read-only table over the same state. It prints
`node<TAB>kind<TAB>agent<TAB>child<TAB>session<TAB>model<TAB>life<TAB>state`, where state is one of
`blocked`, `ready`, `pending`, `active`, `done`, `error`, or `cancelled`.
The session, model, and life columns are `-` for local parent nodes. Delegated
child nodes show the explicit child session, or the inherited parent session
when the schedule node omits one, plus the selected backing agent model and
lifecycle.
For delegated nodes, the backing agent must exist as both `agent/<name>` and
`agent/<name>.d/`; schedule commands must not invent `main`/`owned` defaults for
a missing worker object.
Each emitted `handoff` line includes the child `agent`, `session`, selected
`model` from `agent/<name>.d/model`, `life` from `agent/<name>.d/life`, a
shell-quoted `parent='agent:<name> session:<session>'` reference, and the
stable `handoff`, `result`, and `refs`
ABI file paths under `context/child/<child>/`. A parent can hand these paths to
a worker without guessing where the worker should read input, which spark model
path and lifecycle it should use, or where it should write compact results.
`ctx schedule claim` marks a materialized child channel `active` when a worker
has claimed the handoff. It is a single status-file transition from `pending`
to `active`, idempotent while active, and it does not start a runtime. Its
output line includes the claimed child `agent`, `session`, backing `model`,
backing `life`, parent reference, and the same stable `handoff`, `result`, and
`refs` paths.
`ctx schedule result` writes a terminal child result back to the same parent
session child channel: `status`, `result.md`, and `refs.jsonl`. Its output line
includes the child `agent`, `session`, backing `model`, backing `life`, parent
reference, and the written `result` and `refs` paths.
Neither command starts agents, loops in the background, polls, or creates a
second submission namespace.

Agent lifecycle conveniences exist as thin wrappers:

```text
ctx agent new NAME [--temp] [--parent PARENT] [--label LABEL] [--model MODEL] [--tool TOOL] [--shared NAME:read|write] [--mount SOURCE TARGET ro|rw]
ctx agent new [NAME] --from PROFILE
ctx agent apply NAME --from PROFILE
ctx agent start NAME
ctx agent stop NAME
ctx agent status NAME
ctx agent env NAME
ctx agent ps
ctx agent children NAME
ctx agent wait NAME CHILD
```

`ctx agent new` must call `/ctx/tool/agent.create` when that tool exists. If the
tool is absent, host-side `ctx` may create a standard agent object directly by
writing `agent/<name>.d/*` controls and `home/<uid>/agent/<name>/` skeleton
directories; this fallback is a supervisor operation, not an agent policy
grant. `ctx agent new --temp` records `life=temp` in either path. `--parent`
records the ordinary `agent/<name>.d/parent` control value, such as
`agent:coder session:default run:r1`, so a created worker child has a
wait/stop-visible parent without adding a separate process table.

`--from` accepts a host-side `agent.yaml` file, a directory containing one,
or a short profile name. New/apply validates profile fields before materializing
them into ordinary `.d/*` controls. Apply preserves unspecified controls and
unknown `meta.json` object keys; it rejects symlink controls and invalid
profile or metadata before writing.

`ctx agent start` starts the explicit runtime for an existing agent. After the
runtime terminal socket is reachable, host-side `ctx` writes
`agent/<name>.d/status` to `ready` and appends an `agent.start` event to
`agent/<name>.d/log`. `pid` remains numeric-only; systemd invocation ids are log
facts, not `pid` values.

`ctx agent stop` calls `/ctx/tool/agent.stop` when that tool exists. If the tool
is absent, host-side `ctx` may perform a supervisor stop by writing
`agent/<name>.d/status` to `dead`, clearing `agent/<name>.d/pid`, and appending
an `agent.stop` event to `agent/<name>.d/log`. The same supervisor fallback also
marks any existing `owned` or `temp` child agents whose `parent` points at the
stopped agent as cancelled/dead, recursively, while leaving their history and
control objects inspectable. When the child agent is the backing runtime for a
pending or active parent `context/child/<child>/` channel, the fallback records
that parent-side child result as `cancelled` so `ctx agent wait` observes the
terminal state. It must not invent a new lifecycle namespace or queue.
Retired reference agents `base`, `worker`, and `executor` are manual-review
objects: child discovery excludes them before reading legacy ownership fields,
so stop cascade never changes their controls, status, or child result channels.
Before any unit reset or control write, fallback stop validates the complete
non-retired descendant plan, detects ownership cycles, and preflights planned
controls and existing pending/active child-result channels. It executes the
validated plan in post-order, descendants before their parent.
`ctx agent status` reads ordinary `agent/<name>.d/*` controls and prints the
status value first, followed by `model=`, `life=`, `parent=`, `children=`,
`pid=`, `uid=`, `gid=`, `groups=`, `root=`, and `cwd=` lines.
This keeps the first line usable as the process state while exposing the
backing model, direct child count, parent relationship, Linux identity, and
chroot/cwd for worker inspection. The `children=` count includes direct child
agents whose effective state is not `dead`; recorded `ready` or `busy` children
with stale numeric pids are excluded the same way as `ctx agent ps`. `ctx agent ps` may
read `agent/<name>.d/parent`,
`model`, `life`, `status`, and `pid` directly and print the current agent tree.
Default `main` model selections and `owned` lifecycles may stay implicit;
non-default worker models and non-owned lifecycles should be visible in the
tree.
`ctx agent env` derives the same runtime view as `ctx agent start` and prints
the sandbox environment as `KEY=value` lines. It is a read-only inspection of
existing control files, not a way to inherit host variables or mutate runtime
state.
`ctx agent children` reads the parent session `context/child/<child>/` table and
prints tab-separated `child`, child-channel `status`, backing `agent`,
child-channel `session`, backing agent `parent_session`, backing agent `model`,
backing agent `life`, backing agent `status`, and backing agent `pid` (`-` when
absent). The last five backing-agent columns are ordinary
`agent/<agent>.d/parent`, `model`, `life`, `status`, and `pid` controls, so
worker task state and its parent-session attachment are
inspectable without copying runtime state into the parent context.
`ctx agent wait` reads the same child channel. If a child is still `active` but
its backing agent's effective state is `dead`, has no live `pid`, and still
points back to the waiting parent agent/session, `wait` records the child
channel as `cancelled` and returns the cancellation exit code. A recorded
`ready` or `busy` state with a numeric `pid` that is absent from `/proc` is
treated as no live `pid` for this read. Terminal output uses the same
child-channel `agent`, `session`, backing `model`, and backing `life` fields as
`ctx agent children`, then prints `result.md`. This is a synchronous reap, not
a background poller.

## Installation Boundary

Reserve `/ctx/bin` for CortexFS ABI-level helper programs needed by runtimes,
chroots, or scripts:

```text
/ctx/bin/ctx
/ctx/bin/ctxterm
/ctx/bin/tsh
```

The first implementation may expose only `ctx`, but agent terminal runtimes
should use `ctxterm` and `tsh` when present. The placement rule is:

```text
human CLI              system PATH, usually one ctx binary
agent capability       /ctx/tool
runtime ABI helper     /ctx/bin
```

`ctxterm` is the agent terminal emulator. It owns the pseudo-terminal and starts
`tsh` by default. `tsh` is the tool shell that runs inside that terminal. `tsh`
resolves command names through `CTX_PATH`, not `PATH`, and must not execute
arbitrary host commands directly. A command such as `bash` works only when a
tool named `bash` is visible through `CTX_PATH`.

`ctx agent start <agent> --session <session>` starts the default agent
terminal in a sandbox. Unless overridden, the caller's current working
directory is bind-mounted at `/workspace` with read-write access. If that
directory contains `.git`, `.git` is over-mounted at `/workspace/.git` with
read-only access. The agent process starts with `/workspace` as its current
directory. The host path is therefore not exposed as the agent's `pwd`; the
agent sees the authorized project mount through the sandbox path. The sandbox
home is `/home/agent`, backed by `/ctx/home/<uid>/agent/<agent>`, so shell
state such as `.config`, `.cache`, and `.bash_history` does not land in the
project workspace.

The terminal process starts from an empty environment with a small allowlist
such as `CTX_ROOT`, `CTX_HOME`, `HOME=/home/agent`, `PATH=/usr/bin:/bin`,
`USER`, `LOGNAME`, `SHELL`, `TERM`, and `LANG`. Host session variables and
secrets are not inherited by default.

Additional mounts can be supplied explicitly:

```text
ctx agent start <agent> --session <session> \
  --mount /host/path /workspace rw \
  --mount /host/input /input ro \
  --cwd /workspace
```

`ctxterm --listen SOCKET` exposes the PTY for observation and attachment.
Session terminals use:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

The ABI socket may be a symlink to a runtime socket. User-started terminals
prefer `/run/user/<uid>/cortexfs/terminal/<agent>/<session>/main.sock` so
ordinary users do not need write access to `/ctx` or `/run/cortexfs`.
Existing installations may still expose
`/run/cortexfs/terminal/<uid>/<agent>/<session>/main.sock` as historical
artifacts, but `ctx agent attach` does not use this legacy fallback anymore.
`ctx agent attach` should try the ABI path first, then the user runtime path.
If both locations are unavailable, it returns a socket-availability error.

The corresponding human commands are:

```text
ctx agent watch <agent> --session <session>
ctx agent attach <agent> --session <session>
```

When auditing historical sessions, `terminal/main.sock` can appear as a broken
symlink after the session ends. A broken socket is often runtime residue, commonly
present in archived snapshots, and should not be treated as an ABI structural
regression by itself. You can identify intentionally stale session sockets
explicitly:

```bash
find /ctx/home/<uid>/agent -type l -path '*/session/*/terminal/main.sock' -print0 |
  while IFS= read -r -d '' sock; do
    [ -e "$sock" ] || printf 'BROKEN: %s\n' "$sock"
  done
```

### `/ctx` runtime drift quick-check

- List both supported socket forms for agents (symlink and direct socket):

```bash
find /ctx/agent -maxdepth 1 -type l -name '*.sock' -print
find /ctx/agent -maxdepth 1 -type s -name '*.sock' -print
```

- Check reachability for terminal sockets and identify stale links:

```bash
find /ctx/home/<uid>/agent -type l -path '*/terminal/main.sock' -print0 |
  while IFS= read -r -d '' sock; do
    if [ -e "$sock" ]; then echo "live:$sock"; else echo "stale:$sock"; fi
  done
```

- Inspect resolved runtime socket target for an agent path:

```bash
readlink -f "/ctx/agent/<agent>.sock" 2>/dev/null || stat "/ctx/agent/<agent>.sock"
```

For standalone human sessions, `tsh` reads `CTX_HOME/.tshrc` before inherited
process `CTX_PATH` when the file exists. The file is data-only and supports a
single stable setting:

```text
CTX_PATH=/ctx/tool:/ctx/home/<uid>/tool
```

Inside an agent terminal, `tsh` keeps the runtime-provided process `CTX_PATH`
authoritative.

Do not let `/ctx/bin` become a second `/usr/bin`.

## Path Model

`ctx` resolves paths under `CTX_ROOT`, defaulting to `/ctx`.

Examples:

```text
ctx ls agent
ctx cat model/openai/gpt-5.6.d/cap
ctx file type tool/fs.read
ctx exec agent/coder "fix tests"
```

Object strings use ABI path form:

```text
model/openai/gpt-5.6
agent/coder
tool/fs.read
```

## Core Commands

`ctx status` reads `/ctx/status`.

`ctx ls` uses `readdir`. It accepts an ABI path under `CTX_ROOT`, defaulting to
the root when no path is provided. It does not query a database, index,
registry, or daemon catalog.

`ctx which` finds executable objects by ABI class:

```text
ctx which model openai/gpt-5.6
ctx which agent coder
ctx which tool fs.read
```

`ctx tool NAME [ARG...]` is a narrow compatibility entrypoint for allowlisted
safe CortexFS core tool CLIs that are implemented inside the local `ctx`
binary, for example:

```text
ctx tool tsh.config
ctx tool tsh.config '{"max_loaded_tools":32}'
```

Before running an allowlisted core tool CLI, `ctx tool` still resolves `NAME`
through `CTX_PATH` so the visible ABI object exists. It must refuse ordinary
visible tools and authority-bearing core tools such as `fs.write` and
`shell.exec`; executing those directly from `CTX_PATH` would bypass CortexFS
tool authorization. Non-allowlisted tools are run through `tsh`, an agent
runtime, or another authorized execution path.

`ctx cat` reads ABI files. It should not interpret much.

`ctx set` updates by same-directory atomic replacement. `ctx append`
is only for appendable ABI files such as newline lists. `ctx file check`
validates path shape and file syntax where the ABI defines it.

`ctx file` inspects CortexFS paths using path shape, `stat`, `readlink`, and
read-only `user.cortexfs.*` extended attributes. It prints stable type strings,
projected byte size, token estimates, and available CortexFS xattrs. It does
not query a registry.

Stable type strings:

```text
ctx.model.exec
ctx.model.socket
ctx.model.control
ctx.agent.exec
ctx.agent.socket
ctx.agent.control
ctx.tool.exec
ctx.tool.socket
ctx.tool.control
ctx.session.dir
ctx.session.messages
ctx.session.events
ctx.shared.dir
ctx.shared.tool.exec
ctx.shared.tool.control
ctx.shared.queue
ctx.shared.result
ctx.home.dir
ctx.symlink
ctx.ordinary
ctx.unknown
```

## cd

An external process cannot change its parent shell cwd. `ctx cd` must not
pretend otherwise.

Correct ordinary usage:

```bash
cd "$(ctx path shared project-a)"
```

If `ctx cd` exists, it is a shell integration helper:

```bash
eval "$(ctx cd project-a --shell)"
```

## Sessions

`ctx agent history`, `ctx agent output`, `ctx agent trajectory`, and `ctx agent resume` read session files and connect to
the relevant socket. They do not keep a private chat database.

When `--session` is omitted, they use `session/index/current` first and fall
back to `default`. `ctx latest` is intentionally not a command; the current
session behavior belongs to `--session` omission.

Examples:

```text
ctx agent history coder
ctx agent output coder
ctx agent trajectory coder
ctx agent resume coder --session default
```

These commands read:

```text
/ctx/home/<uid>/agent/<agent>/session/index/list
/ctx/home/<uid>/agent/<agent>/session/index/current
/ctx/home/<uid>/agent/<agent>/session/<session>/latest.md
/ctx/home/<uid>/agent/<agent>/session/<session>/messages.jsonl
/ctx/home/<uid>/agent/<agent>/session/<session>/events.jsonl
```

`ctx agent trajectory` prints a validated ATIF projection. It correlates tool
calls, observations, and token usage by run/call identity and does not create a
second durable history. Only tool results carrying a run and a call id matching
a canonical `tool_call` event are projected; unmatched results are dropped.
Projection does not invent a tool call or chat message. If validation still fails, the CLI lists
actionable issue locations (step/result/call id), capped at 16 entries with the
remaining count reported. Session-derived source/call identifiers are escaped
for terminal output, field-bounded, and each rendered issue is capped at 256
characters.

## Provider OAuth

`ctx provider oauth` is a host-side credential helper. It does not add a
`/ctx/provider` namespace and does not expose tokens through model files.

```text
ctx provider oauth login PROVIDER [--timeout SECONDS]
ctx provider oauth status PROVIDER
ctx provider oauth refresh PROVIDER
```

`login` reads `/etc/cortexfs/providers.d/*.json`, uses the provider `oauth`
block, creates a PKCE `S256` authorization request, waits on the configured
localhost `redirect_uri`, exchanges the authorization code for tokens, and
stores tokens in the system secret store:

```text
service=cortexfs:<provider> account=oauth:access
service=cortexfs:<provider> account=oauth:refresh
```

## Non-Goals

`ctx` must not:

```text
expose provider keys through /ctx
store private chat history
implement tool calling
parse OpenAI/Anthropic/Gemini request formats
maintain an agent registry
modify messages.jsonl to fake chat
decide policy locally
fallback to another model
hide runtime errors behind product language
```

Those jobs belong to Rig, the agent runtime, tools, or the CortexFS ABI itself.
