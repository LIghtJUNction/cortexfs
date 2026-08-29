# CortexFS

<p align="center">
  <img src="docs/assets/cortexfs-hero.svg" alt="CortexFS /ctx" width="900">
</p>

<p align="center">
  <a href="https://crates.io/crates/cortexfs"><img alt="crates.io" src="https://img.shields.io/crates/v/cortexfs"></a>
  <a href="https://lightjunction.github.io/cortexfs/"><img alt="documentation" src="https://img.shields.io/badge/docs-live-2A8F73"></a>
  <a href="https://www.rust-lang.org/"><img alt="Rust 1.91+" src="https://img.shields.io/badge/rust-1.91%2B-000000?logo=rust"></a>
  <a href="https://github.com/LIghtJUNction/cortexfs/blob/main/LICENSE"><img alt="MIT" src="https://img.shields.io/badge/license-MIT-2A8F73"></a>
</p>

**A durable, inspectable agent runtime for Linux.** CortexFS uses FUSE to make
models, agents, tools, channels, and session facts available through a small
`/ctx` filesystem ABI. Rust processes enforce authority and execute work;
ordinary files expose the durable facts needed to inspect, resume, and audit it.

```text
/ctx/model     provider-neutral inference objects
/ctx/agent     policy-bound agent definitions
/ctx/tool      governed capability endpoints
/ctx/channel   channel state and channel-local tools
/ctx/home      per-UID durable state
/ctx/shared    explicitly shared state
```

## Core properties

- Session history is append-only `messages.jsonl` and `events.jsonl`; prompt
  context is disposable and rebuildable.
- Authority comes from Linux identity, mount visibility, path checks, policy,
  and host-owned secrets—never prompts or skills.
- Agents use one portable tool path, `tsh`, and see only permitted tools.
- Providers, channels, and extensions use narrow Rust crates and versioned
  Unix-socket ABIs instead of a monolithic agent framework.

Read the normative [specification](docs/spec/README.md),
[architecture](docs/architecture.md), and
[internal architecture](docs/internal-architecture.md) before production use.

## Models, authentication, and metadata

Provider configuration is host state under `/etc/cortexfs/providers.d/*.json`.
Credentials belong only in the root-owned CortexFS secret store; they never
appear in `/ctx`, model controls, agent environments, audit errors, or logs.

```bash
sudo ctx provider preset install codex
ctx auth methods codex
sudo ctx auth login codex --profile subscription
sudo ctx auth status codex --profile subscription

# API keys are read from stdin and stored as a named profile.
printf '%s' "$MY_PROVIDER_KEY" | sudo ctx auth login openai \
  --method api-key --stdin --profile work
```

The provider-auth boundary supports API keys, Authorization Code + PKCE, and
device-code flows. Provider adapters own vendor request shapes; model objects
remain provider-neutral. See the [Model ABI](docs/spec/model-abi.md).

`cortexfs-metadatas` dynamically fetches and atomically caches
[models.dev/catalog.json](https://models.dev/catalog.json). It normalizes limits,
modalities, tool calling, reasoning, and lifecycle facts while retaining exact
provider-serving and model-only records (including benchmarks and weights).
No provider model table is hardcoded in Rust; missing or invalid cache facts
stay **unknown**.

## Channels and extensions

`cortexfs-channels` defines the generic message, receipt, effect, command, and
`cortexfs.channel.socket/v1` boundary. Platform adapters own their credentials,
rate limits, retries, uploads, and WebSocket lifetimes.

`cortexfs-channel-sdk` is the Rust SDK for a process-isolated adapter:
implement `ChannelService`, run `ChannelRuntime`, and use `ChannelSender` from
the platform receive loop. Discord is the first reference implementation with
gateway input, per-UID session routing, Unix-socket tool control, idempotent
embed/file/thread/component operations, and redacted errors.

See [channels.md](docs/channels.md) and the
[Channel ABI](docs/spec/channel-abi.md).

## Evaluation

The optional `cortexfs-futureagi` adapter turns a validated CortexFS ATIF
trajectory into Future AGI evaluation inputs or submits it to a compatible
Future AGI endpoint. It is explicit and one-shot: no background uploader and
no additional filesystem ABI. See [Future AGI evaluation](docs/futureagi.md).

## Eve-compatible direction and Pi-level elegance

CortexFS targets functional compatibility with platforms such as
[Vercel Eve](https://vercel.com/eve), while retaining Rust and the filesystem
ABI: durable sessions, tools, skills, sandboxed execution, channels, dynamic
capabilities, subagents, approval pauses, and observable events map onto
existing agent/session/tool/channel boundaries. It does not introduce parallel
`workflow`, `hook`, `plugin`, or `memory` roots.

Internally, elegance tracks the Pi toolkit
([badlogic/pi-mono](https://github.com/badlogic/pi-mono)): `cortexfs-protocol`
stays the provider-neutral IR (no HTTP, secrets, or agent loop);
agent runtime + object runner own the minimal tool loop and event facts;
`ctx`, terminals, and channel adapters are replaceable surfaces around the
same core. See [architecture.md](docs/architecture.md) and
[internal-architecture.md](docs/internal-architecture.md).

`cortexfs-protocol` stays deliberately narrow: a pure request/event IR and
converter for OpenAI Chat/Responses, Anthropic Messages, and Gemini. It has no
HTTP client, credentials, retry logic, filesystem access, or agent loop. This
adopts useful provider-protocol ideas from libraries such as
[genai](https://crates.io/crates/genai) without turning CortexFS into a
multi-purpose client facade.

## Install and develop

```bash
curl -fsSL https://raw.githubusercontent.com/LIghtJUNction/cortexfs/main/scripts/install.sh | sh
ctx status
ctx update --ref main        # plan an immutable host update
ctx update --ref main --yes  # apply with native-package rollback
```

For a source checkout:

```bash
cargo run -p cortexfs --bin ctx -- bootstrap
cargo run -p cortexfs --bin ctx -- doctor
scripts/test.sh cargo test --workspace
```

Continue with [getting started](docs/getting-started.md),
[using CortexFS](docs/using-cortexfs.md), and the
[specification index](docs/spec/README.md).
