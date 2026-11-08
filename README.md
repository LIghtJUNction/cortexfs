# CortexFS

An agent runtime exposed through the Linux filesystem at `/ctx`.

[简体中文](README.zh-CN.md) · [Documentation](docs/README.md) · [Reconstruction recipe](recipes/cortexfs/RECIPE.md)

This repository contains **documentation, ABI contracts, and reconstruction
recipes**. Product source, installers, tests, and the documentation website
implementation have been removed. Product commands in the guides are historical
references.

## Start here

| I want to… | Read |
| --- | --- |
| Understand the interface | [Architecture](docs/architecture.md) |
| Rebuild the product | [Recipe](recipes/cortexfs/RECIPE.md) → [Build task](recipes/cortexfs/BUILD_PROMPT.md) → [Acceptance](recipes/cortexfs/ACCEPTANCE.md) |
| Look up a path or protocol | [ABI specification](docs/spec/README.md) |
| Find a guide | [Documentation index](docs/README.md) |

## Use this checkout

After installing the [Recipkg CLI](https://github.com/recipkg/recipkg) separately,
run these commands from the repository root:

```sh
recipkg check recipes/cortexfs
recipkg build recipes/cortexfs --lang en
```

`check` validates recipe structure and references. `build` prints the recursive
implementation task for a builder to carry out. Product compilation, runtime
tests, and acceptance remain separate work.

The recipe includes two local dependencies: `cortexfs-agent-shell`, an optional
thin client, and `software-quality`, the shared engineering requirements.
Host prerequisites, media, and performance targets are documented in
[Environment](recipes/cortexfs/ENVIRONMENT.md),
[Assets](recipes/cortexfs/ASSETS.md), and
[Performance](recipes/cortexfs/PERFORMANCE.md).

## Two documented versions

| Material | Scope |
| --- | --- |
| [`recipes/cortexfs/`](recipes/cortexfs/RECIPE.md) | Reconstruct source snapshot `84f349c` using its bundled contracts. |
| [`docs/`](docs/README.md) | Product documentation retained through `7261cb3`, including the later hosted Agent CLI design. |

Keep these versions separate when rebuilding. The
[documentation index](docs/README.md#versions-and-authority) records their full
source references. [Research](recipes/cortexfs/RESEARCH.md) and
[improvement requirements](recipes/cortexfs/IMPROVEMENTS.md) identify additional
acceptance targets; they do not record completed product validation.

## Contribute

Read [AGENTS.md](AGENTS.md) before editing. Keep ABI details in the specification,
link to them from guides, and submit changes through a pull request. Shared
skills and their pinned submodules are described in
[Recipkg integration](.agents/RECIPKG.md).

[MIT license](LICENSE) · [Recipe attribution](recipes/cortexfs/LICENSE.md)
