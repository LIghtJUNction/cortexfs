# Agent, Tool, and Security ABI

Layer boundary:

```text
model = pure inference endpoint
tool  = executable capability endpoint
agent = policy-bound orchestrator process
```

By default, a model has no:

```text
tool permission
filesystem write permission
project context
long-term memory
task planning
chroot/mount policy
MCP/skill
cluster scheduling
```

An agent owns Linux uid/gid/groups, label, home, root, cwd, mounts, policy,
context, and tool execution decisions. Real file writes, tool calls, and
shared-space access are attributed to the agent, not to the model.

Tool boundary:

```text
model may emit tool_call events
model must not execute tools
agent decides whether to execute tools
agent policy decides whether execution is allowed
```

## Agent as File

```text
/ctx/agent/
  coder
  coder.sock
  coder.d/
    owner
    uid
    gid
    groups
    label
    iso
    parent
    life
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

Control files:

```text
owner   owning Linux user uid
uid     runtime uid; defaults to owner
gid     runtime gid
groups  supplementary groups, one gid per line
label   CortexFS agent label, for example user_u:agent_r:coder_t:s0
iso     isolation profile: shared, uid, or userns
parent  parent agent, session, or run that created this agent
life    lifecycle ownership, default owned
```

Multiple agents may share one Linux uid. The uid expresses the user boundary.
The label expresses the agent security boundary.

`meta.json` may exist for longer descriptions such as purpose, creation time,
or issue number. Policy decisions must not depend on `meta.json`.

Agent startup:

```text
1. Read /ctx/agent/<name>.d/*
2. Set CTX_ROOT, CTX_HOME, and CTX_PATH
3. Merge agent/<name>.d/env
4. Establish runtime identity from uid/gid/groups/label
5. Create the mount namespace
6. Apply bind mounts from mount
7. chroot to root
8. cd to cwd
9. exec the agent runtime
```

## Agent View

An agent view is the set of files, tools, models, sockets, and shared spaces
visible to an agent.

It is derived from:

```text
root
cwd
mount
path
model
policy
Linux uid/gid/groups/mode bits
CortexFS label
```

Resource tiers are distinct:

```text
/ctx/model              system models, visible to all users by default
/ctx/agent              system agents, visible to all users by default
/ctx/tool               system tools, visible to all users by default
/ctx/home/<uid>/model   user-specific models and aliases
/ctx/home/<uid>/agent   user-specific agent state and user agents
/ctx/home/<uid>/tool    user-specific tools
```

Those directories are durable resources. They are not the same thing as an
agent's runtime view. At runtime, CortexFS/FUSE projects the tools visible to
one agent in memory from `agent/<name>.d/path`, `policy`, `mount`, uid/gid, and
mode bits. Do not create placeholder files or symlink copies merely to express
that a system tool is visible to an agent.

CortexFS does not define MCP config formats, skill formats, project rule
formats, or prompt package formats. Those are ordinary files.

Examples:

```text
/home/alex/.codex/config.toml  /home/agent/.codex/config.toml  ro  bind,nosuid,nodev,noexec
/home/alex/project/.mcp.json   /work/.mcp.json                 ro  bind,nosuid,nodev,noexec
```

An agent may read those files only if they are visible inside its chroot or
mount namespace and allowed by Linux permissions. Executing any capability
derived from them still requires CortexFS tool policy.

Skill files are ordinary files visible through the agent mount namespace.
CortexFS does not define skill file formats. Skill visibility is determined by
mount visibility, Linux permissions, and policy. Skills do not grant authority.

Lifecycle:

```text
start
ready
busy
idle
stopping
dead
```

v1 does not introduce a global daemon. Prefer each agent process owning its own
socket, pid, log, and sessions. A future supervisor is implementation detail.
It must not add another root directory.

## Agent Home

An agent does not use the user's home directly:

```text
/ctx/home/1000/
  agent/
    coder/
      root/
      session/
      data/
      cache/
      log/
  tool/
  model/
```

Recommended config:

```text
/ctx/agent/coder.d/root = /ctx/home/1000/agent/coder/root
/ctx/agent/coder.d/cwd  = /work
```

Runtime environment:

```sh
CTX_ROOT=/ctx
CTX_HOME=/ctx/home/1000
HOME=/ctx/home/1000/agent/coder
CTX_PATH=/ctx/tool:/ctx/home/1000/tool
```

`CTX_PATH` names candidate tool source tiers. The runtime-visible tool
directory can be a filtered in-memory FUSE projection of those tiers for this
agent. A tool being present in `/ctx/tool` means it is installed system-wide;
it does not by itself grant any agent execution authority.

## Mount File

`/ctx/agent/<name>.d/mount` format:

```text
source<TAB>target<TAB>mode<TAB>options
```

v0 parser rules:

```text
source and target must be absolute paths
source and target must not contain TAB or newline; return EINVAL if they do
mode is only ro or rw
options is a comma-separated list of small words
unknown option returns EINVAL
```

Fixed v0 option set:

```text
bind
rbind
nosuid
nodev
noexec
-
```

`-` means no extra option. Except for `-`, options must not repeat. `bind` and
`rbind` are mutually exclusive.

Example:

```text
/ctx	/ctx	ro	rbind,nosuid,nodev
/ctx/home/1000/agent/coder	/home/agent	rw	rbind,nosuid,nodev
/home/me/project	/work	rw	rbind,nosuid,nodev
/ctx/shared/project-a	/shared/project-a	rw	rbind,nosuid,nodev
/tmp	/tmp	rw	rbind,nosuid,nodev
```

## Agent Creation

An agent can create another agent only through normal CortexFS objects and
policy checks. There is no root-level `spawn/`, `factory/`, or
`agent-template/`.

`base` is the ordinary root agent for v1 lineage:

```text
/ctx/agent/base
/ctx/agent/base.sock
/ctx/agent/base.d/
```

`base` is not a template namespace and does not add inheritance semantics.
It is a normal agent object with a normal label, mount table, policy, socket,
home, and session state. New top-level agents should be created by
`agent.create` with `parent=agent:base`. Child agents created by other agents
must still be attenuated from their direct parent.

The child appears as ordinary agent ABI:

```text
/ctx/agent/reviewer
/ctx/agent/reviewer.sock
/ctx/agent/reviewer.d/
  owner
  uid
  gid
  groups
  label
  iso
  parent
  life
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
```

`parent` is a small text file. v1 should keep it simple:

```text
agent:base
agent:coder
```

or, when needed:

```text
agent:coder session:default run:01H...
```

Do not turn lineage into a separate tree in v1.

Child defaults:

```text
owner  = parent owner
uid    = parent uid
gid    = parent gid
groups = subset of parent groups
iso    = shared
life   = owned | temp
```

A temp child uses the same defaults except:

```text
life   = temp
```

Every agent should have a distinct CortexFS label unless it is intentionally
the same security domain. A child that reuses the parent's label is the same
security subject for policy purposes.

## Agent Tool Visibility

An agent can see and execute only the intersection of:

```text
user-visible scope
CortexFS security context
```

User-visible scope is derived from ordinary Linux and mount facts:

```text
agent uid/gid/groups
tool file owner/group/mode bits
agent mount table
mount mode and noexec option
CTX_PATH search order
```

CortexFS security context is derived from stable agent controls:

```text
agent label subject, for example coder_t
agent/<name>.d/policy
tool/<tool>.d/policy
shared/session/mount policy where relevant
```

Both sides must allow access. A tool that is executable and mounted but not
allowed by policy is invisible for execution. A tool allowed by policy but not
visible to the agent uid/gid/groups or blocked by `noexec` is also invisible.
Prompts, skills, MCP config files, schemas, and model output never expand this
set.

The agent terminal path is:

```text
te starts tsh
tsh resolves tool names through CTX_PATH
```

Agents should be granted the `tsh` terminal capability as their primary shell
tool. `tsh` is not a host command shell and must not fall back to `PATH`.
Interactive behavior such as `bash`, `tmux`, or `zellij` is provided by
ordinary visible tool objects with those names.

Child agent attenuation is mandatory:

```text
child permissions must be a subset of parent effective permissions
child policy must be a subset of parent effective policy unless a supervisor grants more
child groups must be a subset of parent groups unless a supervisor grants more
child mounts must be derived from parent-visible mounts
```

Mount attenuation:

```text
parent rw may become child ro
parent visible may become child hidden
parent ro must not become child rw
parent hidden must not become child visible
```

A child mount must not expose paths invisible to the parent. For example, a
parent that sees `/work` and `/shared/project-a` may grant the child read-only
views of those paths, but not `/home/user`, `/etc`, `/var/log`, or
`/shared/project-b` unless a supervisor authorizes them.

Owned child agents are cancelled when the parent dies. Parent death cancels
the child runtime, not the child's session history. See `17-child-agents.md`
for handoff, result, and lifecycle rules.

Names should stay short:

```text
coder
reviewer
planner
runner
worker1
fix-123
rev-123
```

Put longer descriptions in `agent/<name>.d/meta.json`.

## Tool, Shared, Policy, and Logs

Tool ABI, MCP-projected tools, shared-space access, policy v0, and log
placement are normative in [tool-policy-abi.md](tool-policy-abi.md).
