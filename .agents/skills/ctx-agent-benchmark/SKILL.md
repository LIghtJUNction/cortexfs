---
name: ctx-agent-benchmark
description: Benchmark the live CortexFS multi-agent system with Inspect AI and the ctx CLI. Use when asked to test ctx agents, compare architect/coder/reviewer/worker performance, measure quality, latency, TTFT, throughput, tokens or errors, diagnose benchmark regressions, or produce a repeatable agent evaluation report.
---

# CTX Agent Benchmark

Use the repository benchmark as the single implementation. Do not copy its JSONL parser or metric logic into the skill.

## Run the benchmark

1. Read the repository `AGENTS.md` and apply the `CortexFS Test` skill.
2. Confirm the live surface with `rtk ctx status`, `rtk ctx doctor`, the `/ctx` mount, `rtk ctx ping agent/ROLE`, and `rtk ctx agent status ROLE` for every selected role. Require one JSON `pong` and an exact first status line of `idle`. Abort on any failure; do not treat an ABI placeholder socket as a listener.
3. Confirm whether the user authorized real configured-provider calls. The system benchmark sends real prompts and can incur cost.
   Do not run it without that explicit authorization.
4. Run all roles sequentially:

From the repository root:

```bash
rtk proxy ./inspect_benchmark/run_benchmark.sh system \
  --output-dir results
```

5. Use Inspect logs for a focused quality run when needed:

```bash
rtk proxy ./inspect_benchmark/run_benchmark.sh ctx coder
```

6. Compare `summary.json` files from equivalent datasets, repeats, agents, and provider routes. Do not compare unlike configurations as a regression.

The benchmark requires pre-existing canonical listeners. It never starts or stops agents automatically. It creates unique sessions only after proving they did not exist, cancels exact canonical run IDs on timeout, and archives only an owned, inactive, non-current, unreferenced session selected alone by the exact GC preview. Cleanup refusal is retained in receipts and makes the run nonzero.

## Report

Report runtime success rate and exact accuracy separately. Include mean/p50/p95 latency, mean/p50/p95 TTFT, token totals, tokens/s or chars/s, and errors grouped by code. Start latency is unavailable and must be reported as `n/a`; report preflight ping/status timing instead. State whether `/ctx` was a real FUSE mount, which canonical `ctx ping agent/ROLE` probes returned pong, which configured models were exercised, cleanup outcomes, and every skipped or unavailable metric.

Use unique benchmark sessions. Do not add watchers, polling loops, provider-specific core branches, or a second workflow ABI.
