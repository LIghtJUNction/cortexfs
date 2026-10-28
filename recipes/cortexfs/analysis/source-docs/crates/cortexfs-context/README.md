# cortexfs-context

`cortexfs-context` provides bounded, rebuildable context primitives for agent
applications. It keeps durable history separate from the prompt working set,
selects recent messages under a byte budget, and supports optional summaries
without coupling callers to a model provider or HTTP runtime.

```toml
cortexfs-context = "0.1.7"
```

The crate is intentionally runtime-neutral. CortexFS uses it to build the
history portion of an agent prompt while keeping the authoritative JSONL
session files in its own filesystem layer.
