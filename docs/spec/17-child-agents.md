# Child Agent Lifecycle ABI

Independent tasks should run in child agents. The parent agent should keep task
decomposition, global constraints, and result indexes. The child agent owns the
task process and returns a compact result.

The default lifecycle is owned:

```text
Child agents are owned by their parent unless explicitly detached by policy.
Owned children die when the parent dies.
```

v1 should support `owned`. `detached` is reserved for a future explicit policy
grant and should not be exposed unless needed.

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

`life` is one small text value:

```text
owned
```

Future value:

```text
detached
```

Detached children require explicit policy authorization and are not required
for v1.

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

## Handoff Protocol

Parent to child:

```text
handoff.md
input refs
policy subset
mount subset
output contract
```

Child to parent:

```text
result.md
artifacts
refs.jsonl
status
```

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

## Permission Attenuation

Child authority is attenuated from the parent:

```text
child policy must be a subset of parent effective policy
child mounts must be a subset of parent visible mounts
child groups must be a subset of parent groups
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

Recommended implementation:

```text
each parent agent has a runtime process group or cgroup
each owned child agent is tracked under the parent
runtime maintains parent -> children state
parent death synchronously cancels owned children
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
