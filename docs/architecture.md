# CortexFS Architecture

Normative ABI detail lives under [spec/](spec/). Visual identity lives in
[DESIGN.md](DESIGN.md) (Google Labs DESIGN.md format). This file is the
engineering design entry: what CortexFS is, where state lives, and what must
not become root ABI.

## One-page model

```text
/ctx is a FUSE filesystem interface for agent runtimes.
model is a pure inference file.
agent is the policy-bound orchestrator.
tool is a capability endpoint.
session is ordinary file history.
policy is a minimal SELinux-like allowlist.
CortexFS protocol adapters remove provider and API-format differences.
CortexFS does not express provider/API formats as root ABI.
MCP servers are tool sources; MCP capabilities are ordinary tools.
CortexFS controls agent visibility, execution, and sharing, not framework config formats.
```

## Frozen root rule

```text
root only contains stable object classes
root never mirrors provider, database, workflow, memory, or orchestration internals
MCP must not become a root namespace
MCP configs, skills, project rules, and prompt packages are ordinary visible files
```

Forbidden root namespaces (examples):

```text
skill/  memory/  mcp/  workflow/  chan/  job/  hook/  audit/  control/
```

Those concepts may exist as object-local files, session data, or tools. They
must not become new root classes.

## Core invariants

```text
Context is a working set, not the full history.
Raw history is durable.
Prompt context is disposable and rebuildable.
Compaction must not destroy raw messages.
Independent tasks should run in child agents.
Child agents are owned by their parent unless explicitly detached by policy.
Owned children die when the parent dies.
Prompt text and skill metadata never grant authority.
Policy, path, mount, uid/gid, and mode bits grant authority.
Mechanism enforces principal, path, mount, and Linux constraints; an injected
policy evaluator may only further restrict that authority.
```

## Identity, lifetime, and transport

CortexFS uses four different identities. They must not be collapsed into an
"agent daemon" or duplicated in a second lifecycle tree:

| Layer | Stable identity | Owner |
| --- | --- | --- |
| Definition | `agent/<name>` + `agent/<name>.d/` | reference tree |
| Runtime instance | supervisor unit + invocation receipt | runtime/supervisor |
| Session | `home/<uid>/agent/<name>/session/<session>/` | durable files |
| Run | entropy-backed run id in session events | session recorder |

The definition says how an Agent may run. A runtime instance says which
processes currently realize that definition. A session owns durable human and
Agent history. A run correlates one bounded execution inside that session.

`agent/<name>.d/meta.json` may retain the latest receipt-bound supervisor facts
needed for inspection and safe cleanup. `status`, `pid`, and `log` are summary
projections. None of those files changes the Agent's definition identity, and
none is an independent process supervisor.

Do not add `instances/` merely to mirror process state already owned by systemd
and receipt metadata. A future multi-instance feature must first define an
identity that cannot be expressed by the existing agent/session/unit/run tuple,
then nominate one authoritative lifecycle owner and migration path.

Sockets are transports. Live sockets belong under `/run`; paths such as
`agent/<name>.sock` and `session/<session>/terminal/main.sock` are stable ABI
entries or aliases used to discover those transports. Socket presence does not
define object identity, session durability, or process ownership.

Frontend interaction uses the existing Agent/session socket as its canonical
runtime boundary. `cortexfs-runtime-client` names that logical contract
`cortexfs.interaction/v1`; terminal, web, and IM clients share request/event
semantics and correlation ids. `cortexfs-channels` has a separate
`cortexfs.channel.socket/v1` driver boundary for platform lifecycle, delivery,
receipts, and live effects. Neither contract adds a `/ctx/interaction` or
`/ctx/channel` root namespace, and platform-specific message types do not cross
the Agent boundary.

Heavy or OS-specific transports remain external processes on that boundary.
For example, `cortexfs-channel-nostr` owns relay WebSockets and NIP-04/NIP-17
cryptography while the core runtime only sees provider-neutral channel frames.
This keeps a small host from loading every platform SDK into one process and
makes per-channel restart and memory limits explicit.

A terminal is a durable resource below a session. Its resource directory owns
metadata and replayable events; a runtime PTY and socket are replaceable
mechanisms for the process and attachments. The first terminal resource slice
uses:

~~~text
home/<uid>/agent/<agent>/session/<session>/terminal/<terminal-id>/
  meta.json  state  status  owner  cwd  events.jsonl
~~~

The root remains frozen: this session-local path does not add /ctx/terminal.
A top-level terminal class requires a separately versioned root ABI decision.

Interactive frontends use the same session-local discovery rule. Each durable
terminal, web, or external channel is represented by a filename below
`home/<uid>/agent/<agent>/session/index/channel/` (or the corresponding
shared-space session index). The filename is the user-facing channel selector;
its JSON content only maps a generic transport to an existing agent/session.
This keeps `ctx attach` discoverable with `ls` while preserving the frozen root
ABI and the existing interaction socket.

The compact rule is:

```text
object defines identity
supervisor receipt defines process lifetime
ordinary files define durable state
socket provides optional transport
terminal resource owns PTY history; socket is only a live transport
```

Agent session runtime state has two compatible projections. The existing
single-line `state` file remains the lifecycle compatibility surface; the
optional `state.json` file is a structured, non-secret view of status, phase,
run, step, selected model, context revision, and stable error code. Runtime
transitions update both through the existing atomic file replacement helper.
Clients inspect that projection through the agent/session socket's `status`
request and replay facts through `resume`; no watcher or second root control
plane is introduced.

During a hosted Agent step, `context_revision` is a length-delimited SHA-256
digest of the bounded history, tool context, and previous observation inputs.
It lets a status client detect a rebuilt working set without exposing prompt,
message, tool-result, or credential contents.

## Model and context boundary

A model object is the stable provider/model identity. Its `driver` control
selects replaceable adapters for each use case; `cap`, `limit`, `recommended`,
and `compact` project only provider-neutral facts. Agents and context code
consume those projections and must not branch on provider names, API formats,
or model branding.

Capability data is conservative. Hard limits use the precedence defined by the
Model ABI: explicit per-model host configuration, then the validated catalog,
then `unknown`. Stable `cap` words are adapter projections; unsupported or
untrusted facts are omitted. A future per-model capability override or
host-side probe requires a versioned Model ABI change. It must not become a
model-call side effect, a background watcher, or a second configuration store;
accepted evidence would enter the same validated `cap`/`limit` projection or
remain diagnostic-only.

Context construction uses the selected model's hard `limit`, metadata
`recommended`/`compact` policy, and the Agent's attenuating `window`/`compact`
controls. `window=auto` follows the model recommendation, not necessarily the
maximum advertised window. Raw session history remains intact while the
rendered prompt may use a recent tail, summaries, rules, skills, and loaded tool
metadata. Changing models therefore rebuilds prompt context from durable facts;
it does not rewrite history or teach each Agent a table of model-specific cases.

## Runtime module boundary

`cortexfs-module` is the shared static Rust module API plus the versioned
external module wire contract. It defines provider-neutral metadata,
capability declarations, executor-independent async lifecycle methods, and a
deterministic static registry for Agent, Tool, Channel, Model, and Context
modules. The crate is independent of FUSE, `/ctx`, provider protocols, and
runtime storage; domain SDKs add their typed behavior above this boundary.

The Rust trait API is intentionally statically composed. A Rust trait object is
not promised as a `cdylib` ABI. The recommended third-party boundary is the
`cortexfs.module.socket/v1` JSONL-over-Unix-socket contract, which preserves
compiler and allocator independence and adds process isolation while keeping
the same lifecycle and capability model.

## Where things live

The packaged host keeps versioned durable trees under
`/var/lib/cortexfs/storage/generations/<generation>` and exposes the selected
tree through the atomic `/var/lib/cortexfs/storage/current` symlink. On a
systemd restart, `ctx storage update` clones the current generation, applies
and validates the next `bin/cortexfs.bootstrap.json` `tree_version`, then
switches `current`. A failed stage leaves `current` unchanged. This is a
restart boundary, not a watcher, poller, or hot reload;
the `/ctx` ABI shape remains unchanged. The package generates root files
locally; generations are not distributed artifacts. The systemd restart path,
after stopping consumers, explicitly uses `--prune` to remove non-current
generations. There is no background generation GC.
The mount and agent runtime resolve `current` once at process startup and keep
that concrete generation for their full lifetime, including mount cache
refresh. Short-lived object-runner invocations may resolve the then-current
generation each time.

| Place | Path shape | Role |
| --- | --- | --- |
| Control | `/ctx/agent/<name>.d/*` | policy, mount, cwd, system.md, loop |
| Agent home | `/ctx/home/<uid>/agent/<name>/` | session, data, cache, log |
| Session | `.../session/<session>/` | messages, events, state.json, context, load snapshots |
| Runtime IPC | `/run/user/<uid>/cortexfs/...` | terminal sockets only |

Sandbox mapping (typical):

```text
/ctx/home/<uid>/agent/<name>  →  HOME=/home/agent   (rw)
caller project cwd            →  /workspace         (rw, default cwd)
/ctx                          →  /ctx               (often ro)
```

`/run` holds sockets. Agent cwd is usually `/workspace`. Private session files
live under agent home, not under `/run`.

## Prompt load observability

When the object runner builds a run prompt, it best-effort writes:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/AGENTS.md
/ctx/home/<uid>/agent/<agent>/session/<session>/SKILLS.md
```

```text
AGENTS.md   merged rules snapshot (same text as {{rules}})
SKILLS.md   skill metadata only (name, description, path)
```

Ordinary session files, not authority. Full skill bodies stay at listed
`SKILL.md` paths. Implementation: `agent/prompt/snapshot.rs`.

## Engineering taste

```text
short names over long phrases
one clear job per module
reuse before inventing helpers
no parallel enums for Empty/Missing/Invalid
no second root ABI for orchestration
no background watchers, polling, or hot-reload subcommands
Git commit (or process restart) is the development refresh boundary
atomic rename for control-plane writes
ordinary files for history and snapshots
```

Module naming: [naming-guide.md](naming-guide.md). Prefer single-token stems
(`snapshot.rs`); no new `-` / `_` in module file stems.

## Internal code architecture

Product rules above freeze **what** `/ctx` is. How the Rust tree is layered
(process roles, crate/feature splits, module dependency direction, error
tiers, migration phases) lives in
[internal-architecture.md](internal-architecture.md).

Read that document before large refactors (crate splits, executor error
migrations, FUSE vs object boundary changes). Do not “improve structure” by
adding root ABI classes, workflow engines, or background watchers.

## Read the specs in order

```text
spec/README.md
spec/root-abi.md
spec/fuse.md
spec/object-abi.md
spec/model-abi.md
spec/session-abi.md
spec/agent-tool-security.md
spec/agent-runtime.md
spec/module-abi.md
spec/tool-policy-abi.md
spec/ctx-coreutils.md
spec/rolling-upgrades.md
```

## Stable ABI red line

```text
Do not let /ctx become a directory mirror of an AI platform database.
It should stay small, hard, boring, and scriptable.
```
