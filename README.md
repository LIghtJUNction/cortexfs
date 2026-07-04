# CortexFS

<p align="center">
  <img src="docs/assets/cortexfs-social-card.png" alt="CortexFS: agent runtime as a Unix ABI" width="900">
</p>

**CortexFS makes AI runtimes feel like Unix.**

It mounts a small, scriptable FUSE filesystem at `/ctx`. Models are files you
can talk to. Agents are files you can inspect, resume, and attach to. Tools are
files that an agent can load into context, keep resident in memory, or call like
CLI commands.

CortexFS is built for the moment when you want to look inside an agent runtime
while it is running: which model it is using, which files it can see, which tools
are loaded, what its terminal is doing, and what state will survive the session.
Agents wake through systemd socket activation, so idle agents do not need a
polling loop just to be reachable.

The v1 shape is intentionally small:

```text
/ctx/status
/ctx/bin
/ctx/model
/ctx/agent
/ctx/tool
/ctx/home
/ctx/shared
```

For the normative ABI, start with [docs/DESIGN.md](docs/DESIGN.md).

## What It Feels Like

Start an agent, open its chat UI, and ask it to review a file in the mounted
workspace:

```text
ctx
/ctx/agent/coder
live chat

$ ctx agent chat coder
coder/default ❯ review /workspace/docs/DESIGN.md

tool
tsh -> fs.read {"path":"/workspace/docs/DESIGN.md"}

usage
input 912 / output 184
```

That is the intended surface: direct conversation with an agent file, direct
model calls behind it, and tools that can be discovered, loaded, pinned, and
invoked through the same filesystem view.

<p align="center">
  <a href="docs/assets/cortexfs-demo.mp4">
    <img src="docs/assets/cortexfs-demo-poster.jpg" alt="Watch the CortexFS agent REPL demo" width="720">
  </a>
</p>

<p align="center">
  <a href="docs/assets/cortexfs-demo.mp4">Watch the MP4 demo</a>
  ·
  <a href="docs/assets/cortexfs-demo.webm">WebM</a>
  ·
  <a href="https://lightjunction.github.io/cortexfs/">Docs site</a>
</p>

At any time, attach to the agent terminal:

```bash
ctx agent watch coder
ctx agent attach coder
```

`watch` is read-only. `attach` joins the persistent `ctxterm -> tsh` terminal and
lets you see the tool shell exactly as the agent sees it.

## Mental Model

CortexFS has three executable object classes:

```text
model  pure inference endpoint
agent  policy-bound orchestrator process
tool   executable capability endpoint
```

The root stays small, the model tree stays provider-neutral, and agent context is
ordinary visible state instead of hidden SDK state. CortexFS is a way to peer
through the filesystem boundary and see what is happening inside agent software.

![CortexFS v1 ABI map](docs/assets/cortexfs-abi-map.svg)

## Quick Start

Install the AUR package and start the system mount:

```bash
paru -S cortexfs-git
sudo systemctl enable --now cortexfs.service
ctx doctor
```

Then take a quick look around:

```bash
ctx status
ctx ls
ctx ls model
ctx ls agent
ctx ls tool
ctx file type tool/fs.read
ctx which tool tsh
```

For a local checkout without installing the package:

```bash
cargo run -p cortexfs --bin ctx -- bootstrap
cargo run -p cortexfs --bin ctx -- doctor
```

## Bootstrap A Programming Coder

`ctx bootstrap` creates the default `architect`, `coder`, and `reviewer`
agents. Start `coder` from the project checkout, then open the chat UI:

```bash
ctx bootstrap
ctx agent start coder --session default
ctx agent chat coder
```

Inside chat, `/workspace` shows the host checkout mounted for the agent and
`/tools` lists visible CortexFS tools. `ctx agent chat` is the human chat
surface; `tsh` remains the agent-facing tool shell inside `ctxterm`.

Ask clear coding tasks directly:

```text
fix the failing CortexFS test, edit the source, run focused verification, and report changed files
```

The bootstrapped `coder` prompt requires it to inspect `AGENTS.md`, check
`git status --short`, use `fs.replace` for surgical edits, run available
format/static-check/lint/test commands, inspect the diff, and finish with exact
verification evidence.

To verify the full bootstrapped programming path locally:

```bash
npm run bootstrap-coder:smoke
```

The smoke test bootstraps a temporary tree, starts `coder`, mounts a writable
source fixture at `/workspace`, requires nearest `AGENTS.md` evidence, edits the
fixture through `fs.replace`, reruns verification, inspects `git diff`, and
checks that the final answer records changed files and exact verification
commands.

## A First Walk Through `/ctx`

The root ABI is deliberately short:

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

Those top-level names are the contract. CortexFS does not add stable root
namespaces for provider, format, MCP, skill, memory, vector, workflow, job, hook,
or audit internals. Those concepts may exist as visible ordinary files, or as
implementation details behind tools, but they are not part of the root ABI.

Every executable object follows the same basic shape:

```text
name        exec or metadata endpoint
name.sock   stateful JSONL stream endpoint, only when supported
name.d/     small control files
```

Examples:

```text
/ctx/model/openai/gpt-5.4-mini
/ctx/model/openai/gpt-5.4-mini.d/driver
/ctx/agent/coder
/ctx/agent/coder.sock
/ctx/agent/coder.d/policy
/ctx/tool/tsh
/ctx/tool/tsh.d/schema
```

## Models

Models live under `/ctx/model/<provider>/<model>`:

```text
/ctx/model/debug/echo
/ctx/model/openai/gpt-5.4-mini
/ctx/model/anthropic/claude-sonnet-4
/ctx/model/google/gemini-2.5-pro
```

They are executable files. You can call a model path directly for one-shot
inference:

```bash
/ctx/model/openai/gpt-5.4-mini "explain this error"
echo '{"messages":[{"role":"user","content":"hello"}]}' | /ctx/model/openai/gpt-5.4-mini
ctx exec model/openai/gpt-5.4-mini "summarize README.md"
```

`/ctx/model/main` is the conventional default model alias. It is only a symlink,
not a privileged registry entry. Change the symlink to change the default model:

```bash
ln -sfn /ctx/model/openai/gpt-5.4-mini /ctx/home/$(id -u)/model/main
```

Provider API differences are handled below the filesystem ABI. CortexFS keeps
the visible model tree provider-neutral and API-format-neutral. API keys are not
stored in model files or process environments; long-lived provider keys belong in
the root-owned CortexFS system secret store.

## Agents

Agents live under `/ctx/agent`:

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

`/ctx/agent/coder.sock` is the chat/session wakeup point. The packaged systemd
unit `cortexfs-agent@.socket` listens on the runtime socket and projects it back
into the CortexFS tree. A client connection can wake the agent runtime on
demand, instead of keeping every agent hot in the background.

Start an agent and open chat:

```bash
ctx agent start coder --session default
ctx agent chat coder
```

Agents are executable files too. Use chat for live conversation, or call an
agent path directly for a one-shot task:

```bash
/ctx/agent/coder "review /workspace/docs/DESIGN.md"
echo '{"task":"fix tests"}' | /ctx/agent/coder
ctx exec agent/coder "summarize the latest failure"
```

Use `ctx agent watch coder` to observe the persistent terminal, or
`ctx agent attach coder` when you explicitly want to join it and write to stdin.

`ctx agent start` launches the agent inside a lightweight sandbox. The default
path is:

```text
ctx agent start
  -> bwrap sandbox
  -> ctxterm
  -> tsh
```

`ctxterm` is the agent terminal emulator. It owns the PTY, keeps the terminal
observable, and exposes `watch` and `attach`. `tsh` is the agent-facing tool
shell that discovers tools, loads tool context, and invokes allowed
capabilities.

Session commands default to the latest or current session when `--session` is
omitted:

```bash
ctx send coder "summarize the current failure"
ctx history coder
ctx resume coder
ctx agent chat coder
ctx agent history coder
ctx agent output coder
ctx agent resume coder
ctx agent pack coder
```

An agent's runtime view is derived from control files plus Linux permissions:

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
agent view. Terminal startup cannot bypass the policy and mount files that
define the agent.

## Tools And `tsh`

Tools live under `/ctx/tool` and are found through `CTX_PATH`, not shell `PATH`:

```sh
export CTX_ROOT=/ctx
export CTX_HOME="$CTX_ROOT/home/$(id -u)"
export CTX_PATH="$CTX_ROOT/tool:$CTX_HOME/tool"
export PATH="$CTX_ROOT/bin:$PATH"
```

`tsh` is the tool shell and the default native tool exposed to agents. Agents use
it to discover, load, pin, and run tools according to policy. A tool can be:

```text
discovered under /ctx/tool
loaded into the agent's current tool context
pinned so it stays available
invoked from tsh
called directly as a CLI-style CortexFS tool when policy allows it
```

Human usage:

```bash
tsh tools
tsh which fs.read
tsh help fs.read
tsh load fs.read
```

Direct CLI-style usage:

```bash
/ctx/tool/fs.read '{"path":"README.md"}'
echo '{"path":"README.md"}' | /ctx/tool/fs.read
```

Standalone `tsh` can inspect visible tools and metadata. Tool execution runs
inside an agent terminal so CortexFS can apply policy, mounts, uid, gid, and
`CTX_PATH` together.

`ctx tool` is only a direct CLI entrypoint for allowlisted safe CortexFS core
tools such as `tsh.config`. Ordinary visible tools, plus authority-bearing core
tools such as `fs.write` and `shell.exec`, still run through `tsh` or an
authorized agent/runtime path.

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

Native dynamic tool artifacts are loaded on demand. The SDK keeps resident
dynamic tools under a W-TinyLFU cache, so hot tools can stay in memory while cold
unpinned tools are admitted or evicted by use. `tsh.config` controls the visible
tool context size with `max_loaded_tools`; pinned tools are protected from
automatic context eviction.

Tool metadata printed to a terminal is escaped so untrusted descriptions and
schemas cannot inject terminal control sequences.

## Files, Metadata, And xattrs

`ctx file` describes CortexFS file types and ABI metadata. It is not a
replacement for `cat`.

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
allow coder_t model:openai/gpt-5.4-mini use
allow coder_t shared:project-a read
allow coder_t shared:project-a write
```

There is no explicit deny, glob, priority, inheritance, variable expansion, or
path matching. Default is deny.

The security stack is layered:

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
ctx agent chat coder
ctx agent watch coder
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

Run the deterministic agent tool-loop smoke without a live model:

```bash
npm run agent-tool-loop:smoke
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
