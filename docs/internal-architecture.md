# CortexFS Internal Architecture

This document is the **engineering structure** companion to
[architecture.md](architecture.md). Product/ABI rules stay there. This file
governs **how Rust code is layered**, which boundaries may depend on which,
how errors and binaries should look, and how to migrate the current monolith
without breaking the frozen root ABI.

Normative ABI: [spec/](spec/). Naming: [naming-guide.md](naming-guide.md).
Contributor rules live in the repository-root `AGENTS.md`.

---

## 1. Problem we are solving

Today almost all production logic lives in one crate (`cortexfs`) with:

| Symptom | Cost |
| --- | --- |
| ~75k LOC lib + ~20k LOC bins in one package | Slow compile; any change rebuilds everything |
| Heavy deps always linked (`fuser`, `arrow`/`parquet`, `reqwest`, …) | SDKs and narrow tools pull a FUSE filesystem stack |
| Flat `pub use` / `use crate::*` surface | Hidden coupling; hard to see allowed edges |
| Many domain `*Error` types + residual `Result<_, String>` | Inconsistent recovery, weak `source()` chains |
| God modules (`object/`, `agent/`, `runtime/`, `bin/ctx`) | Cognitive load; hard reviews |

Product architecture (files, policy, sessions) is already clear. **Internal**
architecture is the missing layer: process roles, crate roles, module layers,
and error policy.

---

## 2. Process architecture (runtime shape)

CortexFS is a **small set of long-lived and short-lived Unix processes**, not a
single in-process AI framework.

```text
┌─────────────┐     FUSE      ┌──────────────────┐
│  user tools │──────────────▶│ cortexfs-mount   │  (projection of generation)
│  ctx / tsh  │               └────────┬─────────┘
└──────┬──────┘                        │ reads
       │ JSONL socket                  ▼
       │                    /var/lib/cortexfs/storage/current
       ▼
┌──────────────────┐   spawn / unit   ┌─────────────────────────┐
│ agent runtime    │─────────────────▶│ cortexfs-object-runner  │
│ (per agent)      │   exec tool/     │ (model / agent / tool)  │
└──────────────────┘   model/agent    └─────────────────────────┘
       │
       │ optional MCP projection (explicit adapter, not root ABI)
       ▼
┌──────────────────┐
│ ctxmcp           │  maps ordinary tools ↔ MCP stdio
└──────────────────┘
```

| Process | Job | Must not |
| --- | --- | --- |
| `cortexfs-mount` | Project generation tree as `/ctx` | Own sessions, call providers |
| `cortexfs-agent-runtime` | Socket accept, durable record, launch policy | Become a second ABI root |
| `cortexfs-object-runner` | One-shot model/agent/tool execution | Long-lived daemon state |
| `ctx` / `tsh` / `ctxchat` / `ctxterm` | Host UX over the same files/sockets | Invent parallel control planes |
| `ctxmcp` | Explicit MCP adapter | Create `/ctx/mcp` root class |

**Invariant:** development refresh is **Git commit or process restart**. No
background watchers, polling loops, or hot-reload subcommands.

---

## 3. Target crate architecture

Move from “one lib does everything” to **thin crates with one job**. Prefer
**feature flags first**, physical split second (same module tree, lower risk).

### 3.1 Target graph

```text
cortexfs-abi          pure types, path grammar, request frames, policy enums
        ▲
cortexfs-support      plain fs, jsonl, layout, path checks (no FUSE, no HTTP)
        ▲
cortexfs-runtime      socket, session record, egress, control handshakes
        ▲
cortexfs-object       install / replace / executor / runner helpers
        ▲
cortexfs-fuse         fuser projection only
        ▲
cortexfs (facade)     re-exports + optional features; bins depend on facade
        ▲
bins: ctx, tsh, mount, runner, agent-runtime, ctxmcp
sdks: cortexfs-module, tool-sdk, agent-sdk, runtime-client   (depend only on narrow crates)
```

### 3.2 Crate rules

| Crate | Allowed deps | Forbidden |
| --- | --- | --- |
| `cortexfs-abi` | `serde`, small pure crates | `fuser`, `nix` process, HTTP, parquet |
| `cortexfs-support` | `abi`, `nix` fs bits, serde_json | FUSE, provider HTTP, agent launch |
| `cortexfs-runtime` | `abi`, `support` | FUSE mount loop, object install stages |
| `cortexfs-object` | `abi`, `support`, runtime-client, tool-sdk | FUSE server |
| `cortexfs-fuse` | `abi`, `support`, `fuser` | object executor, provider HTTP |
| SDKs | `runtime-client` (+ minimal abi types) | full `cortexfs` monolith when avoidable |

**MCP stays an adapter binary** (`cortexfs-mcp` / `ctxmcp`). It may call
support helpers; it must not force a root ABI class.

### 3.3 Near-term (no directory move): Cargo features

Until physical crates land, gate heavy stacks behind features on `cortexfs`:

```toml
[features]
default = ["fuse", "columnar", "runtime", "object"]
fuse = ["dep:fuser"]
columnar = ["dep:arrow-array", "dep:arrow-schema", "dep:parquet"]
runtime = []
object = []
cli-support = []
```

| Feature | Modules (indicative) |
| --- | --- |
| `fuse` | `fuse/**`, mount driver projection |
| `columnar` | `support/columnar.rs` and callers |
| `runtime` | `runtime/**` socket + record |
| `object` | `object/**` install + executor |
| `cli-support` | `cli/**` shared by bins |

Acceptance: `cortexfs-runtime-client` and SDKs can depend on
`default-features = false` plus only what they need, without linking `fuser`
or parquet when unused.

---

## 4. Module layers inside `crates/cortexfs/src`

Layers are **directional**. A module may depend on the same layer or a lower
layer only. Violations need an explicit design note, not a silent `use`.

```text
L0  abi, policy          pure grammar and allowlist types
L1  support              plain files, jsonl, layout, path, process helpers
L2  authority, context   identity, packs, control inspection
L3  provider, tool       model registry, tool schema/state (no FUSE)
L4  reference, mount     storage generations, mount table
L5  agent, runtime       launch, child, socket, durable session
L6  object               install/swap/residue + executor/runner
L7  fuse                 projection only
L8  bin/*                process entrypoints; may use L0–L7, not the reverse
```

### 4.1 Allowed / forbidden edges (hard rules)

| From → To | Rule |
| --- | --- |
| `fuse` → `object::executor` | **Forbidden** (projection must not run tools) |
| `support` → `agent` / `runtime` / `fuse` | **Forbidden** |
| `abi` → anything above L0 | **Forbidden** |
| `object::executor` → `fuse` | **Forbidden** |
| `bin/*` → library modules | Allowed |
| library → `bin/*` | **Forbidden** |
| `runtime` → `object::install` | Avoid; prefer callbacks/traits at boundary |
| SDK / mcp → `support::plain` | Prefer a narrow `pub` façade later; today `#[doc(hidden)]` is a temporary escape hatch only |

### 4.2 One job per module (enforcement targets)

| Module | One job | Split when |
| --- | --- | --- |
| `object/executor` | Run model/agent/tool once | Already multi-file; finish `ExecError` then stop growing |
| `object/install` + `swap` | Atomic object lifecycle | Keep residue/replace as siblings |
| `runtime/socket` | Accept + frame + peer policy | `exec` / `stream` / `spawn` by stage |
| `agent/launch` | systemd/user unit lifecycle | unit / receipt / alias files |
| `support/columnar` | Durable JSONL backing store | wal / manifest / shard / claim |
| `bin/ctx` | Host CLI parsing + UX | Keep domain logic in library modules |

**Size policy (clippy-aligned):**

- New functions: ≤ 120 lines unless an `#[expect]` cites an issue.
- New non-test files are hard-capped at 120 lines; any debt over 120 lines in production
  files must be reduced in follow-up commits.
- `scripts/source-budget.sh` is the single executable authority for all-Rust and production
  Rust baselines, targets, and ratchets; do not copy snapshot numbers into docs.
- Ratchet forbids moving production code into `tests`, one-lining, source-path/include
  tricks, or adding thin wrapper files to evade the budget.
- Do not add `too_many_lines` expects on greenfield code.

### 4.3 Import policy

| Pattern | Policy |
| --- | --- |
| `use crate::*` | **No new uses.** Shrink existing when touching a file. |
| `pub use imports::*` in `lib.rs` | Freeze; do not expand the prelude. Prefer explicit paths in new modules. |
| `exports.rs` | Public ABI-facing re-exports only; not an internal kitchen sink. |
| Glob imports in production | Forbidden (workspace lint); tests keep documented exceptions only. |

### 4.4 Mechanism and policy

Authority enforcement is split across two boundaries:

| Boundary | Owns | Must not |
| --- | --- | --- |
| Mechanism (`authority`) | principal class, path lookup, Linux identity and mode bits, mount visibility/options, stable denial mapping | parse policy formats or assume one policy implementation |
| Policy (`policy`) | subject/object/permission decisions through `PolicyEvaluator`; v0 text parsing through `PolicyV0` | bypass mechanism checks or grant from prompts, schemas, skills, or model output |

`PolicyV0` is the built-in evaluator, not part of the enforcement mechanism.
Alternative evaluators must be injected as already-loaded, host-owned policy
state. A positive policy decision never bypasses principal, path, Linux, or
mount checks, and any refusal still refuses.

The boundary applies beyond tool execution. Schedule `requires` validation,
post-routing model authorization, and named network egress gates consume
`PolicyEvaluator`; only control-file adapters parse `PolicyV0`. Provider egress
routing produces a validated immutable plan before the runtime allocates
directories, sockets, relay threads, or upstream HTTP processes.

Large modules follow the same ownership split. For example,
`runtime/egress/{plan,secret,target}.rs` owns egress decisions while
`runtime/egress.rs` owns relay lifetime, and
`runtime/record/schedule/{record,complete,advance}.rs` owns schedule state
transitions outside the child-channel receipt mechanism.

---

## 5. Error architecture

### 5.1 Three tiers

```text
Tier A — Domain stable enums
  SocketRuntimeError, AgentLaunchError, InstallError, …
  Eq/PartialEq for tests; Display + std::error::Error required.
  Prefer source() / nested variants over map_err(|_e| UnitVariant).

Tier B — Process-local typed shells
  ExecError (object runner), StopError, CLI simple errors
  Stable user-visible message strings; Display + Error.
  Used where failures are mostly stringly today but still process-local.

Tier C — Binary boundary
  Exit codes + one-line stderr. May convert Tier A/B with .to_string()
  or message() exactly once at main.
```

### 5.2 Rules

1. **Library / runner internals:** no new `Result<T, String>` for errors.
   (`Result<String, E>` where `String` is a success payload is fine.)
2. **User-visible text is ABI** when tests or docs pin it. Changing wording
   needs an explicit product decision.
3. **IO mapping:** prefer `with_io("prefix", &error)` style helpers so text
   stays `prefix: {error}` and pedantic stays happy.
4. **No parallel Empty/Missing/Invalid enums** — reuse
   `ControlLineIssue` / `PathLayoutIssue` families (see AGENTS.md).
5. **Migration order for `object/executor`:**
   `path/policy/wire` → `model/inference` → `agent` shell → `tool` + `run`
   (partially underway; finish before starting unrelated refactors in executor).

### 5.3 Target end state for executor

```text
object/executor/**  Err type = ExecError only
executor::run       Result<ExitCode, ExecError>
bin/runner main     map_err once to stderr + exit code
From<String> for ExecError  removed after migration
From<ExecError> for String  optional, binary-only convenience
```

---

## 6. Object / tool / MCP placement

Product rule (unchanged): **MCP is not a root class.** Capabilities appear as
ordinary tools under `/ctx/tool/...` with optional `.d/mcp` locator control.

```text
install path:  object manifest + controls (description, schema, cap, policy, mcp?)
runtime path:  ctxmcp reads locator → stdio MCP server → Tool SDK frames
agent path:    same tool execution + policy as any other tool
```

Internal rule: MCP client code lives in **`cortexfs-mcp`**, not inside
`fuse` or root projection. Shared validation of locator JSON may live in
`object/mcp` as install/bootstrap validation only.

---

## 7. Data and durability architecture

Already fixed by product design; internal code must respect it:

| Concern | Mechanism | Code gravity |
| --- | --- | --- |
| Control-plane publish | write temp → `rename` to `*.req.json` / control files | `support::plain`, authority helpers |
| Session history | append-only JSONL (+ optional columnar store) | `runtime/record`, `support/columnar` |
| Generation switch | clone → validate → atomic `current` symlink | `reference/storage` |
| Tool/agent install | stage + receipt + swap | `object/install`, `swap`, `residue` |
| Cancellation / stop | receipt-bound plans | `agent/stop`, runtime stop traits |

Do not introduce in-memory “workflow engines” that bypass these files.

### 7.1 Performance engineering boundary

Performance work is measure-first architecture work, not a license to weaken
the ABI or safety model. This section defines acceptance boundaries; it does
not claim that any candidate optimization has landed. The detailed workflow is
the project skill at `.agents/skills/cortexfs-performance/SKILL.md`.

| Boundary | Required rule |
| --- | --- |
| Evidence | Compare equivalent release workloads, measure baseline noise, and accept a gain only above `max(3%, 2 × noise)` while meeting the task's p95 and RSS gates. |
| Safety / portability | Keep `unsafe` forbidden; do not use `target-cpu=native`, global `target-feature`, or default CUDA. Optional acceleration requires a tested, equivalent CPU fallback. |
| Cache truth | Bound caches and invalidate them with the generation, commit/restart, policy, provider, model, or config identity that supplies their data. A cache never grants authority. |
| Semantics | Preserve paths, framing, limits, errors, ordering, durability, permissions, cancellation, and fallback behavior. |
| Review | Separate writer and reviewer; review the raw runs and diff, not only a summarized speedup. |

Socket/JSONL framing, agent output, context rendering, FUSE metadata, provider
catalog/config caches, thread pools, and optional accelerators are investigation
surfaces only. Profile the exercised path before choosing one; do not document
an unmeasured rewrite as an implemented optimization.

---

## 8. Public API surface policy

`lib.rs` currently re-exports broadly for historical reasons. Target:

| Visibility | Audience |
| --- | --- |
| `pub` in `exports` / documented modules | External crates, integration tests |
| `pub(crate)` | Cross-module inside cortexfs |
| `pub(super)` / private | Module internals |
| `#[doc(hidden)] pub` | Temporary for sibling bins (mcp); track and shrink |

**Shrink list (ongoing):**

1. Stop new `pub use` of implementation modules from `lib.rs`.
2. Give `cortexfs-mcp` a narrow module (`cortexfs::fsutil` or support façade)
   instead of growing `#[doc(hidden)]` on random plain helpers.
3. SDKs depend on `runtime-client` + abi types, not full projection stack.

---

## 9. Testing architecture

| Layer | Location | Role |
| --- | --- | --- |
| Unit | `src/**/tests*.rs`, `tests/unit/**` | Pure logic, parse, policy |
| Executor | `object/executor/tests/**` | Tool loop, process limits |
| Integration | `tests/*.rs`, FUSE under `tests/mounts/cortexfs` | Mount + ABI |
| Live | skills / scripts with `smollm2:135m` | Real model path only when asked |
| MCP e2e | `cortexfs-mcp/tests` | Adapter only |

Rules:

- FUSE mount point remains `tests/mounts/cortexfs` (no fixtures stored there).
- Prefer moving giant `#[cfg(test)]` blocks out of hot production files when
  touching them.
- Do not expand frozen test-parent glob exceptions (AGENTS.md).

---

## 10. Migration roadmap

Execute in order. Each phase must keep `cargo clippy -D warnings` and relevant
tests green. Prefer **vertical slices** over horizontal rewrites.

### Phase A — Error consistency (in progress)

- [x] Introduce `object/executor::ExecError`
- [x] Migrate `args`, `call`, `exec`, `path`, `policy`, `wire`, (partial) `output`/`agent` loop
- [ ] Finish `model`, `inference`, `agent` shell, `tool`, `executor::run`
- [ ] Add `Display` + `Error` to remaining stable domain enums without changing variants
- [ ] Remove `From<String> for ExecError` when no callers remain

**Exit criteria:** no `Result<_, String>` error side under `object/executor`
(except success payloads named `String`).

### Phase B — Layer hygiene

- [ ] Ban new `use crate::*`; convert files when touched
- [ ] Document layer of each top-level module in `lib.rs` rustdoc
- [ ] Split next god file only when a feature change already requires editing it
  (order: `runtime/socket/exec` → `agent/launch` → `support/columnar`)
- [ ] Replace temporary `#[doc(hidden)]` support exports with one façade module

**Exit criteria:** dependency graph from `fuse`/`support` shows no upward edges
in new code; CodeGraph or manual review on PRs.

### Phase C — Feature flags

- [ ] Add `fuse` / `columnar` features; `cfg` the modules
- [ ] CI matrix: full features + `no-default-features` + minimal SDK build
- [ ] Measure `cargo build -p cortexfs-runtime-client` / tool-sdk link set

**Exit criteria:** SDK build without `fuser` and without parquet.

### Phase D — Physical crate split

- [ ] Extract `cortexfs-abi` (types only)
- [ ] Extract `cortexfs-support`
- [ ] Move runtime / object / fuse behind separate crates or keep features if
      split cost > benefit
- [ ] Facade crate preserves versioned path deps for bins

**Exit criteria:** workspace compile graph matches §3.1; no behavior change.

### Phase E — Bin packaging

- [ ] Each bin depends only on features/crates it needs
- [ ] `ctx` stays UX; domain logic remains in library modules
- [ ] Align packaging/systemd units with process table in §2

---

## 11. PR / review checklist (architecture)

Reviewers ask:

1. **ABI:** Any new root path or orchestration entry? (Must be no.)
2. **Layer:** Does this module only call same/lower layers?
3. **Error:** New `Result<_, String>`? Missing `Display`/`Error` on public errors?
4. **Size:** New expects for `too_many_lines` / `too_many_arguments` without split?
5. **Deps:** New heavy dependency justified, and feature-gated if optional?
6. **Process:** Any new background watcher/poller/hot reload? (Must be no.)
7. **Reuse:** Existing `support::plain` / `path` / `process` / layout helpers checked first?

---

## 12. What “done” looks like

```text
Product ABI        unchanged, still boring files + sockets
Internal graph     layered crates/features; SDKs stay thin
Errors             typed; strings only as success payloads or final stderr
Modules            one job; god files split on natural edit boundaries
Bins               process roles match §2; no logic-only-in-bin duplication
MCP                adapter only; tools remain ordinary objects
```

This document is the north star for refactors such as `ExecError`, crate
features, and module splits. Prefer small PRs that move one checkbox in §10
over multi-thousand-line “architecture rewrites.”
