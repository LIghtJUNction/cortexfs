# CortexFS

[简体中文](README.zh-CN.md)

This repository now contains documentation and ABI references. Product source,
tests, examples, build and installation code, evaluation runner, and site
implementation have been removed from the tracked tree.

## Rebuild prompt

The [`cortexfs-prompts` submodule](docs/cortexfs-prompts/README.md) contains the
reconstruction package. Start with
[`BUILD_PROMPT.md`](docs/cortexfs-prompts/BUILD_PROMPT.md). It was extracted
from the clean local checkout at commit
`84f349c97bc2f6a611109686a02e21de3800042a` (`v0.1.21-10-g84f349c9`) when this
work began. It describes that snapshot and has not passed an independent
rebuild.

## Documentation snapshots

The canonical documents retained in `docs/` were updated upstream through
commit `7261cb399ba1f8efa387736938ea3d8eec1a0acb` (2026-09-30). They describe a
newer hosted Agent CLI direction than the prompt package's source snapshot.
Use the prompt package's own contract snapshot when reconstructing commit
`84f349c`; use [`docs/spec/README.md`](docs/spec/README.md) for the later
documentation and migration status.

The detailed root ABI defines eight stable entries, including
`/ctx/channel/`. The docs-site implementation is no longer in this repository.

## Historical files

Guides such as [`docs/evaluation.md`](docs/evaluation.md) describe their source
snapshot and may reference tools removed from this tree. Any local files under
`dist/` are historical artifacts, not supported builds of this documentation
repository.
