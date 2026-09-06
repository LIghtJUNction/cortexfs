---
name: ctx-agent-benchmark
description: Benchmark live CortexFS agents with Inspect AI and the ctx CLI. Use when comparing current architect/executor/product-manager or custom agents, measuring quality, latency, TTFT, memory, tokens, cost-equivalent usage, or diagnosing benchmark regressions.
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

Use the repository `paired` mode for ctx/Pi ABBA or A/A evidence; keep
`--retain-frames` enabled for reportable runs. Warm-ups are retained but not
scored. Bootstrap/noise must cluster repeated epochs and both ABBA pairs by
`sample_id`; repeated calls are not independent tasks.

Use Inspect logs focused quality run when needed:

```bash
rtk proxy ./inspect_benchmark/run_benchmark.sh ctx executor
```

Compare `summary.json` only when workload fingerprints match. Agent identity,
rendered prompt/tools/cwd/policy hashes, binaries, and memory method are
treatment evidence; the observation count still belongs to the workload. The
benchmark requires pre-existing canonical listeners and
never starts or stops agents.
It creates unique sessions only after proving they did not exist, cancels exact
canonical run IDs on timeout, archives only an owned, inactive, non-current,
unreferenced session selected alone by exact GC preview.
Cleanup refusal is retained in receipts and makes the run nonzero.

## Report

Apply `cortexfs-performance`; summary-only comparison is never a passing
performance verdict. A five-task smoke and any ctx/Pi run with unequal
workspace/tool contracts, incomplete cgroup coverage, mutable artifacts, or
missing independent review stays explicitly inconclusive. Report runtime
success rate and exact accuracy separately; failures receive zero quality
credit and incomplete usage is excluded with its denominator shown.
Include mean/p50/p95 latency, mean/p50/p95 TTFT, token totals, tokens/s or
chars/s, and errors grouped by code.
If start latency is unavailable, report it as `n/a`; report preflight timing
instead.
Include delegated live-proof outcome, configured models, cleanup outcomes, and
every skipped unavailable metric.

Do not add watchers, polling loops, provider-specific core branches, or second
workflow ABI.
