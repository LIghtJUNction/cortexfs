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
ctx update
ctx mount

ctx ls
ctx ls model
ctx ls agent
ctx ls tool
ctx ls home
ctx ls shared/project-a

ctx which model openai/gpt-4o
ctx which agent coder
ctx which tool fs.read

ctx path shared project-a
ctx agent history coder
ctx agent output coder
ctx agent resume coder --session default
ctx agent wait coder work-123 --session default

ctx agent new reviewer --model openai/gpt-4o --tool fs.read
ctx agent new reviewer --label reviewer_t --shared project-a:read --mount /work /work ro
ctx agent start reviewer
ctx agent stop reviewer
ctx agent status reviewer
ctx agent env reviewer
ctx agent ps
ctx agent chat reviewer
ctx agent send reviewer --approve example.echo "run the declared echo tool"

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

ctx object check tool.yaml
ctx object install --source /var/lib/cortexfs/storage/v1-root tool.yaml --tier system
ctx object inspect --source /var/lib/cortexfs/storage/v1-root tool example.echo --tier system
ctx object uninstall --source /var/lib/cortexfs/storage/v1-root tool example.echo --tier system
ctx object residue audit --source /var/lib/cortexfs/storage/v1-root
ctx object residue cleanup --source /var/lib/cortexfs/storage/v1-root --path tool/.cortexfs-install-123-0 --dev DEV --ino INO

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

`ctx update [SOURCE]` is an alias for `ctx bootstrap [SOURCE]`. It updates the
reference source tree only; it does not remount `/ctx`, start a watcher, or add
a second refresh boundary.

Optional flags:

```text
ctx update --check [SOURCE]     report tree_version, missing agents, retired leftovers
ctx update --dry-run [SOURCE]   show would_ensure / would_skip / would_write (no writes)
```

Default bootstrap materializes `architect` / `coder` / `reviewer`, writes
`bin/cortexfs.bootstrap.json` (`schema`, `tree_version`, `managed_agents`,
`applied_migrations`) only when state differs. Retired `base` / `worker` /
`executor` objects are reported and retained for manual review because legacy
trees have no manifest proving ownership and full control-tree integrity.
Session history under `home/` is never deleted by update.

Top-level agent session shortcuts follow the same current-session default as
their `ctx agent ...` forms:

```text
ctx history AGENT
ctx history AGENT --session SESSION
ctx history AGENT SESSION
ctx resume AGENT
ctx resume AGENT --session SESSION
ctx resume AGENT SESSION
ctx send AGENT INPUT
ctx send AGENT --session SESSION INPUT
ctx send AGENT SESSION INPUT
ctx agent wait AGENT CHILD [--session SESSION]
```

Omitting the session reads `session/index/current` first and falls back to
`default`. The positional `SESSION` form remains accepted for compatibility.
`ctx send` and `ctx resume` render assistant events the same way as
`ctx agent send` and `ctx agent resume`; raw socket JSONL is reserved for lower
level socket commands and explicit raw agent modes.

`ctx agent chat` is the preferred human-chat spelling. `ctx agent repl` is a
compatibility alias for the same socket UI and session defaults. It is not the agent terminal and
does not enter `tsh`; humans use `ctx agent watch` or `ctx agent attach` for the
persistent terminal.

`ctx agent send` and `ctx agent chat`/`repl` accept repeatable
`--approve TOOL`. In non-raw mode the client answers a hosted SDK
`approval_request` with `allow_once` only when its exact tool name is in this
explicit list; every other name is denied. There is no blanket approval or TTY
prompt in v1. Raw clients and clients without this handler close their write
half and therefore fail closed for `approval=ask`.

`ctx agent wait` is a non-blocking waitpid-shaped reader for a parent-owned
child result channel. It reads `context/child/<child>/status`; `pending` and
`active` fail with service unavailable, while terminal `done`, `error`, and
`cancelled` print
`child<TAB>status<TAB>agent<TAB>session<TAB>model<TAB>life<TAB>role` followed by
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
`node<TAB>kind<TAB>agent<TAB>child<TAB>session<TAB>model<TAB>life<TAB>role<TAB>child_parent<TAB>state`, where state is one of
`blocked`, `ready`, `pending`, `active`, `done`, `error`, or `cancelled`.
The session, model, life, role, and child_parent columns are `-` for local parent
nodes. Delegated child nodes show the explicit child session, or the inherited
parent session when the schedule node omits one, plus the selected backing agent
model, lifecycle, and backing parent ref.
For delegated nodes, the backing agent must exist as both `agent/<name>` and
`agent/<name>.d/`; schedule commands must not invent `main`/`owned` defaults for
a missing worker object.
Each emitted `handoff` line includes the child `agent`, `session`, selected
`model` from `agent/<name>.d/model`, `life` from `agent/<name>.d/life`, `role`, a
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

Executable extensions use host-side check, new-object-only install, read-only
inspect, and receipt-managed uninstall commands:

```text
ctx object check MANIFEST
ctx object install --source PATH MANIFEST [--tier user|system]
ctx object inspect --source PATH CLASS NAME [--tier user|system]
ctx object uninstall --source PATH CLASS NAME [--tier user|system] [--yes]
```

`ctx object check` is read-only and requires no source tree. It performs the
same strict manifest, control, artifact type, executable mode, and SHA-256
validation used before publication by `install`; success prints
`valid CLASS/NAME`. It accepts exactly one manifest path and no install flags.

`--source` is required and names the durable backing tree that may be written.
`/ctx`, `CTX_ROOT`, and `--root` are ABI projections and are never inferred as
installation targets. `MANIFEST` names class `tool` or `agent`, binds one
executable path to its SHA-256, and supplies class controls. Legacy schema
`cortexfs.object/v1` strictly accepts neither `version` nor `compatibility`.
Schema `cortexfs.object/v2` requires `version` as an object SemVer and
`compatibility.cortexfs` as a Cargo-style SemVer requirement. Unknown fields
and controls, symlinks, non-regular or non-executable artifacts, digest
mismatches, and existing object names are rejected. Relative executable paths
resolve against the manifest directory. The manifest cannot specify commands,
arguments, wrappers, or install tier.

Both `check` and `install` compare a v2 CortexFS requirement with the CortexFS
package version compiled into the current `ctx`. A mismatch is invalid input,
exits 2, and performs no writes. Version compatibility is not an authority
grant and does not start a runtime.

Agent manifests may include the optional `abi` control with exactly `argv-v1`
or `sdk-envelope-v1`. If it is absent, the installed agent uses the exact
legacy `argv-v1` contract. Other values are rejected before publication.

User-tier tools install under `home/<effective-uid>/tool`; system tiers use
`tool` and `agent`. The root ABI retains `home/<effective-uid>/agent`, but
neither manifest schema carries tier identity to the root socket runtime, so
the installer rejects user-tier agents and directs callers to system tier.
Installation does not grant policy authority. It initializes canonical
runtime-owned status/pid/log files but does not create socket state. The
complete control directory is staged and synced,
then published no-replace before the executable is published last as the
visible object commit boundary. Both published receipts are checked again
before success is reported. Success or failure may retain a hidden
`.cortexfs-install-*` safety residue for explicit future cleanup.

A `cortexfs.object-install/v2` receipt records `object_version` and
`cortexfs_requirement`; a `cortexfs.object-install/v1` receipt records neither.
Manifest v2 adds compatibility metadata only: installation remains
new-object-only, and upgrade or replacement is not declared by this contract.

`ctx object inspect` is a read-only check of one exact installer-managed `tool`
or `agent`; the tier defaults to `user`. It validates the installer receipt and
its identity/version, the recorded class/name/tier, the retained control
directory's device/inode/type, and the retained executable's
device/inode/regular type, execute bits, and SHA-256. It also rejects executable
length, mode, mtime, or ctime changes observed during inspection; the receipt
does not bind the complete install-time mode. Success prints:

```text
installed CLASS/NAME tier=T schema=cortexfs.object/v1 sha256=HASH executable=DEV:INO control=DEV:INO
installed CLASS/NAME tier=T schema=cortexfs.object/v2 version=VERSION requires-cortexfs=REQ sha256=HASH executable=DEV:INO control=DEV:INO
```

Inspection does not claim that mutable control-file contents still match their
install-time values. An object with a missing or legacy receipt is unmanaged
and is reported as unavailable; inspection never adopts or modifies it. For v2,
the compatibility values are recorded facts: inspection does not reject an
installed object merely because a later CortexFS build no longer matches them.

`ctx object uninstall` accepts only one exact installer-receipt-managed `tool`
or `agent` pair; the tier defaults to `user`. Its default dry-run performs the
same retained-receipt validation as inspection and does not write. Success
reports the exact executable and control device/inode pair that would be, or
was, removed.

With `--yes`, uninstall first quarantines the executable on the same filesystem
to form the invisible object boundary, syncs and rechecks its receipt, then
quarantines the control directory, syncs and rechecks both receipts. It reuses
bounded residue cleanup only after the complete exact stage has been verified.
This ordering is deliberately not a claim of pair atomicity. At receipt
checkpoints, a failure does not intentionally overwrite or delete a foreign
replacement; it may leave audit-visible safety residue when safe restoration
cannot complete.

Before `--yes`, the caller must quiesce the matching agent runtime and all other
processes under the same Unix authority that can write the backing directory.
Receipt checks do not close Linux's final pathname syscall race against such a
writer. Uninstall grants no authority, creates no socket, and does not start or
stop a runtime. It does not re-run v2 compatibility admission, so a later
CortexFS version mismatch cannot strand a receipt-managed object.

Durable residue maintenance is explicit and separate from installation:

```text
ctx object residue audit --source PATH
ctx object residue cleanup --source PATH --path REL --dev DEV --ino INO [--yes]
```

`audit` performs a bounded, no-follow, descriptor-relative walk of the durable
source. It reports `.cortexfs-install-*`, `.cortexfs-cleanup-*`, and
`.ctx-rollback-*` observations in relative-path order, one terminal-safe line
per residue, including kind, path, device, inode, file kind, empty/occupied
state, and cleanup eligibility. An audit observation is not cleanup authority:
a later command must supply the relative path and exact `dev`/`ino` receipt
explicitly. Audit does not silently skip unreadable, cross-device, or
over-limit subtrees; a system backing tree therefore requires an identity that
can inspect the complete tree.

Cleanup accepts install-stage directories only under `tool/`, `agent/`,
`home/<decimal-uid>/tool/`, or `home/<decimal-uid>/agent/`. It defaults to a
dry-run and prints `would-clean ... entries=N`; `--yes` is required to mutate
and prints `cleaned ... entries=N` on success. Applying cleanup first isolates
the top path with same-directory no-replace rename and verifies the moved inode.
It then handles each preflighted descendant the same way before post-order
deletion, without following symlinks. The isolation name is
`.cortexfs-cleanup-*`; retained cleanup quarantine is always audit-only and
cannot be supplied as a cleanup target. If a later cleanup step fails, the
command tries to restore the original `.cortexfs-install-*` name only while the
quarantined top-level inode still matches the submitted receipt and no-replace
restoration is safe. Successful restoration permits a fresh audit and retry.
If safe restoration is impossible, the error reports the exact retained
`.cortexfs-cleanup-*` path for later audit. Unknown file kinds, traversal
limits, new entries, or sync failures also stop cleanup.

The caller must stop concurrent processes that share write authority to the
backing directories before using `--yes`. Linux has no atomic
“unlink only if this path still has dev/ino” operation, so receipt checks cannot
protect the final syscall window from a hostile writer in the same Unix
authority boundary. Cleanup never intentionally unlinks a receipt mismatch at
its checkpoints.

`.ctx-rollback-*` is always audit-only because it may preserve an inode from a
rollback conflict. A retained `.cortexfs-cleanup-*` is likewise audit-only;
only an eligible `.cortexfs-install-*` path can be submitted to cleanup. This
command never deletes rollback residue or owned agent objects. Installation
does not invoke residue cleanup automatically.

At runtime, `CTX_SOURCE` is only an ambient candidate path. Durable writers
must authenticate the runtime capability receipt and match its nofollow source
directory path, device, inode, and plain-directory type before writing.

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

`ctx agent new` calls `/ctx/tool/agent.create` only from a complete agent
runtime context (`CTX_AGENT`, `CTX_SESSION`, `CTX_RUN_ID`, and `CTX_SOURCE`).
An ordinary human host invocation creates a standard agent object directly by
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
`agent/<name>.d/log`. Start output and the `agent.start` event echo `model`,
`life`, and `role`; `pid` remains numeric-only, and systemd invocation ids are log facts.

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
status value first, followed by `model=`, `life=`, `role=`, `parent=`, `children=`,
`pid=`, `ppid=`, `uid=`, `gid=`, `groups=`, `root=`, and `cwd=` lines.
This keeps the first line usable as the process state while exposing the
backing model, worker role, direct child count, parent relationship, Linux identity, and
chroot/cwd for worker inspection. The `parent=` line uses the same normalized
parent ref as `ctx agent ps`, including optional `session` and `run`. The `children=` count includes direct child
agents whose effective state is not `dead`; recorded `ready` or `busy` children
with stale numeric pids are excluded the same way as `ctx agent ps`. `ctx agent ps` may
read `agent/<name>.d/parent`, `model`, `life`, `status`, and `pid` directly and
print the current agent tree with derived worker roles and live `ppid=` values.
Default `main` model selections and `owned` lifecycles may stay implicit;
non-default worker models and non-owned lifecycles should be visible in the
tree.
`ctx agent env` derives the same runtime view as `ctx agent start` and prints
the sandbox environment as `KEY=value` lines. It is a read-only inspection of
existing control files, not a way to inherit host variables or mutate runtime
state.
`ctx agent children` reads the parent session `context/child/<child>/` table and
prints tab-separated `child`, child-channel `status`, backing `agent`,
child-channel `session`, backing agent `parent_session`, backing agent
`parent_run`, backing agent `model`, backing agent `life`, backing agent `role`,
backing agent `status`, live parent `ppid`, and backing agent `pid` (`-` when absent). `role`
derives from the v1 worker-role name convention; the other backing-agent columns
are ordinary `agent/<agent>.d/*` controls, so worker task state and its parent
session/run attachment are
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
ordinary users do not need write access to `/ctx` or `/run/cortexfs`. Existing
installations may still expose `/run/cortexfs/terminal/<uid>/<agent>/<session>/main.sock`.
`ctx agent attach` tries the ABI path, the user runtime path, then the legacy
runtime path.

The corresponding human commands are:

```text
ctx agent watch <agent> --session <session>
ctx agent attach <agent> --session <session>
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
ctx cat model/openai/gpt-4o.d/cap
ctx file type tool/fs.read
ctx exec agent/coder "fix tests"
```

Object strings use ABI path form:

```text
model/openai/gpt-4o
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
ctx which model openai/gpt-4o
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
second durable history. For legacy tool results whose call id has no matching
event, projection preserves non-empty result content but clears the unproven
`source_call_id`; empty unmatched results are dropped. The trajectory `extra`
map records the stable `legacy_unmatched_tool_results` count. Projection does
not invent a tool call or chat message. If validation still fails, the CLI lists
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
stores tokens in the system keychain:

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
