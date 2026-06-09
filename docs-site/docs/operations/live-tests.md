---
title: 实时测试
---

# 实时测试

仓库包含 ignored live tests，用本机轻量模型 fixture 验证 provider adapter、daemon execution plane 和文件式 pipeline。live test 不依赖外部云 API。

当前 fixture：

```text
smollm2:135m
```

运行前确认 Ollama 可达且模型存在：

```bash
ollama list
```

如果没有 `smollm2:135m`，先提示用户安装/拉取；不要静默换模型。需要拉取时使用精确 fixture：

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
