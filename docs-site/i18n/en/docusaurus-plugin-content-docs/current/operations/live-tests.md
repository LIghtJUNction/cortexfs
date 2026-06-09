---
title: Live Tests
---

# Live Tests

The repository includes ignored live tests using a local lightweight model
fixture to verify provider adapters, the daemon execution plane, and the file
pipeline. Live tests do not depend on external cloud APIs.

Current fixture:

```text
smollm2:135m
```

Before running, confirm that Ollama is reachable and the model exists:

```bash
ollama list
```

If `smollm2:135m` is missing, report that it must be installed or pulled; do
not silently switch models. Pull the exact fixture when needed:

```bash
ollama pull smollm2:135m
```

Run live tests:

```bash
cargo test -p cortex-providers --test ollama_live --locked -- --ignored --nocapture
cargo test -p cortexd --test execution_ollama_live --locked -- --ignored --nocapture
cargo test -p cortexfs --features live-tests --test ollama_file_pipeline_live --locked -- --ignored --nocapture
```

Ollama is only the current live-test fixture, not a core CortexFS provider.
