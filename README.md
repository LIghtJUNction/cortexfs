# CortexFS

![CortexFS turns AI runtimes into Unix-shaped files](docs/assets/cortexfs-hero.svg)

CortexFS is a Linux FUSE ABI for AI agents. It exposes models, agents, tools,
sessions, policies, and runtime state as a small Unix-shaped filesystem under
`/ctx`.

The project goal is not to mirror an AI platform database into directories. The
v1 ABI stays deliberately small:

```text
/ctx/status
/ctx/bin
/ctx/model
/ctx/agent
/ctx/tool
/ctx/home
/ctx/shared
```

The detailed specification lives in [docs/DESIGN.md](docs/DESIGN.md).

## Quick Start

Install the AUR package and start the system mount:

```bash
paru -S cortexfs-git
sudo systemctl enable --now cortexfs.service
ctx doctor
```

Useful first checks:

```bash
ctx status
ctx ls
ctx ls model
ctx ls agent
ctx ls tool
ctx file type tool/fs.read
ctx which tool tsh
```

For a local development tree without installing:

```bash
cargo run -p cortexfs --bin ctx -- bootstrap
cargo run -p cortexfs --bin ctx -- doctor
```

## Mental Model

CortexFS has three executable object classes:

```text
model  pure inference endpoint
agent  policy-bound orchestrator process
tool   executable capability endpoint
```

Each object follows the same shape:

```text
name        exec or metadata endpoint
name.sock   stateful JSONL stream endpoint, only when supported
name.d/     small control files
```

Examples:

```text
/ctx/model/openai/gpt-4o
/ctx/model/openai/gpt-4o.d/driver
/ctx/agent/coder
/ctx/agent/coder.sock
/ctx/agent/coder.d/policy
/ctx/tool/tsh
/ctx/tool/tsh.d/schema
```

![CortexFS v1 ABI map](docs/assets/cortexfs-abi-map.svg)

## Root Layout

The root ABI is intentionally boring:

```text
/ctx/
  status
  bin/
  model/
  agent/
  tool/
  home/
  shared/
```

CortexFS does not add root namespaces for provider, format, MCP, skill, memory,
vector, workflow, job, hook, or audit internals. Those may exist as ordinary
files visible to an agent, or as implementation details behind tools, but they
are not stable root ABI.

## Models

Models live under `/ctx/model/<provider>/<model>`.

```text
/ctx/model/debug/echo
/ctx/model/openai/gpt-4o
/ctx/model/anthropic/claude-sonnet-4
/ctx/model/google/gemini-2.5-pro
```

`/ctx/model/main` is the conventional default model alias. It is just a symlink,
not a special registry entry. Change the symlink to change the default model.

```bash
ln -sfn /ctx/model/openai/gpt-4o /ctx/home/$(id -u)/model/main
```

Provider API differences are handled by Rig. CortexFS keeps the filesystem ABI
above provider and API-format details. API keys are never stored in model files;
resolution prefers environment variables, then the system keychain, then reports
unconfigured.

## Agents

Agents live under `/ctx/agent`.

```text
/ctx/agent/coder
/ctx/agent/coder.sock
/ctx/agent/coder.d/
  owner
  uid
  gid
  groups
  label
  root
  cwd
  mount
  path
  model
  policy
  system.md
  prompt.template.md
```

Start and attach to an agent terminal:

```bash
ctx agent start coder --session default
ctx agent attach coder
```

Session commands default to the latest/current session when `--session` is
omitted:

```bash
ctx send coder "summarize the current failure"
ctx history coder
ctx resume coder
ctx agent history coder
ctx agent output coder
ctx agent resume coder
ctx agent pack coder
```

Agent runtime visibility is derived from control files plus Linux permissions:

```text
agent.d/root
agent.d/cwd
agent.d/mount
agent.d/path
agent.d/model
agent.d/policy
uid/gid/groups
mode bits
```

CLI `--mount` arguments are validated, but runtime execution uses the derived
agent view. That keeps terminal startup from bypassing the policy and mount
files that define the agent.

## Tools And tsh

Tools live under `/ctx/tool` and are found through `CTX_PATH`, not through shell
`PATH`.

```sh
export CTX_ROOT=/ctx
export CTX_HOME="$CTX_ROOT/home/$(id -u)"
export CTX_PATH="$CTX_ROOT/tool:$CTX_HOME/tool"
export PATH="$CTX_ROOT/bin:$PATH"
```

`tsh` is the tool shell. It is the default native tool exposed to agents. Agents
can use `tsh` to discover, load, pin, and run other tools according to policy.

Human usage:

```bash
tsh tools
tsh which fs.read
tsh help fs.read
tsh load fs.read
```

Standalone `tsh` can inspect visible tools and metadata. Tool execution runs
inside an agent terminal so CortexFS can apply the agent's policy, mounts, uid,
gid, and `CTX_PATH` together.

`ctx tool` is only a direct CLI entrypoint for allowlisted safe CortexFS core
tools such as `tsh.config`. Ordinary visible tools and authority-bearing core
tools such as `fs.write` and `shell.exec` still run through `tsh` or an
authorized agent/runtime path, so `ctx tool` cannot bypass CortexFS tool
authorization.

```bash
ctx tool tsh.config
ctx tool tsh.config '{"max_loaded_tools":32}'
```

Inside `tsh`, `load` and `pin` load tool metadata into the agent context without
executing the tool binary or dynamic library:

```text
tsh> tools
tsh> load fs.read
tsh> pin bash
tsh> loads
```

Tool metadata printed to a terminal is escaped so untrusted descriptions and
schemas cannot inject terminal control sequences.

## Files, Metadata, And xattrs

`ctx file` describes CortexFS file types and ABI metadata. It is not a replacement
for `cat`.

```bash
ctx file type tool/fs.read
ctx file check agent/coder.d/mount
ctx file info model/main
cat /ctx/status
```

Virtual files can expose xattrs for cost and origin hints before an agent reads
their contents:

```bash
getfattr -d /ctx/tool/fs.read
getfattr -n user.cortexfs.token_estimate /ctx/model/main
getfattr -n user.cortexfs.origin /ctx/model/helper
```

Common values:

```text
user.cortexfs.origin          virtual | disk | overlay
user.cortexfs.storage         memory | disk
user.cortexfs.token_estimate  approximate read cost
user.cortexfs.cache_bytes     cache size hint
user.cortexfs.cache_entries   cache entry count
```

## Sessions

Durable agent sessions live in the owning user's CortexFS home:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/
  messages.jsonl
  events.jsonl
  latest.md
  state
  cwd
  context/
  index/
```

Socket requests are JSONL frames:

```jsonl
{"op":"send","id":"client-msg-id","session":"default","scope":"private","cwd":"/work","input":"hello"}
```

Scopes:

```text
private  current uid only, durable and resumable
shared   written to /ctx/shared/<name> according to policy
temp     temporary, no durable resume requirement
```

Clients should read `session/index/current`, `session/index/list`, and
`session/index/by-cwd/*` instead of maintaining a second hidden history store.

## Policy Model

Policy v0 is a minimal allowlist:

```text
allow coder_t tool:tsh execute
allow coder_t tool:fs.read execute
allow coder_t model:openai/gpt-4o use
allow coder_t shared:project-a read
allow coder_t shared:project-a write
```

There is no explicit deny, glob, priority, inheritance, variable expansion, or
path matching. Default is deny.

The security stack is intentionally layered:

```text
Linux uid/gid/groups
file mode bits
chroot + bind mounts
CortexFS label
agent policy
tool/model policy
```

## Common Commands

```bash
ctx status
ctx doctor
ctx env
ctx root
ctx ls
ctx ls /
ctx ls home
ctx ls model
ctx ls agent
ctx ls tool
ctx which tool fs.read
ctx path shared project-a
ctx file type tool/fs.read
ctx file check agent/coder.d/policy
ctx agent ps
ctx agent start coder
ctx agent attach coder
ctx agent history coder
ctx agent output coder
```

## Development

Build and test:

```bash
cargo fmt --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

Regenerate README images and the local benchmark chart:

```bash
scripts/update-readme-svg.sh
```

![CortexFS local benchmark](docs/assets/cortexfs-performance.svg)

Verus proof sources live under `proofs/verus/`. They are opt-in and do not
change the runtime Cargo workspace. Install the upstream `verus` binary from
<https://github.com/verus-lang/verus> and run:

```bash
scripts/verify-verus.sh
```

Current proofs cover the v1 object-name ABI predicate; see
[docs/proofs/verus.md](docs/proofs/verus.md).
