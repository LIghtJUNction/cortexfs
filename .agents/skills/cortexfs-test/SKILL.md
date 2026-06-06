---
name: CortexFS Test
description: This skill should be used when working in the CortexFS repository and the user asks to "开始测试", "测试 FUSE", "跑 CortexFS 测试", "用 Ollama 测试", "测试 smollm2:135m", or mentions the local test mount at tests/mounts/cortexfs.
version: 0.1.0
---

# CortexFS Test

Use this project skill for CortexFS test work inside this repository.

## Ground Rules

- Treat Linux as the only supported runtime target.
- Use `tests/mounts/cortexfs` as the local FUSE integration-test mountpoint.
- Keep the mountpoint empty except for the mounted virtual tree; do not place source files, fixtures, or durable state inside it.
- Treat Ollama as the current local live-test fixture only, not as a privileged CortexFS provider or filesystem ABI special case.
- Use local Ollama for model-backed tests.
- Use `smollm2:135m` as the default Ollama model for lightweight test traffic.
- Do not silently switch to a different model. If `smollm2:135m` is missing, report that the model must be pulled.
- Avoid external cloud APIs in tests unless the user explicitly requests them.

## Standard Test Ladder

Run tests from cheap to expensive:

1. `cargo fmt --all -- --check`
2. `cargo check --workspace --all-targets --all-features`
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
4. `cargo test --workspace --all-targets --all-features`
5. CLI smoke tests such as `cargo run -p cortex-cli -- status`
6. Ollama availability checks for `smollm2:135m`
7. FUSE mount tests under `tests/mounts/cortexfs` after real mounting is implemented

## Ollama Checks

Before model-backed tests:

1. Check whether the Ollama daemon is reachable on `127.0.0.1:11434`.
2. Check whether `smollm2:135m` appears in `ollama list`.
3. If missing, tell the user to run `ollama pull smollm2:135m` or ask before pulling it.
4. Use short prompts and deterministic expectations for smoke tests.

Example minimal prompt:

```text
Reply with exactly: cortexfs-ok
```

## FUSE Mount Checks

Before mounting:

- Confirm `tests/mounts/cortexfs` exists.
- Confirm the directory is not already mounted.
- Confirm the directory has no runtime files that should be preserved.
- Prefer foreground/debug mounting for early integration tests.

After mounting:

- Inspect the top-level tree with `find tests/mounts/cortexfs -maxdepth 2`.
- Read simple virtual files first.
- Test writes only through documented control nodes.
- Unmount before finishing the test session.

## Reporting

Report:

- commands run,
- whether Ollama was reachable,
- whether `smollm2:135m` was available,
- whether a real FUSE mount was attempted,
- any skipped step and the concrete reason.
