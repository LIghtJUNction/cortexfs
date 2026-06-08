---
title: Live Tests
---

# Live Tests

仓库包含 ignored live tests，用本机轻量模型 fixture 验证 provider adapter 和 daemon execution plane。

当前 fixture：

```text
smollm2:135m
```

确认模型存在：

```bash
ollama list
```

如果没有，拉取精确 fixture：

```bash
ollama pull smollm2:135m
```

运行：

```bash
cargo test -p cortex-providers --test ollama_live --locked -- --ignored --nocapture
cargo test -p cortexd --test execution_ollama_live --locked -- --ignored --nocapture
cargo test -p cortexfs --features live-tests --test ollama_file_pipeline_live --locked -- --ignored --nocapture
```

Ollama 只是当前本地 live-test fixture，不是 CortexFS 的特殊核心 provider。
