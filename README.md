# CortexFS

CortexFS is a Linux-only Rust FUSE filesystem that exposes AI API formats,
providers, models, agents, tools, MCP, skills, memory, cluster state, and audit
records as ordinary files.

The mount tree is the public ABI. Many files are virtual files backed by
`cortexfs`/`cortexd` runtime state, not durable files on disk. This follows the
spirit of Linux interfaces such as `/proc` and `sysfs`: read files to inspect
state, write documented control files to request operations, and use normal
Unix tools to debug what an agent runtime is doing.

CortexFS is built for Linux first. Other operating systems are not a current
target.

## What This Version Provides

This first version establishes the repository, FUSE projection, strict Rust
workspace, and a provider-neutral filesystem ABI.

Implemented surfaces include:

- FUSE projection with top-level `formats`, `providers`, `models`, `spaces`,
  `agents`, `clusters`, `mcp`, `skills`, `tools`, `memory`, `vector`,
  `databases`, `audit`, and `control`.
- API format directories for `openai.chat`, `openai.responses`,
  `anthropic.messages`, and `google.generate_content`.
- Provider and model discovery through small text files such as `count`,
  `list`, `formats`, `base_url/*`, `enabled/*`, `health/*`, and `models/*`.
- File-based API submission through atomic rename into `inbox/*.req.json`.
- Runtime queue draining through `control/drain`.
- Route-aware audit records under `audit/events.jsonl`.
- Thread state with `messages.jsonl`, `latest.md`, `fingerprint`, `io.sock`,
  and `tool-loop/steps.jsonl`.
- MCP, tool, skill, memory, export, agent, and cluster projections that can be
  inspected with normal filesystem commands.
- Provider-neutral live-test support with local Ollama only as the current test
  fixture.

`cortex-cli mount` is wired to the FUSE projection. `init` and `daemon` are
reserved CLI commands and are not implemented yet.

## Filesystem Shape

The mounted root currently looks like this:

```text
/
  status
  capabilities/
  api/
  formats/
  providers/
  models/
  spaces/
  agents/
  clusters/
  mcp/
  skills/
  tools/
  memory/
  vector/
  databases/
  audit/
  control/
```

Small state files use simple text:

```bash
cat /mnt/cortex/status
cat /mnt/cortex/providers/count
cat /mnt/cortex/providers/list
cat /mnt/cortex/formats/openai.chat/models/list
cat /mnt/cortex/models/list
```

One value per file is preferred. Lists are newline-delimited. JSON and JSONL
are used when the payload is naturally structured.

## Provider And Model Discovery

Providers are configured backend instances, not hard-coded vendors. A provider
can represent OpenAI, Anthropic, Google/Gemini, an OpenAI-compatible relay,
Kimi, MiniMax, a local runtime, or another adapter.

Common reads:

```bash
cat /mnt/cortex/providers/count
cat /mnt/cortex/providers/list

provider_id="$(head -n1 /mnt/cortex/providers/list)"
cat "/mnt/cortex/providers/${provider_id}/formats"
cat "/mnt/cortex/providers/${provider_id}/models/count"
cat "/mnt/cortex/providers/${provider_id}/models/list"
cat "/mnt/cortex/providers/${provider_id}/health/status"
```

For a user/space-specific view, read from the space:

```bash
uid="$(id -u)"
cat "/mnt/cortex/spaces/users/${uid}/models/count"
cat "/mnt/cortex/spaces/users/${uid}/models/list"
cat "/mnt/cortex/spaces/users/${uid}/routes/openai.chat/provider"
cat "/mnt/cortex/spaces/users/${uid}/routes/openai.chat/model"
cat "/mnt/cortex/spaces/users/${uid}/routes/openai.chat/reason"
```

Provider secrets do not enter the mount tree. CortexFS exposes secret status,
active key identity, and rotation controls only.

## File-Based API Calls

API requests are submitted by writing a temporary file and atomically renaming
it into an inbox. Plain `write()` does not call a provider.

```bash
uid="$(id -u)"
api="/mnt/cortex/spaces/users/${uid}/api/openai.chat"

printf '%s\n' '{"messages":[{"role":"user","content":"Reply with cortexfs-ok"}]}' \
  > "${api}/inbox/001.tmp"

mv "${api}/inbox/001.tmp" "${api}/inbox/001.req.json"
printf '1\n' > /mnt/cortex/control/drain
cat "${api}/outbox/001.resp.json"
```

The request id is the filename stem. Responses materialize as
`<id>.resp.json`; provider or validation failures materialize as `<id>.error`.

The same pattern is used by batch jobs, tools, MCP prompt rendering, memory
ingest, feedback pairs, agent tasks, and cluster tasks where those projections
expose `inbox/` and `outbox/`.

## Agent Transparency

CortexFS is meant to make agent software inspectable instead of opaque.

Examples:

```bash
cat /mnt/cortex/agents/helper/runtime/state
cat /mnt/cortex/agents/helper/profile/default_model/provider
cat /mnt/cortex/agents/helper/tools/enabled
cat /mnt/cortex/agents/helper/skills/enabled
cat /mnt/cortex/mcp/tools/local-fs.read_file/input_schema.json
cat /mnt/cortex/skills/installed/cortexfs-test/SKILL.md
```

Thread context is visible:

```bash
uid="$(id -u)"
thread="/mnt/cortex/spaces/users/${uid}/threads/demo"

cat "${thread}/messages.jsonl"
cat "${thread}/latest.md"
cat "${thread}/fingerprint"
cat "${thread}/tool-loop/steps.jsonl"
```

Socket files such as `io.sock` are reserved as fast paths. The file tree remains
the source of truth for inspectable state and audit.

## Audit And Exports

Audit is a first-class filesystem surface:

```bash
cat /mnt/cortex/audit/fields
cat /mnt/cortex/audit/events.jsonl
cat /mnt/cortex/audit/usage
cat /mnt/cortex/audit/cost
```

Space exports are exposed as JSONL:

```bash
uid="$(id -u)"
ls "/mnt/cortex/spaces/users/${uid}/exports"
cat "/mnt/cortex/spaces/users/${uid}/exports/conversations.jsonl"
cat "/mnt/cortex/spaces/users/${uid}/exports/sft.jsonl"
cat "/mnt/cortex/spaces/users/${uid}/exports/preference.jsonl"
cat "/mnt/cortex/spaces/users/${uid}/exports/tool_calls.jsonl"
cat "/mnt/cortex/spaces/users/${uid}/exports/agent_traces.jsonl"
```

The export shape is intended to keep training-data conversion straightforward
without hiding source context.

## Build

Prerequisites:

- Linux
- Rust toolchain compatible with the workspace `rust-version`
- FUSE 3 userspace support
- `fusermount3`

Build and validate:

```bash
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

Mount locally:

```bash
mkdir -p tests/mounts/cortexfs
cargo run -p cortex-cli -- mount tests/mounts/cortexfs
```

The test mountpoint is `tests/mounts/cortexfs`. It is local runtime state and
must not contain source files, fixtures, or persistent data.

## Live Provider Test

The repository includes an ignored live test for a local model. It does not use
external cloud APIs.

Current fixture:

```text
Ollama model: smollm2:135m
```

Run it only when the model is installed:

```bash
ollama list
cargo test -p cortex-providers --test ollama_live --locked -- --ignored --nocapture
```

If `smollm2:135m` is missing, pull that exact fixture model before running the
live test:

```bash
ollama pull smollm2:135m
```

Ollama is not a privileged CortexFS provider path. It is just the current local
live-test fixture.

## Development Rules

- Do not add `mod.rs`.
- Add dependencies with `cargo add`.
- Keep provider/model design neutral.
- Use Git commits as the development event boundary.
- Do not add background watchers, polling loops, hot reload, or a `dev`
  subcommand.
- Keep FUSE callbacks short; long-running work belongs in the daemon execution
  plane.
- Treat the mounted tree as an ABI: paths and file semantics should be stable,
  documented, and tested.

## Status

CortexFS is pre-release infrastructure. The first version is suitable for
validating the filesystem ABI, inspecting the projected tree, exercising
file-based request flow, and building out the daemon/runtime behind the ABI.
