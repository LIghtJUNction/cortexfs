# Child Agent Lifecycle ABI

Independent tasks should run in child agents. The parent agent should keep task
decomposition, global constraints, and result indexes. The child agent owns the
task process and returns a compact result.

The default durable lifecycle is owned:

```text
Child agents are owned by their parent unless explicitly detached by policy.
Owned children die when the parent dies.
```

v1 supports `owned` and `temp`. `detached` is reserved for a future explicit
policy grant and should not be exposed unless needed.

## Agent Control Files

A child is still an ordinary agent object:

```text
/ctx/agent/<child>
/ctx/agent/<child>.sock
/ctx/agent/<child>.d/
  parent
  life
  owner
  uid
  gid
  groups
  label
  iso
  root
  cwd
  env
  path
  mount
  model
  window
  policy
  status
  pid
  log
  meta.json
```

`parent` identifies the creating agent, session, and run when known:

```text
agent:coder session:default run:r123
```

The reference tree's default `worker` is parented by `agent:coder` without a
fixed session, so every coding session has the same inspectable spark worker
below it in the agent process tree. Host-side
`ctx agent new --parent 'agent:coder session:default run:r123'`
can still record a session-specific parent control value when the lifecycle
tool is absent. This lets a worker child agent be created with inspectable
parentage through the existing agent object controls.

`life` is one small text value:

```text
owned
temp
```

Future value:

```text
detached
```

Detached children require explicit policy authorization and are not required
for v1.

`owned` agents are durable child agents. Their control object and session
history remain after cancellation so the parent can inspect failure state and
raw history.

`temp` agents are ordinary child agents while they run: they still have
`agent/<name>`, `agent/<name>.sock`, and `agent/<name>.d/` controls, and they
must pass the same identity, policy, and mount attenuation checks as `owned`
agents. A runtime may remove the temp agent object and socket after exit,
cancel, or parent death. Durable task results should be written through the
parent child-result channel or an explicitly shared space; temp agent private
history is not a stable retention contract.

## Child Session

The child has an independent session:

```text
/ctx/home/1000/agent/rev-123/session/default/
  messages.jsonl
  events.jsonl
  latest.md
  state
  cwd
  created_at
  updated_at
  meta.json
  context/
```

The parent keeps only child coordination state and results:

```text
/ctx/home/1000/agent/coder/session/default/context/child/rev-123/
  agent
  session
  status
  handoff.md
  result.md
  refs.jsonl
  artifact/
```

The parent context pack should include `result.md`, summarized refs, and
necessary artifacts. It should not include the child's full `messages.jsonl`.
CLI inspection may join this coordination table with the backing agent process
controls. For example, `ctx agent children coder` reports each child channel's
stable `status` plus the backing `agent/<agent>.d/parent`, `model`, `status`,
and `pid`, giving a `ps`-like view of worker task state and its parent
session/run attachment without adding fields to `context/child/<child>/`.
If `ctx agent wait` sees an `active` child whose backing agent's effective
state is `dead`, has no live `pid`, and still points back to the same parent
agent/session, it may synchronously reap that child channel as `cancelled`. A
recorded `ready` or `busy` state with a numeric `pid` that is absent from
`/proc` is treated as no live `pid` for this read. This mirrors a parent
observing child process death; it does not introduce a watcher or polling
runtime.
When parent stop cancels a child result channel, the compact `result.md` should
name the backing child agent that was cancelled.

## Child Context Window

Authenticated dynamic child creation carries an optional `window` token count
through the Agent SDK, runtime client, per-run capability frame, and compensated
creation transaction. It is a positive `u32`, not control-file text. Absence
means inherit; zero is invalid.

The child inherits the parent's selected model. Window materialization follows
these rules:

```text
request absent, parent effective known    child window = parent effective number
request absent, parent effective unknown  child window = auto
request number                            child window = requested number
```

An explicit request must not exceed the parent effective window and must not
exceed the inherited model's known hard limit. If either required upper bound
is unknown, explicit creation fails closed. A child can therefore retain or
reduce a known parent window but cannot expand it. The exact canonical value is
written into `agent/<child>.d/window` inside the existing compensated child
creation transaction before executable publication.

Failed validation creates no child object, private home, session, result
channel, runtime unit, or executable wrapper. Failure after preparation uses
the existing receipt-checked compensation path. The SDK, runtime-client, and
host must use one shared wire field definition; unknown fields and a numeric
zero are protocol errors.

## Child Tool Path

Dynamic `agent.create` accepts an optional `path` string using the canonical
colon-separated `agent/<name>.d/path` syntax. Absence means exact inheritance
of the parent's current canonical tool path. An explicit value may only remove
parent tiers while preserving their first-hit order. Added tiers, duplicate
tiers, reordered tiers, empty components, and an empty path are invalid. The
validated canonical value is written to the child `path` control inside the
existing compensated creation transaction; materialization must not substitute
`/ctx/tool` or re-derive a different path.

## Handoff Protocol

Parent to child:

```text
handoff.md
input refs
policy subset
mount subset
output contract
```

The default `agent.create` RPC intentionally keeps the child in the parent's
security domain: it preserves the complete parent label and policy subject and
attenuates the effective policy and mounts. A future supervisor that assigns a
distinct child label must also provision the corresponding global capability
grants explicitly; child creation never mutates global tool policy.

Child to parent:

```text
result.md
artifacts
refs.jsonl
status
```

The parent may inspect a terminal child result with:

```bash
ctx agent wait coder work-123 --session default
```

This is a non-blocking waitpid-shaped read of the parent-owned result channel:
`pending` and `active` are not terminal, while `done`, `error`, and `cancelled`
return the child status and compact result. The process exit status is 0 for
`done`, 1 for `error`, and 130 for `cancelled`. It does not poll or reap
history. The parent result channel remains durable. For a dedicated temp
`worker-*` or `executor-*` backing agent, `wait` may also reap the temp
`agent/<name>`, `agent/<name>.sock`, and `agent/<name>.d/` object after the
terminal result is recorded; the canonical shared `worker` and `executor`
objects remain reusable worker entries.
The same distinction applies when parent stop cancellation reaches children:
dedicated temp `worker-*` and `executor-*` objects may be removed, while the
canonical shared worker objects stay present.

Example `handoff.md`:

```markdown
Task: Review the mount ABI section.

Scope:
- Read spec/agent-tool-security.md
- Check whether bind/rbind/noexec/nosuid/nodev semantics are clear
- Do not edit files
- Return issues and proposed patches

Output:
- summary
- concrete suggested changes
- risk notes
```

Example `result.md`:

```markdown
Summary:
The mount ABI is mostly clear, but it needs explicit same-directory atomic
rename requirements for queue claims.

Findings:
1. ...
2. ...

Suggested patch:
...
```

Example child refs:

```jsonl
{"path":"/work/docs/spec/agent-tool-security.md","hash":"sha256:...","summary":"mount spec reviewed"}
{"path":"artifact/patch.diff","kind":"patch","summary":"suggested patch"}
```

## Hybrid DAG/ReAct Scheduling

Scheduling is parent agent behavior, recorded as ordinary parent-session
context. v1 does not add a root `workflow/`, `job/`, `hook/`, `scheduler/`, or
`react/` namespace, and it does not add a background watcher. A parent may keep
a bounded hybrid plan in its own session context, for example:

```text
/ctx/home/1000/agent/coder/session/default/context/plan.json
```

The stable shape is data:

```json
{
  "version": 1,
  "mode": "dag-react",
  "nodes": [
    {
      "id": "plan",
      "kind": "dag",
      "agent": "planner",
      "requires": [
        {"class": "tool", "name": "fs.read", "permission": "execute"}
      ]
    },
    {
      "id": "review",
      "kind": "react",
      "agent": "reviewer",
      "child": "rev-123",
      "session": "default",
      "handoff": "Task: review the plan\nScope:\n- Check the accepted refs\nOutput:\n- result.md summary\n",
      "deps": ["plan"],
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "reviewer", "permission": "create"}
      ]
    },
    {
      "id": "implement",
      "kind": "react",
      "child": "work-123",
      "handoff": "Task: implement the accepted plan\n",
      "deps": ["review"],
      "max_steps": 8,
      "requires": [
        {"class": "agent", "name": "worker", "permission": "create"}
      ]
    }
  ]
}
```

Rules:

```text
plan.json is parent-owned context, not a submission queue.
nodes form a directed acyclic graph.
kind is either dag or react.
react nodes must declare a bounded max_steps value.
deps may only name other nodes in the same plan.
requires only records permissions the parent effective policy already grants.
child identifies the child result channel when a node is delegated.
delegated nodes must include handoff text.
session and handoff are only valid when child is present.
delegated nodes may omit agent; omitted delegated agent means worker.
delegated nodes may omit session; omitted delegated session means the parent session name.
delegated nodes require parent agent:<node.agent-or-worker> create authority.
ready nodes are incomplete nodes whose deps all have durable parent-visible results.
ready delegated nodes may be materialized as context/child/<child>/handoff.md.
delegated nodes are complete when context/child/<child>/status is done.
local completion inputs only apply to non-delegated parent-owned nodes.
advance means derive completed nodes from parent context and materialize ready delegated handoffs once.
advance must not rewrite an already materialized child result channel.
an already materialized child channel must match the node agent, session, and handoff.
an already materialized child channel must have valid status and refs files.
handoff/result/refs still use context/child/<child>/.
```

The reference `coder` agent's default `system.md` follows this rule: it acts as
the parent coordinator and should prefer delegated `react` nodes for independent
implementation work. The reference `worker` agent runs on the spark model path
and should execute bounded handoffs without making architecture decisions beyond
the parent-provided scope.
Agent names `worker`, `worker-*`, `executor`, and `executor-*` are the v1
worker-role naming convention. If such an agent object omits `agent/<name>.d/model`,
the runtime and schedule views use the spark worker model by default. Other
agent names must keep an explicit model control file or use the normal `main`
default only in non-runtime display contexts.

The thin CLI entrypoint for the single transition is:

```bash
ctx schedule status home/1000/agent/coder/session/default/context/plan.json --done plan
ctx schedule advance home/1000/agent/coder/session/default/context/plan.json --done plan
ctx schedule claim home/1000/agent/coder/session/default/context/plan.json work-123
ctx schedule result home/1000/agent/coder/session/default/context/plan.json work-123 done "implemented"
ctx schedule result home/1000/agent/coder/session/default/context/plan.json work-123 cancelled "interrupted"
```

`status` only reads the parent-visible schedule table and child states.
`advance` only derives completed nodes and materializes ready child handoffs.
`claim` only marks a materialized handoff `active` when a worker accepts it; it
is idempotent while active and cannot rewind terminal results.
`result` writes a terminal child status plus compact result text and optional
refs JSONL back into `context/child/<child>/`. The command output names the
parent ref and the exact child `handoff.md`, `result.md`, and `refs.jsonl` ABI
paths so the parent and worker can treat the handoff as an inspectable file
boundary. These commands are not a daemon, watcher, queue worker, or hot-reload
boundary.
When a worker is launched from this output, the parent should hand over the
existing `model=`, `life=`, `role=`, `parent=`, `child_parent=`, `plan=`,
`handoff=`, `result=`, and `refs=` fields; the
worker should claim and finish through the same `ctx schedule` commands rather
than creating another coordination file, queue, or runtime abstraction.
`ctx schedule status` exposes the same `child_parent` value in its read-only
table so parent and worker views agree before claim/result transitions.

The parent uses DAG edges for known ordering and uses ReAct only inside a node's
bounded execution loop. ReAct steps may decide tool calls and child handoffs,
but each action is still checked against the agent's policy, mount view, tool
visibility, and child attenuation rules.

Git commit remains the development event boundary. A parent may revise
`context/plan.json` after a commit, but the revision is just session context.
It must not create a second hot-reload, polling, or hook trigger.

## Permission Attenuation

Child authority is attenuated from the parent:

```text
child policy must be a subset of parent effective policy
child mounts must be a subset of parent visible mounts
child groups must be a subset of parent groups
child tool path must be an ordered tier subset of the parent tool path
child context must be the handoff context, not the full parent context
```

Rules:

```text
child cannot see parent full context unless explicitly handed off.
child cannot read parent messages.jsonl by default.
child can only write result channels and authorized artifacts back to parent.
parent rw may become child ro.
parent visible may become child hidden.
parent ro must not become child rw.
parent hidden must not become child visible.
```

This preserves context isolation. A child agent is an isolated task unit, not a
shortcut for reading the parent's entire prompt state.

## Parent Death

Runtime must enforce owned child shutdown. It must not rely on the child
choosing to exit.

When the explicit `/ctx/tool/agent.stop` lifecycle tool is absent, host-side
`ctx agent stop` acts as a small supervisor fallback: it marks the stopped agent
and any existing `owned` or `temp` descendants as cancelled/dead through their
ordinary `agent/<name>.d/status`, `pid`, and `log` controls. This fallback keeps
child history and control objects readable. If the stopped child agent backs a
pending or active parent `context/child/<child>/` channel, the fallback records
that channel as `cancelled`, making the terminal state visible to
`ctx agent wait`. It is not a second workflow or queue namespace.

Retired reference agents `base`, `worker`, and `executor` are retained for
manual review and never participate in this stop cascade, even when legacy
`parent` and `life` controls make them look parent-owned. Their controls,
runtime status, and parent child-result channels remain unchanged.

Before the supervisor fallback resets a unit or writes a control, it builds and
validates the complete non-retired descendant plan. This includes ownership
cycle detection, no-follow writable checks for every planned control, and
validation of existing pending/active parent child-result channels. Execution
then follows validated post-order: descendants before their parent. A planning
error leaves every planned control and cancellation channel unchanged.
For a dedicated temporary `worker-*` or `executor-*`, that same plan validates
the complete cleanup before stopping anything: wrapper/socket paths must be
absent or removable non-directories, and every control-tree directory must be
owner-writable. Control-tree symlink entries are cached as leaf unlinks and are
never followed. Cleanup executes only the cached post-order entries.

Recommended implementation:

```text
each parent agent has a runtime process group or cgroup
each owned child agent is tracked under the parent
runtime maintains parent -> children state
parent death synchronously cancels owned children and removes or cancels temp children
```

Example cgroup shape:

```text
/sys/fs/cgroup/cortexfs/user-1000/agent-coder/
  cgroup.procs

/sys/fs/cgroup/cortexfs/user-1000/agent-coder/child/rev-123/
  cgroup.procs
```

Cancellation sequence:

```text
1. mark parent state = stopping/dead
2. mark child state = stopping
3. close child sockets
4. send SIGTERM to child process group or cgroup
5. wait a short grace period
6. send SIGKILL if needed
7. mark child session state = cancelled
8. append events.jsonl
```

Events:

```jsonl
{"type":"agent.child.cancel","parent":"coder","child":"rev-123","reason":"parent_dead"}
{"type":"agent.stop","agent":"rev-123","status":"cancelled"}
```

Parent death cancels runtime, not history:

```text
child process dies
child socket closes
child session state = cancelled
child messages/events remain readable
```

## Summary Rules

```text
1. Independent tasks should run in child agents.
2. Child agents receive handoff context, not full parent context.
3. Child permissions must be a subset of parent effective permissions.
4. Child mounts must be a subset of parent visible mounts.
5. Owned child agents die when the parent dies.
6. Parent death cancels child runtime, not child history.
7. Detached children require explicit policy and are not required in v1.
```
