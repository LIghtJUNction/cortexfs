---
title: Harness evaluation
description: Run deterministic agent harness contracts and inspect reproducible evidence.
---

# Evaluate the harness

Start with the contracts that make an agent run trustworthy: ordered events,
bounded context, tool authorization, cancellation, durable history and process
boundaries. The evaluation environment runs the existing Rust implementations
and fixtures. It uses Python's standard library and has no model API dependency.

## Run locally

Use a Linux checkout with the repository's Rust toolchain and system build
dependencies installed. See [Development](developing-cortexfs.md) for setup.
Run as a normal user; several filesystem and execution tests exercise ownership.

```sh
# Discover contracts without compiling anything.
python3 evals/harness/run.py --list

# Run all deterministic contract groups serially.
python3 evals/harness/run.py

# Short feedback loop after a protocol or context change.
python3 evals/harness/run.py --suite wire --suite context

# Require cached Cargo dependencies and prohibit Cargo network access.
python3 evals/harness/run.py --offline

# Preserve the complete existing CI test gate and collect the same report.
python3 evals/harness/run.py --workspace
```

Cargo uses `--locked`; the initial dependency download can require network
access. No command above invokes a model or provider. Every Cargo invocation
uses `scripts/serialize-cargo.sh`, and every Rust test binary uses one test
thread. Set `CORTEXFS_CARGO_LOCK` to one shared lock path when using multiple
worktrees. The runner does not install dependencies or change provider settings.

Results go to a new directory below `target/harness-eval/`. Each run writes
`report.json`, a readable `report.md`, and a raw log for each Cargo invocation.
Use `--output /path/to/new-directory` to choose a location. Existing directories
are refused so a later run cannot overwrite earlier evidence. The report records
the Git revision, dirty worktree state, manifest digest, toolchain, exact commands,
test counts and exit status. Keep the entire directory when sharing evidence.

The default timeout is 3,600 seconds **per Cargo invocation**, including compile
time and lock wait. Use `--timeout 6600` for a cold workspace build. A timeout or
interrupt terminates the invocation's process group and retains a failed report.
Durations measure the test/build command; they are not agent inference latency.
While waiting, the runner prints elapsed time and raw-log byte count every 30 seconds.

Exit codes: `0` means all selected contracts have passing evidence; `1` means a
test, timeout, compilation, or required-evidence failure; `2` means invalid setup
or arguments; `130` means interrupted execution. A failed invocation stops later
groups, which remain explicitly `not_run` in the report. Partial selections are
labelled `selected`, never presented as a full harness evaluation.

## What the fixtures prove

The manifest at `evals/harness/suites.json` maps each contract to its canonical
Rust fixture files and required test names. Each required test must be observed
as `ok`. A renamed, missing or ignored required test fails the contract, even if
Cargo exits successfully. An invocation with zero passing tests also fails.

| Group | Exercised behavior | Boundary |
| --- | --- | --- |
| `wire` | Initial/continuation envelope framing, observation identity and size, ordered streamed events and commands | Runtime client and real Unix-stream fixtures |
| `context` | Tiny UTF-8 byte bounds, output reservation, latest observation retention, unchanged raw history | Context crate |
| `authority` | Combined policy/mount/identity checks, symlink rejection, prompt text cannot grant tool authority | Host authorization mechanism |
| `cancellation` | Child authority attenuation; cancellation appends durable facts without erasing history | Owned child lifecycle |
| `recording` | Terminal facts affect only their run; replay/idempotency; tool success and denial become observations | Durable session recorder |
| `ownership` | Both cancellation/completion orderings, concurrent finishers, fault rollback preserves replacements | Receipt-bound session operations |
| `sockets` | Actual socket reads, peer rejection before mutation, idle timeout, cancellation index checks | One-request v1 socket service |
| `aliases` | Exact broker target, legacy compatibility, owner checks and path rejection | FUSE projection methods; no mounted filesystem required |
| `senders` | Telegram/Discord/Slack actor identity; default-deny routes | Channel event adapters |
| `routing` | Rejected users never dispatch; allowed users retain distinct sessions | Host bridge |
| `schedule` | Delegated work selects the installed executor and requires its create authority | Schedule validation |
| `sdk` | Installed Agent and Tool SDK executables complete two declared native tool calls; canonical CLI failures and oversized input | Installer, SDK processes and host tool loop |

Fixtures stay with their Rust modules. Adding a contract means extending those
tests and registering evidence in the manifest, rather than implementing a
second agent loop inside the evaluator. Runner regression tests check failure
reporting, zero-test rejection, missing coverage and process cleanup:

```sh
python3 -m unittest discover -s evals/harness -p 'test_*.py' -v
```

CI uses `--workspace` with the same `--locked --workspace --all-targets
--all-features` gate as before. Format, source-budget, Clippy and documentation
gates remain separate. CI uploads the evidence even when tests fail.

## Evidence limits

Passing these contracts does not measure model reasoning, coding task success,
cost, tokens, TTFT, p95 latency or peak RSS. There is no aggregate "intelligence"
score. The groups have different responsibilities; combining their counts
into a leaderboard would obscure failures.

The focused profile does not establish mounted FUSE behavior, systemd/cgroup
enforcement, full kernel sandbox isolation or persistent concurrent interaction
v2 behavior. V2 type validation alone cannot establish a deployed v2 service.
Some existing workspace tests contain platform-conditional branches; a libtest
`ok` cannot reveal a branch that returned early. Read the linked fixture sources
and raw logs when evaluating that broader evidence. The required contract tests
are selected from concrete assertions at implemented boundaries.

## Optional live evaluation

Run a live evaluation separately on a disposable configured deployment after
the deterministic contracts pass. Use the current [harness replacement
boundaries](harness.md), provider registry, route and secret configuration. Keep
the user-selected provider and model explicit. Do not bypass the mounted agent
with a direct vendor SDK call or treat `debug/echo` as model-quality evidence.

For the local lightweight fixture, explicitly install `smollm2:135m` before
configuring its provider and route. If it is unavailable, stop with that fact;
do not silently substitute another model. A paid configured-provider run needs
the user's explicit authorization. The deterministic runner never triggers it.

Use a dedicated evaluation agent with the necessary tool policy and a disposable
workspace. Submit through the normal client, for example:

```sh
ctx --root /ctx agent send evaluator --session eval-001 \
  "Read input.txt, double its integer, write output.txt, then read output.txt to verify."
ctx --root /ctx agent send evaluator --session eval-001 \
  "Continue from the previous result and explain which tool observations verified it."
```

These examples require an already configured `evaluator` agent and input fixture.
The client uses the canonical Agent socket; file-queue clients retain the normal
same-directory atomic `*.req.json` submission and outbox/audit semantics. Check
the output file independently and inspect the durable session history; a final
answer that merely claims success is insufficient evidence.

Record pinned source/package revisions, provider/model and generation settings,
input fixture hash, task oracle, tool schemas, policy, concurrency, step/token
budgets, timeout, every attempted run (including failures), tool/model counts and
raw events. Retain private traces locally and redact secrets before sharing.
Cancellation and recovery experiments belong on that disposable deployment.
Comparisons with another harness need identical tasks, tools and budgets plus
repeated measurements and reported uncertainty; this environment makes no
comparative performance or quality claim.

## Replacing the legacy benchmark

The old `inspect_benchmark/` Inspect/Pi runners, datasets and Python dependency
lockfile, non-equivalent protocol timing example and generated performance card
were removed. They are recoverable from Git history. The replacement
has no Inspect, cloud SDK or separate scoring dependency. All existing Rust
correctness tests remain, and the [2026-09-05 validation
report](reports/2026-09-05-harness-validation.md) remains historical evidence,
with its original limitations and failed experiments intact.
