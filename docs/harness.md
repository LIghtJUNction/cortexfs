# FUSE-first replaceable harness

This is the delivery map for the default harness, not a second filesystem ABI.
The architectural layers remain those in [architecture.md](architecture.md).
Normative contracts live in [spec/](spec/README.md).

## Invariants

FUSE projects the authority and durable state boundary. Clients do not bypass
Linux identity, receipts, policy, mounts, or session recording by replacing a
component. Writes use the existing atomic same-directory `*.req.json` handoff,
outbox responses, and append-only audit facts where a queue operation applies.
Agent/session sockets are live transports, not a second source of history.

The default harness composes model routing, an agent step driver, context
projection, tools, policy, and a frontend. Replacement means selecting an
ordinary executable or configuration at an existing boundary, not editing the
FUSE executor or loading untrusted Rust trait objects into its process.
Changes are deployed at a Git commit boundary with explicit process restart;
there is no file watcher or hot-reload service.

## Replacement matrix

| Part | Default | Supported boundary | Verification |
| --- | --- | --- | --- |
| Model | registry-selected `main` alias | provider registry, routes, secrets, model adapter | real routed request; do not special-case a vendor |
| Agent step | packaged SDK agent | Agent SDK executable; `loop` and `loop.d/<name>` | next observation reaches the next step |
| Context | recent history / optional summary | `compact.strategy`, `compact.d/<name>`, `cortexfs.compact/v1` | byte bound, recent observations retained, raw history unchanged |
| Tools | `tsh` with governed objects | Tool SDK; `invoke.strategy`, `invoke.d/<name>` | actual tool result recorded, including denial/error |
| Instructions | system/template/rules/skills | object-local text and bounded context inputs | next run sees the change; authority unchanged |
| Policy | host evaluator plus Linux restrictions | static evaluator interface and per-object policy files | a deny remains a deny through a replacement |
| Frontend | ctx / terminal / channel adapters | interaction v1; Channel SDK driver | client is not the history owner |
| Module lifecycle | static registry | `cortexfs.module.socket/v1` external process contract | lifecycle and framing fixtures; not universal dynamic loading |

See [extensions.md](extensions.md) for authoring and installation examples.
Policy evaluators are not currently an arbitrary user-loadable executable
slot. Linux/FUSE authority and durable session ownership are intentionally
host-owned invariants, not replaceable plugins. A universal component loader
or in-process Pi-compatible extension API is **not implemented**.

## Current runtime versus target design

V1 remains a one-request connection with serial admission. Typed interaction
v2 frames do not activate asynchronous Master/Slave service behavior. The
unwired prototype was excluded from deployment after review found shared
`current`/`current_run` races and cancellation not bound to the requested
session. The original work is retained in a private checkpoint for follow-up.

Before enabling v2, require all of:

- durable per-session execution ownership, independent of the Agent `current`
  convenience index;
- atomic event-sequence allocation and crash-safe replay;
- peer-authenticated negotiation and synchronized bounded frame writers;
- session-specific cancellation and disconnect semantics;
- real socket tests for simultaneous sessions, saturation, restart, replay,
  malformed frames, and recovery. Fake worker concurrency is not sufficient.

## Comparison with Pi

The useful comparison is common tasks and replaceability, not matching product
names. Pi's model/agent/application separation maps to protocol adapters,
agent SDK/runtime, and ctx/channel clients. Pi's in-process extension ergonomics
are different from CortexFS's executable/Unix isolation; neither implies the
other's API compatibility. CortexFS's distinctive core is the mounted file ABI,
Linux-enforced object authority, and durable session facts.

For a fair performance or quality comparison, pin both revisions, one provider
and model, identical tool schemas, tasks, budgets, timeouts, and concurrency.
Record complete transcripts, success and failure, model/tool counts, tokens,
TTFT, wall time, p95, and peak RSS. Local deterministic fixtures verify contracts;
real-provider runs verify integration. Neither alone establishes superiority.
No speed, cost, or quality win is claimed by this architecture change.

## Acceptance for this iteration

1. Format, source-budget, core/runtime-client/context tests pass serially.
2. Oversized summaries cannot remove selected recent observations or exceed
   even a zero/tiny UTF-8 byte budget; raw histories remain intact.
3. Deploy a commit-identified package only with an exact rollback artifact,
   configuration backup, health checks, and bounded service interruption.
4. On a real FUSE mount, exercise the configured provider through the Agent,
   a dependent multi-step tool loop, session continuation, custom context
   projection, and independently selected extension fixtures.
5. Retain raw results and clearly distinguish live-model evidence, deterministic
   fixture evidence, untested surfaces, and planned work.
