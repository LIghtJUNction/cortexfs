# ctx Coreutils Design

> Historical contract: `ctx` source code has been removed. This document
> preserves the command contract from the earlier `84f349c` source snapshot;
> see [`cortexfs-prompts`](cortexfs-prompts/BUILD_PROMPT.md) for its independent
> reconstruction prompt. The newer hosted CLI migration is documented in
> [`architecture.md`](architecture.md).

`ctx` is specified in [spec/ctx-coreutils.md](spec/ctx-coreutils.md).

`ctx` is CortexFS coreutils: a thin Unix client for `/ctx`. It is not an AI
chat product, daemon, runtime, provider SDK, registry, workflow runner, or
private chat database.
