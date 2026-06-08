---
title: Live Tests
---

# Live Tests

The repository includes ignored live tests using a local lightweight model
fixture.

Current fixture:

```text
smollm2:135m
```

Check it:

```bash
ollama list
```

Pull it if missing:

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
