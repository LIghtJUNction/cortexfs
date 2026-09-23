# CortexFS Architecture

Normative ABI detail lives under [spec/](spec/). Visual identity lives in
[DESIGN.md](DESIGN.md). This document is the engineering architecture entry
point: what CortexFS owns, what hosted Agent CLIs own, and where compatibility
code is being removed.

## One-page model

```text
CortexFS is a Unix/FUSE execution substrate, not a second Agent framework.
/ctx is a read-write FUSE filesystem with path-level Unix/policy attenuation.
agent objects define identity, authority, visible paths, and launch boundaries.
hosted Agent CLIs own their model/provider auth, session/context, approvals,
tool loop, compaction, and provider-specific behavior.
CortexFS owns process, identity, mount, path, network, resource, socket, and FUSE limits.
MCP and other open protocols are reused directly when they already fit.
```

The first target hosted CLIs are OpenAI Codex CLI, Anthropic Claude Code, and
`earendil-works/pi`. Antigravity CLI is the current Google-side extension
target. These are **target integrations** during issue #318, not a claim that
all launch profiles already exist in the current release.

## Frozen root rule

```text
root only contains stable object classes
root never mirrors provider, database, workflow, memory, or orchestration internals
MCP must not become a root namespace
skills, project rules, prompts, and CLI config remain ordinary visible files
```

Forbidden root namespaces include `skill/`, `memory/`, `mcp/`, `workflow/`,
`chan/`, `job/`, `hook/`, `audit/`, and `control/`. Those concepts may exist as
ordinary files, object-local state, or external protocol endpoints; they are not
new root classes.

## Authority model

Authority comes from Linux and CortexFS policy, never from prompt text.

```text
agent definition
  -> uid/gid/supplementary groups
  -> mode/umask and path policy
  -> authorized mounts and sockets
  -> policy-derived network namespace / egress authority
  -> cgroup/resource ceilings
  -> child process
```

`/ctx` is mounted read-write. Individual files or directories may be read-only
when their semantics require it. Backing storage must never be exposed as a
writable bind that bypasses FUSE enforcement.

A read-write `/workspace` follows ordinary Unix subtree semantics. CortexFS does
not secretly hide `.git` or other project metadata from a hosted CLI. A policy
may explicitly make `/workspace/.git` read-only. Paths outside the authorized
workspace, including an out-of-tree linked-worktree gitdir, still need separate
authority.

## Execution boundary

The backend-neutral launch contract is intentionally small:

```text
executable + argv
cwd
environment
stdin/stdout/stderr or PTY
uid + gid + supplementary groups
umask/mode policy
authorized mounts and sockets
policy-derived network namespace / egress authorization
resource ceilings
exit status
signals + cancellation
```

A launch profile may translate a backend's command-line spelling, but it must
not become a provider registry, model router, session manager, or second
workflow engine. Backend-specific logic stays at the edge.

The same process boundary must be usable for interactive and headless modes.
CortexFS should host the CLI's existing text, JSON/JSONL, RPC, resume, MCP, and
approval capabilities instead of normalizing them into a new CortexFS wire
protocol.

## Responsibility split

| Layer | Owns | Must not own |
| --- | --- | --- |
| Hosted Agent CLI | model/provider selection, auth, Agent loop, context/session semantics, approval UX, CLI-native tools | host filesystem or network authority outside projected capabilities |
| CortexFS execution boundary | identity, cwd/env, stdio/PTTY, signals, mounts, sockets, network authority, resource limits, FUSE projection, hard policy ceiling | provider-specific Agent intelligence or a duplicate loop |
| FUSE `/ctx` | inspectable object classes and authorized read/write projection | provider or framework configuration mirrors |
| Frontends / channels | presentation, transport adaptation, user input/output | a second process or Agent authority model |

Prompt text, skills, AGENTS.md, CLAUDE.md, and similar project rules may affect
CLI behavior, but they never increase Linux or CortexFS authority.

## Process shape

The target process graph is ordinary Unix composition:

```text
user / frontend
      |
      v
ctx launch / supervisor
      |
      +-- apply identity, policy, mounts, network authority, resources
      |
      +-- stdio or PTY via ctxterm when needed
      |
      v
Codex | Claude Code | Pi | Antigravity | other compatible CLI
      |
      +-- reads/writes authorized workspace and /ctx paths
      +-- connects only through policy-authorized network/socket paths
      +-- connects to explicitly projected MCP/tools/sockets
      +-- exits with ordinary process status
```

`ctxterm` is PTY mechanics, not an Agent runtime. `ctxmcp` is an explicit MCP
adapter, not a `/ctx/mcp` root. systemd/user services and cgroups may supervise
process lifetime without becoming a CortexFS orchestration DSL.

## Omarchy and Linux

Omarchy is the primary desktop/development validation environment, currently
tracked from `omacom/omarchy`. Support should remain ordinary Arch/Linux
support rather than a brand-specific branch in core.

The important compatibility points are:

```text
PATH and shell discovery
XDG config/data/cache directories
mise or other CLI wrappers
systemd --user
Wayland terminal environment
FUSE availability and permissions
uid/gid/groups
home/workspace visibility
signals and PTY behavior
```

Do not solve Omarchy support by exposing the entire user home, trusting all of
`~/.local/bin`, or inheriting every host environment variable into a sandbox.
Project the minimum required executable/config paths with explicit policy.

## Open protocols first

Prefer existing protocols and Unix behavior:

- MCP for tool servers where MCP already fits.
- OAuth 2.0 / OIDC for authentication flows owned by the hosted CLI/provider.
- OpenTelemetry GenAI conventions for observable facts where applicable.
- JSON Schema / OpenAPI / JSON-RPC only when an external boundary already uses them.
- HTTP/SSE/WebSocket as transport, not as reasons to invent a new root ABI.
- Unix files, sockets, process status, signals, uid/gid, and mount semantics for
  local execution.

A CortexFS-private ABI needs evidence that existing standards and Unix
primitives cannot express the required behavior.

## Compatibility surface during migration

The repository still contains the former self-hosted Agent runtime:
`cortexfs-protocol`, model/provider adapters, Agent SDK loop behavior,
object-runner tool loops, session orchestration, compaction, and the
`cortexfs.interaction/*` runtime path. These remain compatibility code until a
safe migration removes or narrows them.

They are not the target architecture for new Agent features.

Rules during issue #318:

1. Do not add new provider-specific core branches to the legacy runtime.
2. Do not expand the legacy loop to match features already owned by hosted CLIs.
3. Keep stable root/path ABI compatible unless a documented migration says otherwise.
4. Identify real independent consumers before deleting a crate or public type.
5. Prefer deleting wrappers, registries, parsers, and state machines that only
   serve the obsolete self-hosted path.
6. Keep backend adapters thin and process-shaped.

## Durable state and observability

CortexFS may persist facts it actually owns: object definitions, policy,
receipts, process status, terminal replay, audit facts, and explicitly defined
session compatibility state. It must not silently become the authoritative
conversation database for hosted CLIs that already own their session format.

Observable events should be facts about CortexFS-owned boundaries, for example:

```text
process requested / started / exited
signal or cancellation sent
mount or identity rejection
resource-limit failure
PTY attached/detached
FUSE permission denial
```

Do not translate every backend model/tool event into a new universal CortexFS
event hierarchy unless a concrete external consumer requires that mapping.

## Extension points

Extend at edges, not in the root:

| Need | Preferred mechanism |
| --- | --- |
| New Agent CLI | thin launch profile / executable selection |
| Tool server | MCP or existing executable tool boundary |
| Platform channel | process-isolated channel adapter |
| Project guidance | ordinary visible rule/skill files |
| New hard authority | Unix/FUSE policy with explicit ABI review |
| Observability | boundary facts and standard telemetry |

`agent.sh` remains a thin convenience entry point. It must not grow protocol,
session, provider, or Agent-loop behavior.

## Migration order

Issue #318 proceeds in small slices:

1. Remove outer sandbox behavior that breaks normal authorized workspace
   semantics. #320 removed the implicit `.git` mask.
2. Reuse the existing arbitrary-child PTY/process machinery instead of adding a
   new runner.
3. Add the smallest explicit executable/argv selection seam for hosted CLIs.
4. Make `/ctx` writable through FUSE where the agent policy allows it; never by
   writable-bypassing backing-store binds.
5. Add backend launch profiles only when command-line differences require them.
6. Retire legacy provider/model/tool/session runtime pieces as independent
   consumers disappear.

Each slice should reduce or preserve conceptual surface. A migration that adds
more managers, registries, protocols, or persistent control state than it
removes needs explicit justification.

## Source of truth

- [spec/](spec/) is normative ABI documentation.
- `AGENTS.md` contains repository development constraints.
- [internal-architecture.md](internal-architecture.md) defines Rust/process
  layering for the migration.
- Legacy self-hosted runtime behavior is documented for compatibility, not as a
  mandate to extend it.