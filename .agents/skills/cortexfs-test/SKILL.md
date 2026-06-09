---
name: CortexFS Test
description: This skill should be used when working in the CortexFS repository and the user asks to "开始测试", "测试 FUSE", "跑 CortexFS 测试", "用 Ollama 测试", "测试 smollm2:135m", "验证文件 API", "检查 tests/mounts/cortexfs", or mentions live tests, the local test mount at tests/mounts/cortexfs, or CortexFS test conventions.
version: 0.1.0
---

# CortexFS Test

Apply this project skill for CortexFS test, live-test, file API, and FUSE mount
verification work inside this repository.

## Scope

- Treat Linux as the only supported runtime target.
- Read `docs/DESIGN.md` before changing ABI, FUSE behavior, provider/model
  projection, tests, or docs that describe filesystem semantics.
- Treat the mounted tree as an ABI, not a UI. Preserve documented path,
  read/write, permission, and error semantics.

## Ground Rules

- Keep `cortexfs` as the FUSE/VFS projection layer; slow work belongs in
  `cortexd` or the execution plane.
- Preserve the unified submission contract: write a temporary file, atomically
  rename it in the same directory to `*.req.json`, read outbox, and audit facts.
- Do not add `chan/`, `job/`, `hook/`, or `workflow/` as second submission or
  orchestration ABIs.
- Use Git commits as the only development refresh boundary. Do not add
  background watchers, polling, hot reload, or `dev` subcommands.
- Do not add `mod.rs`.
- Add Rust dependencies with `cargo add`; do not manually edit dependency
  entries.
- Use `tests/mounts/cortexfs` as the local FUSE integration-test mountpoint.
- Keep the mountpoint empty except for the mounted virtual tree; do not place
  source files, fixtures, or durable state inside it.
- Keep provider/model design neutral. Do not make Ollama a privileged provider,
  core default path, core capability, or special filesystem ABI branch.
- Avoid external cloud APIs in tests unless explicitly requested.

## Standard Test Ladder

Run tests from cheap to expensive:

1. `cargo fmt --all -- --check`
2. `cargo check --locked --workspace --all-targets --all-features`
3. `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
4. `cargo test --locked --workspace --all-targets --all-features`
5. CLI smoke tests such as `cargo run -p cortex-cli -- status`
6. Ollama availability checks for `smollm2:135m`
7. FUSE mount tests under `tests/mounts/cortexfs`

## Live Model Checks

Use local Ollama only as the current live-test fixture:

1. Check whether the Ollama daemon is reachable on `127.0.0.1:11434`.
2. Check whether `smollm2:135m` appears in `ollama list`.
3. If missing, report that the model must be installed or pulled. Do not
   silently switch to another model.
4. Use short prompts and deterministic expectations for smoke tests.

Example minimal prompt:

```text
Reply with exactly: cortexfs-ok
```

## FUSE Mount Checks

Before mounting:

- Confirm `tests/mounts/cortexfs` exists.
- Confirm the directory does not contain source files, fixtures, or durable
  state.
- Confirm no stale mount is present before reusing the mountpoint.

After mounting:

- Inspect the top-level tree with `find tests/mounts/cortexfs -maxdepth 2`.
- Check `status`, `cap/format`, `provider/list`, `model/list`, and
  `home/$(id -u)/route/openai.chat/*` when present.
- After code changes, rebuild and remount; do not expect a mounted instance to
  hot-update.

## Reporting

Report:

- commands run,
- whether the fixed test mountpoint was used,
- whether Ollama was reachable,
- whether `smollm2:135m` was available,
- whether a real FUSE mount was attempted,
- any skipped step and the concrete reason.
