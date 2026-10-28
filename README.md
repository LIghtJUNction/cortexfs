# CortexFS

![Software and dependencies become a recipe for rebuilding software](https://raw.githubusercontent.com/recipkg/recipkg/main/assets/recipkg-cover.png)

[简体中文](README.zh-CN.md)

CortexFS exposes Linux agent runtime behavior through the `/ctx` filesystem.
This repository publishes its **Recipkg reconstruction recipe** and ABI references.
It currently contains no buildable product implementation.

Start with [`recipes/cortexfs/RECIPE.md`](recipes/cortexfs/RECIPE.md), then
[`BUILD_PROMPT.md`](recipes/cortexfs/BUILD_PROMPT.md) and
[`ACCEPTANCE.md`](recipes/cortexfs/ACCEPTANCE.md).

## Recipe and feedback

The reconstruction baseline is source commit
`84f349c97bc2f6a611109686a02e21de3800042a` (`v0.1.21-10-g84f349c9`).
The recipe preserves the eight-entry root ABI, FUSE, models, agents, tools,
durable sessions, channels, CLI/PTY, extensions, Linux installation/update
behavior, and the English/Simplified Chinese documentation experience.
Implementation languages may change while these contracts remain compatible.

[`RESEARCH.md`](recipes/cortexfs/RESEARCH.md) records public issue feedback from
AgentFS, OpenHands, Sandbox Runtime, Open WebUI, and CortexFS itself.
[`IMPROVEMENTS.md`](recipes/cortexfs/IMPROVEMENTS.md) turns that evidence into
12 explicit improvement/regression requirements. These are new acceptance
conditions; they are not claims about behavior already exercised in this repository.

## Use the recipe

Install the Recipkg CLI separately, then run:

```sh
recipkg check recipes/cortexfs
recipkg build recipes/cortexfs --lang en
```

`build` prints the complete recursive rebuild task; it does not execute a model
or build the software. The local dependencies are `cortexfs-agent-shell` (the
optional thin client) and `software-quality`. The client also depends on the
same quality recipe. The closure consists only of UTF-8 Markdown.

For an archive, pack into a temporary directory and validate it there. No
persistent archive is checked in. Images/videos remain outside the payload;
[`ASSETS.md`](recipes/cortexfs/ASSETS.md) gives their pinned identities, hashes,
optional acquisition, and the limitations of a recipe-only rebuild.
External host capabilities/accounts are listed in
[`ENVIRONMENT.md`](recipes/cortexfs/ENVIRONMENT.md). Performance budgets and
unperformed measurements are explicit in
[`PERFORMANCE.md`](recipes/cortexfs/PERFORMANCE.md).

## Repository layout

- `recipes/`: primary reconstruction input and complete local dependencies.
- `docs/`: canonical documentation through later upstream commit
  `7261cb399ba1f8efa387736938ea3d8eec1a0acb`, including the later hosted CLI
  migration direction. Use the recipe's own earlier contract snapshot for rebuilding.
- `.agents/skills/`: existing project skills plus eleven pinned canonical
  `recipkg/*` skill submodules; see [integration guidance](.agents/RECIPKG.md).
- `docs/assets/`: reference media, excluded from every recipe payload.

Clone shared skills with `git clone --recurse-submodules`. The old
`cortexfs-prompts` remote no longer exists; its gitlink has been replaced by
tracked Markdown recipes. An existing local copy is preserved and ignored.
Future generated implementation belongs in a separately managed, pinned
`generated/<lowercase-source-repository>/` child submodule; none is generated here.

Product builds, independent reconstruction, FUSE runtime, providers/channels,
distribution installation/rollback, browser visuals, and performance remain
**unverified**. Historical commands are not runnable product commands in this
documentation/recipe checkout. Source attribution and MIT notices are preserved.
