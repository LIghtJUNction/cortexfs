# FUSE harness validation — 2026-09-05

## Scope and delivery

This is a first architecture/refactoring/deployment iteration, not a declaration
of Pi feature parity. See [harness.md](../harness.md) for replacement boundaries.

- Pi global default: `openai-codex/gpt-6-astra`; verified in the model catalog.
  This changes new sessions, not the already-running coding session.
- Development stayed on `main`; implementation commits were pushed to origin.
- Server: the explicitly requested SSH host `DmitUbuntu`, Ubuntu 26.04, with
  verified existing host key. Production mount remains `/ctx`.
- Deployed package: `0.1.21`, source commit `597e3c49c4bf`. Production Rust
  executables are from `fa6ada751e6e`; the later commit changes unit settings,
  their tests, and documentation, not production Rust behavior.
- Private receipts, executable/package hashes, raw requests, responses, failed
  attempts, and configuration snapshots are retained under server directory
  `/var/lib/cortexfs/deploy/`; the final receipt directory is `597e3c49c4bf/`.
  Credentials and raw private traces are not published in this repository.

## Changes actually delivered

1. Preserved the existing uncommitted work in a private checkpoint. Published
   validated interaction-v2 types and narrow DRY changes, but excluded unwired
   Session Slave/Master prototypes and unsafe shared-session threaded admission.
   Installed sockets remain v1 one-shot; v2 runtime behavior is explicitly planned.
2. Context rendering now reserves selected recent observations before allocating
   remaining UTF-8 byte budget to a summary. Zero/tiny budgets cannot overflow
   through a truncation marker. Formatting helpers are shared, not duplicated.
3. OpenAI-compatible SSE tool-call accumulation ignores empty continuation
   identity fields. A real gateway repeated `id:""`/`name:""` after the first
   chunk, overwriting the original identity and breaking tool execution. The
   captured shape reproduces failure before the fix and passes afterward.
4. Fixed Bubblewrap/systemd compatibility, including non-root execution.
   Namespace creation must be allowed; masked parent procfs prevents a child
   from mounting its own procfs. Hostname/syslog syscall denial and CAP_SYSLOG
   removal remain explicit; private PID/UTS/proc, dropped child capabilities,
   policy, receipts, cgroups, and NoNewPrivileges remain. The trusted root
   launcher's increased procfs visibility is a documented trade-off, **not
   equivalent parent-process hardening**. See the runtime sandbox contract.

## Evidence

| Check | Observed result | Evidence |
| --- | --- | --- |
| Serial repository gates | format, source budget, check, Clippy, tests pass | implementation commit hooks and retained local logs |
| Context regression | two new tests fail before fix; six context behavior tests pass in release afterward | `compact-release-before.log`, `compact-release-after.log` in private checkpoint |
| SSE regression | accumulator fails on empty identity continuations before fix; four selected stream tests pass afterward | `sse-regression-before.log`, `sse-regression-after.log` |
| Real model + FUSE tools | three dependent calls: read `37/harbor`, write `74/HARBOR`, independently read result | `597e3c49c4bf/live-tools/`, terminal status `ok`, 43 frames, 12.262 s |
| Policy enforcement | denied until both caller and target policies permit the isolated test label | preceding failed attempts retained; no wildcard grants added |
| Custom compactor + continuation | old `CHECKPOINT` and compactor-only marker recalled in the next turn | `597e3c49c4bf/live-compact/`, 3.540 s seed and 2.522 s recall |
| Durable history | exact byte prefix retained across compaction | 11,776 bytes before, 12,119 after; prefix assertion passes |
| Component replacement | custom `loop.d/fixture` executes and yields a declared native call | `sdk-proof.jsonl`; complete custom-tool sequence does **not** pass |
| Deployment health | FUSE ready, 8/8 entries loaded, coder ping succeeds, Discord and socket services active | final health files and systemd checks |

These timings are individual integration observations, not p95, a benchmark,
or a speed/cost/quality advantage over Pi. The denied/failed attempts are not
included in a claimed throughput average because no such average is claimed.

## Model and migration exceptions

The existing default `lmm/deepseek-v4-flash-0731` returns upstream HTTP 500 with
no available channel. The default `main` route was **not silently changed**.
An isolated UID-1000 agent used the working `qwen3.8-flash` model through the
same provider registry, secret, and routing machinery. That model was added
only as an explicit non-default test entry. No supplier-specific core branch
or credential was added to code.

`doctor` still exits 69 for retained legacy `coder`, `reviewer`, and `worker`
names. Only these exact migration notices were accepted; unrelated ABI errors
were not ignored. Existing definitions and history were not deleted. Legacy
labels' original target-policy grants were restored in ten tool-policy files,
without introducing new wildcard authority.

## Remaining work — not passed

- **Long individual history rows:** the existing context parser filters JSONL
  rows larger than 16 KiB before selection. A roughly 48.5 KiB user row remains
  durable but never reaches the compactor. The successful live compaction test
  intentionally uses a smaller, still-oversized-for-context row. Silent loss of
  long tool/user observations needs a separate bounded parsing design and tests.
- **External Tool SDK through FUSE:** the SDK fixture installs and its receipt
  verifies; its backing executable is ELF. The mounted entry is a gate wrapper.
  A custom loop reaches `example.echo`, but the execution path invokes the
  reference runner and reports an unimplemented tool/empty SDK output. Native
  SDK fixture tests are not evidence that this mounted path works. Fixing this
  must preserve receipt binding and authority; do not bypass the FUSE gate.
- Persistent interaction v2, durable per-session concurrent ownership/replay,
  arbitrary runtime-loaded policy evaluators, and a universal component loader
  are not implemented by this iteration.
- No paired live Pi benchmark or full security audit was performed.

## Operational state

The isolated test socket is stopped after verification. Its controlled
workspace, custom compactor/loop fixtures, SDK install receipt, and raw evidence
are retained for reproduction; default test-agent controls are restored.

Initial failed deployments exercised rollback to the exact installed-file and
configuration preimage. The final automatic rollback timer was disarmed only
after real tool execution and bounded health checks passed. Backups remain.
A later manual rollback must first preserve post-deployment session facts and
verify the managed compatibility drop-in has not been modified; it is not a
license to overwrite newer configuration or history.
