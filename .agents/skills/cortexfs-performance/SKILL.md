---
name: cortexfs-performance
description: Use when changing CortexFS performance, latency, throughput, memory, token usage, cost, caching, process reuse, streaming, or when comparing CortexFS with another agent runtime.
---

# CortexFS Performance Engineering

Use this skill before changing code for a performance claim. Read `AGENTS.md`,
`docs/internal-architecture.md` §7.1, the relevant normative ABI, and
`ctx-agent-benchmark` when live agents are involved.

## Non-negotiable boundaries

- Use release builds for reported measurements. Debug data is diagnostic only.
- Run Cargo, Clippy, and tests serially through the repository wrappers.
- Do not add `unsafe`, `target-cpu=native`, global `target-feature`, or default
  CUDA. Optional acceleration must retain an equivalent CPU fallback and cache
  semantics.
- Do not weaken sandboxing, cancellation, credential isolation, auditing,
  atomic rename, or file/socket ABI behavior for speed.
- Do not add a watcher, polling daemon, hot reload path, or a second workflow
  ABI. Measurement-side sampling must be bounded to one benchmark invocation.
- Never omit failures, timeouts, missing usage, or outliers from raw results.

## 1. Freeze the experiment

Before editing, write a hypothesis naming one metric and one suspected path.
Record in the raw run manifest:

- Git commit and dirty state; `Cargo.lock` and release binary SHA-256;
- Rust/CortexFS/Pi or comparison-runtime versions;
- kernel, CPU, governor, RAM, cgroup limits, and concurrent system load;
- dataset, system prompt, tool schema, policy, workspace, and config hashes;
- provider, resolved model, route, thinking/effort, token budget, and timeout;
- warm/cold policy, sample order, seed, repetitions, and price-table version.

Separate workload identity from treatment identity. A comparison fingerprint
must not include the arm name or runtime version.

## 2. Establish baselines

Use two baselines when applicable:

1. A deterministic local replay with fixed delay, chunks, usage, and tool calls
   to isolate runtime overhead.
2. A live provider run for end-to-end quality and latency, only with explicit
   authorization.

Run an unchanged release against itself first, using paired A/A order. For each
metric `m`, calculate:

```text
noise_m = exp(P95(abs(ln(m_A1 / m_A2)))) - 1
```

Use AB/BA or ABBA ordering for candidate comparisons. Warm up every actual
runtime without counting those calls. Bootstrap by independent task/sample ID:
repeated epochs and the two pairs inside ABBA stay in the same resampling
cluster and never inflate the independent sample count. Fewer than 100
successful independent tasks per arm makes p95 descriptive, not conclusive.

## 3. Measure the complete boundary

Report these separately:

- quality and runtime success rate;
- cold process E2E and warm-request E2E;
- TTFT to the first non-empty user-visible text delta;
- decode throughput from first to last output token;
- end-to-end token goodput;
- input, output, cache-read, cache-write, and reasoning usage coverage;
- peak incremental RSS and cgroup memory events;
- tokens/success and versioned API-price-equivalent cost/success.

Token/cost totals include failed attempts, divide by successful outcomes, use
identical component semantics, one versioned price table, and require complete
attempted-request usage coverage.

Subscription traffic has no attributable per-run bill. Label monetary figures
as API-price-equivalent and report actual subscription incremental cost as
`n/a`.

For RSS, prefer a dedicated cgroup and record `memory.current`, `memory.peak`,
`memory.swap.current`, and `memory.events`. Bind each PID to its `/proc` start
time. Compare the whole owned process tree; do not compare a thin `ctx` client
against an entire competing agent. Subtract a pre-request idle median when a
persistent listener is measured.

## 4. Change one narrow path

Profile before editing. Reuse existing helpers and preserve observable error,
stream, timeout, terminal, and process behavior. Keep one primary optimization
per comparison so the result is attributable. Run `scripts/source-budget.sh`
for every changed Rust slice.

## 5. Acceptance gates

A cross-runtime comparison also requires equivalent isolated workspaces and
complete hashed runtime contracts (binary, provider/model/thinking, system
prompt, tools, policy, permissions, and cwd). Different workspace or tool
contracts make the result inconclusive even for no-tool prompts.

A performance change is acceptable only when all are true:

- relevant correctness, ABI, and hidden quality tests pass;
- runtime success and quality are non-inferior at the declared 95% CI bound;
- no claimed metric wins by trading away p95/p99, RSS, token use, or safety;
- no swap, OOM, `memory.high` event, secret leak, or cleanup failure occurs;
- the paired bootstrap 95% lower bound of the claimed improvement is greater
  than `max(3%, 2 * noise_m)`;
- every counted and warm-up request has complete bounded protocol evidence,
  with redaction/truncation/drop counts explicit, process groups verified empty,
  pre/post binary/config identities stable, and immutable artifact hashes recorded;
- an independent reviewer reconstructs the summary from those artifacts.

A point estimate, mean-only win, five-task smoke run, or graph generated by the
author is not sufficient evidence.

## Required artifacts

Store outside normal source paths unless the repository already defines a
benchmark artifact directory:

- immutable per-sample JSONL, including failures;
- raw protocol/event frames with secrets redacted, never replaced by summaries;
- monotonic timing boundaries and process/cgroup memory evidence;
- A/A and AB/BA or ABBA order, seeds, exclusion log, and fingerprints;
- summary generator version and generated summary/report;
- reviewer verdict naming every failed or unproven gate.

The reviewer must inspect raw data, fingerprints, process ownership, usage
coverage, cleanup receipts, and the source diff. If the reviewer cannot
reproduce a headline number, report the result as inconclusive and revert or
retain the change without a performance claim.
