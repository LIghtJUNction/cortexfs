---
name: ctx-harness-evaluation
description: Run and interpret the CortexFS agent harness contract suite when validating protocol, authorization, ownership, cancellation or channel regressions. Use docs/evaluation.md for the separate live-model evaluation path.
---
# CortexFS Harness Evaluation

Read `AGENTS.md` and `docs/evaluation.md` from the repository root. The maintained
runner is `evals/harness/run.py`; fixtures live beside the Rust modules and the
required evidence is registered in `evals/harness/suites.json`.

```bash
python3 evals/harness/run.py --list
python3 evals/harness/run.py --suite context --suite wire
python3 evals/harness/run.py --workspace --timeout 6600
```

Choose focused groups for the changed contract; use `--workspace` for the full
CI gate. The runner serializes Cargo itself. Do not start another Cargo process
alongside it. A cold build is included in the timeout and elapsed time; periodic
elapsed/log-size messages show progress without discarding raw evidence.

Use a fresh output directory. Inspect `report.json`, `report.md` and raw logs.
Missing or ignored required tests, zero tests, failures, timeout and interruption
do not count as passing evidence. Report selected or unrun groups explicitly.
Linux socket, identity and filesystem restrictions can prevent fixture execution;
retain the failure and use a capable Linux environment rather than weakening a test.

The default suite performs no model inference or paid provider calls. Passing
contracts do not establish task accuracy, latency, token/cost or RSS improvements.
For live-model requests, follow the existing provider-neutral setup and evidence
requirements in `docs/evaluation.md` and `AGENTS.md`; use `cortexfs-performance`
when making a performance change. Existing authorization remains authoritative.

The retired Inspect/Pi runner is available only in Git history. Do not restore
its scripts or duplicate an agent loop, JSONL parser or scoring engine here.
