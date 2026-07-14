# CortexFS Architecture

Normative ABI detail lives under [spec/](spec/). Visual identity lives in
[DESIGN.md](DESIGN.md) (Google Labs DESIGN.md format). This file is the
engineering design entry: what CortexFS is, where state lives, and what must
not become root ABI.

## One-page model

```text
/ctx is a FUSE Agent OS ABI view.
model is a pure inference file.
agent is the policy-bound orchestrator.
tool is a capability endpoint.
session is ordinary file history.
policy is a minimal SELinux-like allowlist.
Rig removes provider and API-format differences.
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
```

## Where things live

The packaged host keeps versioned durable trees under
`/var/lib/cortexfs/storage/generations/<generation>` and exposes the selected
tree through the atomic `/var/lib/cortexfs/storage/current` symlink. On a
systemd restart, `ctx storage update` clones the current generation, applies
and validates the next `bin/cortexfs.bootstrap.json` `tree_version`, then
switches `current`. The legacy `v1-root` directory is adopted once without
losing sessions, controls, or aliases. A failed stage leaves `current`
unchanged. This is a restart boundary, not a watcher, poller, or hot reload;
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
| Control | `/ctx/agent/<name>.d/*` | policy, mount, cwd, system.md |
| Agent home | `/ctx/home/<uid>/agent/<name>/` | session, data, cache, log |
| Session | `.../session/<session>/` | messages, events, context, load snapshots |
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

## Read the specs in order

```text
spec/README.md
spec/root-abi.md
spec/fuse-v1.md
spec/object-abi.md
spec/model-abi.md
spec/session-abi.md
spec/16-context.md
spec/agent-tool-security.md
spec/agent-runtime.md
spec/tool-policy-abi.md
spec/17-child-agents.md
spec/ctx-coreutils.md
spec/phase-1.md
```

## v1 red line

```text
Do not let /ctx become a directory mirror of an AI platform database.
It should stay small, hard, boring, and scriptable.
```
