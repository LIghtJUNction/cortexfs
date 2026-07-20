---
name: ctx-agent-benchmark
description: Benchmark live CortexFS multi-agent system with Inspect AI ctx CLI. Use when asked test ctx agents, compare architect/coder/reviewer/worker performance, measure quality, latency, TTFT, throughput, tokens or errors, diagnose benchmark regressions, produce repeatable agent evaluation report.
---
# CTX Agent Benchmark

Use repository benchmark as a single implementation. Do not copy its JSONL parser
or metric logic into this skill.

## Run benchmark

1. Read `AGENTS.md`, apply `cortexfs-test`, complete its live-agent proof every
   selected role; abort if fails.
2. Obtain explicit authorization configured-provider calls, cost, uploads.
3. Run all roles sequentially repository root:

```bash
rtk proxy ./inspect_benchmark/run_benchmark.sh system --output-dir results
```

Use Inspect logs focused quality run when needed:

```bash
rtk proxy ./inspect_benchmark/run_benchmark.sh ctx coder
```

Compare `summary.json` only across equivalent datasets, repeats, agents, and
provider routes. The benchmark requires pre-existing canonical listeners and
never starts or stops agents.
It creates unique sessions only after proving they did not exist, cancels exact
canonical run IDs on timeout, archives only an owned, inactive, non-current,
unreferenced session selected alone by exact GC preview.
Cleanup refusal is retained in receipts and makes the run nonzero.

## Report

Report runtime success rate and exact accuracy separately.
Include mean/p50/p95 latency, mean/p50/p95 TTFT, token totals, tokens/s or
chars/s, and errors grouped by code.
If start latency is unavailable, report it as `n/a`; report preflight timing
instead.
Include delegated live-proof outcome, configured models, cleanup outcomes, and
every skipped unavailable metric.

Do not add watchers, polling loops, provider-specific core branches, or second
workflow ABI.
