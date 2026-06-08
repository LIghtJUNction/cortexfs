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
- FUSE callbacks must not perform remote API calls or long-running operations.
- Slow work belongs in the daemon/execution plane.

Verification:

```bash
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```
