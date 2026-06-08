---
title: Development Constraints
---

# Development Constraints

开发必须遵守项目 ABI 和测试约定。

## Rules

- 先读 `docs/DESIGN.md`。
- 不新增 `mod.rs`。
- 新增依赖必须使用 `cargo add`。
- provider/model 设计必须保持中立。
- 开发触发事件以 Git commit 为唯一边界。
- 不新增后台监听、轮询、热加载或 `dev` 子命令。
- FUSE callback 不做远程 API 调用、长时间模型发现、向量检索、MCP 调用或 tool execution。
- 慢操作进入 daemon/execution plane。

## Verification

```bash
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

Extra static checks:

```bash
rg --files | rg '(^|/)mod\.rs$'
rg -n "dev|watch|hot.?reload|poll|notify" README.md AGENTS.md crates .agents tests --glob '!vendor/**'
git diff --check
```
