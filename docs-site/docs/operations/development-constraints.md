---
title: 开发约束
---

# 开发约束

开发必须遵守项目 ABI 和测试约定。

## 规则

- 先读 `docs/DESIGN.md`。
- 不新增 `mod.rs`。
- 新增依赖必须使用 `cargo add`。
- provider/model 设计必须保持中立。
- 开发触发事件以 Git commit 为唯一边界。
- 不新增后台监听、轮询、热加载或 `dev` 子命令。
- 不新增 `chan/`、`job/`、`hook/`、`workflow/` 作为第二套提交或编排 ABI。
- 不允许通过 `mkdir` 在运行态创建未声明 ABI 目录；这类操作必须返回 EROFS。
- 统一提交语义是写临时文件，同目录原子 rename 成 `*.req.json`，再从 outbox 和 audit 读取事实。
- FUSE callback 不做远程 API 调用、长时间模型发现、向量检索、MCP 调用或 tool execution。
- 慢操作进入 daemon/execution plane。
- 挂载树是 ABI；路径、读写语义、权限语义和错误语义必须文档化并测试。

## 测试挂载点

FUSE 集成测试固定使用：

```text
tests/mounts/cortexfs
```

该目录只作为本地测试挂载点，不要放源码、fixture 或持久化数据。

## 验证

```bash
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
```

额外静态检查：

```bash
rg --files --glob '!vendor/**' | rg '(^|/)mod\.rs$'
rg -n "dev|watch|hot.?reload|poll|notify" README.md AGENTS.md crates .agents tests --glob '!vendor/**'
git diff --check
```
