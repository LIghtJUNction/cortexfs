<div align="center">
  <h1>CortexFS</h1>
  <p><strong>Expose an agent runtime as a Linux filesystem mounted at <code>/ctx</code>.</strong></p>
  <p>FUSE ABI · models · agents · tools · channels · durable sessions · terminal workflows</p>

  [![Recipe](https://img.shields.io/badge/Recipkg-recipe%200.1.0-111111?style=flat-square)](recipes/cortexfs/RECIPE.md)
  [![Baseline](https://img.shields.io/badge/product%20baseline-v0.1.21-111111?style=flat-square)](recipes/cortexfs/OBSERVATIONS.md)
  [![License](https://img.shields.io/github/license/LIghtJUNction/cortexfs?style=flat-square)](LICENSE)
  [![Last commit](https://img.shields.io/github/last-commit/LIghtJUNction/cortexfs?style=flat-square)](https://github.com/LIghtJUNction/cortexfs/commits/main)

  [简体中文](README.zh-CN.md) · [Recipe](recipes/cortexfs/RECIPE.md) · [Build task](recipes/cortexfs/BUILD_PROMPT.md) · [Acceptance](recipes/cortexfs/ACCEPTANCE.md)
</div>

> [!IMPORTANT]
> This checkout publishes a reconstruction recipe, ABI contracts, research, and documentation. It currently contains **no buildable CortexFS implementation**. Historical product commands in the docs are reference material, not runnable commands in this repository.

## The idea in one diagram

CortexFS turns agent-runtime operations into normal Linux objects:

```text
application / shell / agent
           │
           │ open · read · write · exec · socket
           ▼
        /ctx/
        ├── status
        ├── bin/
        ├── model/
        ├── agent/
        ├── tool/
        ├── channel/
        ├── home/
        └── shared/
           │
           ▼
 models · tools · sessions · terminals · channels · audit
```

The filesystem is the public interface. A client does not need to know the internal database, process layout, programming language, or provider SDK. It interacts with paths, permissions, request files, sockets, JSONL events, and normal Unix identities.

## What this repository preserves

The reconstruction target is fixed to source commit `84f349c97bc2f6a611109686a02e21de3800042a` (`v0.1.21-10-g84f349c9`). The recipe preserves externally observable behavior, including:

- the exact eight-entry `/ctx` root ABI;
- FUSE lookup, read/write, metadata, permissions, links and concurrency semantics;
- provider-neutral model objects and streaming events;
- agent identity, mounts, policies and lifecycle;
- tool submission, idempotency, result and audit behavior;
- durable sessions, channels, CLI, PTY and terminal broker flows;
- Linux installation, update and rollback contracts;
- English and Simplified Chinese documentation behavior.

Implementation languages and internal architecture may change. Public paths, wire formats, exit codes, persistent formats and permission results may not silently change.

## Start here

Read the reconstruction materials in this order:

1. [`RECIPE.md`](recipes/cortexfs/RECIPE.md) — package identity, dependencies and scope.
2. [`ENVIRONMENT.md`](recipes/cortexfs/ENVIRONMENT.md) — Linux capabilities, external accounts and prerequisites.
3. [`OBSERVATIONS.md`](recipes/cortexfs/OBSERVATIONS.md) — source evidence and known facts.
4. [`BUILD_PROMPT.md`](recipes/cortexfs/BUILD_PROMPT.md) — the complete implementation task and ABI reading order.
5. [`ACCEPTANCE.md`](recipes/cortexfs/ACCEPTANCE.md) — required success, failure and recovery checks.
6. [`PERFORMANCE.md`](recipes/cortexfs/PERFORMANCE.md) — explicit budgets, unknowns and unperformed measurements.

Research-driven additions are separated from baseline behavior:

- [`RESEARCH.md`](recipes/cortexfs/RESEARCH.md) records public feedback from AgentFS, OpenHands, Sandbox Runtime, Open WebUI and CortexFS.
- [`IMPROVEMENTS.md`](recipes/cortexfs/IMPROVEMENTS.md) converts that evidence into 12 new requirements.

Those improvements are acceptance targets for a reconstruction, not claims that this repository has already executed them.

## Build the reconstruction task

Install the Recipkg CLI separately, then run:

```bash
recipkg check recipes/cortexfs
recipkg build recipes/cortexfs --lang en
```

`recipkg build` prints a complete recursive implementation task. It does not invoke a model, compile software, mount FUSE, or prove that the product works.

The local dependency closure contains:

```text
cortexfs
├── cortexfs-agent-shell   optional thin agent.sh-compatible client
└── software-quality       engineering, presentation and performance contract
```

All recipe payloads are UTF-8 Markdown. Media, implementation source, installation binaries, Git metadata and external accounts stay outside the recipe closure.

## Pack an auditable archive

Use a temporary directory and validate the archive after packing:

```bash
set -eu
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

recipkg pack recipes/cortexfs -o "$out/cortexfs.recipe.tar.gz"
recipkg check "$out/cortexfs.recipe.tar.gz"
```

[`ASSETS.md`](recipes/cortexfs/ASSETS.md) records external media identities, hashes and acquisition limits. A recipe-only rebuild may reproduce behavior and layout without reproducing unavailable brand media byte-for-byte.

## Repository map

```text
recipes/
  cortexfs/                primary reconstruction contract
  cortexfs-agent-shell/    optional thin client contract
  software-quality/        shared quality requirements

docs/                      canonical and later product documentation
.agents/skills/             project skills and pinned Recipkg skill submodules
docs/assets/               reference media excluded from recipe payloads
```

The `docs/` tree includes documentation through later upstream commit `7261cb399ba1f8efa387736938ea3d8eec1a0acb`, including the later hosted-CLI direction. Reconstruct the earlier fixed contract from the recipe snapshot; do not silently merge later product behavior into the baseline.

Clone shared skills when they are needed:

```bash
git clone --recurse-submodules https://github.com/LIghtJUNction/cortexfs.git
```

The former `cortexfs-prompts` remote no longer exists. Its role is now served by tracked Markdown recipes. A future generated implementation belongs in a separately managed, pinned `generated/<source-repository>/` submodule; no generated implementation is present today.

## Security model

The core rule is simple: **text does not grant authority**.

```text
prompt / skill / channel message / model output
                    ≠
              Linux permission
```

Effective authority comes from host identity, UID/GID, mode bits, mounted paths, policy, executable visibility, working directory and host-side secrets. Model output may request a tool call, but it must not bypass those controls. Secrets must not appear in `/ctx` readable files, prompts, unnecessary child environments, audit text or raw errors.

## Verification status

The following remain unverified in this recipe-only checkout:

- independent product reconstruction;
- a running FUSE mount and complete path semantics;
- real provider and channel integrations;
- installation, update and rollback on supported distributions;
- browser/documentation visual parity;
- runtime resource use and performance budgets.

A passing `recipkg check` proves only that recipe structure and references are complete. Actual claims require build output, executed tests, failure-path evidence and environment-specific results recorded separately.

## License and attribution

Source attribution, historical notices and MIT terms are preserved. See [LICENSE](LICENSE) and the recipe's [`LICENSE.md`](recipes/cortexfs/LICENSE.md) for the exact scope of reconstruction materials and referenced content.
