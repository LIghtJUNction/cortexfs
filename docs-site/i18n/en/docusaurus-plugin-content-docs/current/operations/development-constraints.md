---
title: Development Constraints
---

# Development Constraints

Project rules:

- Read `docs/DESIGN.md` first.
- Do not add `mod.rs`.
- Add Rust dependencies with `cargo add`.
- Keep provider/model design neutral.
- Use Git commits as the only development refresh boundary.
- Do not add background watchers, polling, hot reload, or `dev` subcommands.
- Do not add `chan/`, `job/`, `hook/`, or `workflow/` as second submission or
  orchestration ABIs.
- Do not allow `mkdir` to create undeclared ABI directories at runtime; those
  operations must return EROFS.
- Use one submission contract: write a temporary file, atomically rename it in
  the same directory to `*.req.json`, then read facts from outbox and audit.
- FUSE callbacks must not perform remote API calls, model discovery, vector
  search, MCP calls, or tool execution.
- Slow work belongs in the daemon/execution plane.
- The mounted tree is an ABI; paths, read/write semantics, permission
  semantics, and error semantics must be documented and tested.

## Test Mountpoint

FUSE integration tests use the fixed local mountpoint:

```text
tests/mounts/cortexfs
```

Use that directory only as a local test mountpoint. Do not put source files,
fixtures, or persistent data there.

Verification:

```bash
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

Additional static checks:

```bash
rg --files --glob '!vendor/**' | rg '(^|/)mod\.rs$'
rg -n "dev|watch|hot.?reload|poll|notify" README.md AGENTS.md crates .agents tests --glob '!vendor/**'
git diff --check
```
