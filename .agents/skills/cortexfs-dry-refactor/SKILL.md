---
name: cortexfs-dry-refactor
description: 在 CortexFS 仓库中继续去重、减少重复逻辑、合并等价代码、减少代码量或函数数量、恢复上次 DRY 重构现场，或在新增代码前避免写出重复 helper 时使用。
---

# CortexFS DRY Refactor

用这个 skill 恢复 CortexFS 去重现场。目标是减少真实重复逻辑，但保持行为、错误文本、错误类型和 ABI 稳定。只有共享语义已经清楚时才合并。

## 基本规则

- 编辑前先读 `AGENTS.md` 和 `docs/DESIGN.md`。
- 如果仓库根目录存在 `.codegraph/`，定位代码前先用 CodeGraph。
- shell 命令优先加 `rtk`；只有过滤输出影响判断时才用原始命令。
- 不要新增 `mod.rs`。
- 去重工作默认不加依赖；确实需要时必须用 `cargo add`。
- 不要新增 `/ctx` 顶层 ABI 名称、后台循环、轮询、热加载命令或 provider 特殊默认路径。
- 开发触发事件只以 Git commit 为边界。
- 除非用户明确允许，否则保留用户可见错误文本和错误类型。
- 优先少调用 agent、少拆任务、单轮完成可验证的小改动。

## 恢复现场

每次继续去重，先测当前状态：

```bash
rtk git status --short
rtk git diff --shortstat
rtk proxy bash -lc '
set -euo pipefail
files=$(git diff --name-only -- "*.rs")
base=0
work=0
for f in $files; do
  b=$(git show "HEAD:$f" 2>/dev/null | rg -c "^[[:space:]]*(pub(\\([^)]*\\))?[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+[A-Za-z0-9_]+" || true)
  w=$(rg -c "^[[:space:]]*(pub(\\([^)]*\\))?[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+[A-Za-z0-9_]+" "$f" || true)
  b=${b:-0}
  w=${w:-0}
  base=$((base + b))
  work=$((work + w))
done
printf "changed-rs-fn-count HEAD=%s worktree=%s delta=%+d\n" "$base" "$work" "$((work - base))"
'
```

再找剩余 clone：

```bash
rtk proxy jscpd --reporters ai --min-lines 10 --min-tokens 80 crates/cortexfs/src --ignore "**/target/**"
```

## 新写逻辑前

新增 Rust helper 或本地实现前，先查相邻模块和共享 helper 是否已有等价行为。优先检查这些族：

- `support/plain.rs` 和 `cli/directory.rs`：普通目录、文件创建和同步。
- `support/path.rs`：host workspace 路径校验。
- `support/process.rs`：有限读取进程输出、终止进程组。
- `cli/nofollow.rs`：CLI 侧 no-follow 打开文件。
- `cli/text.rs`：有限读取小文本。
- `cli/stderr.rs`、`cli/json.rs`、`cli/uid.rs`、`cli/alias.rs`、`cli/shell.rs`、`cli/terminal.rs`、`cli/procfd.rs`：CLI 共享工具逻辑。
- provider registry、route、secret、model alias 相关模块：provider/model 行为。

如果新代码像这些 helper，优先调用已有 helper，或在正确边界抽一个窄 helper。不要复制实现后只改变量名。

## 选择合并对象

全部满足时才合并：

- 语义、副作用、错误类型、错误文本和可见边界一致。
- helper 名称能保持领域含义，不需要泛化命名。
- 删除的代码多于新增的代码。
- 调用点更简单，或更容易审计。
- 现有测试覆盖行为，或能便宜地补焦点测试。

任一满足时跳过：

- 相似代码的错误类型、用户可见消息、超时行为、provider 语义、终端安全规则或进程 drain 行为不同。
- 需要很多 flag、闭包或泛型才能共享。
- 跨 bin/library 边界只为减少几行。
- 抽象后让原本清晰的本地操作更难审计。

宏可以用于减少重复源码模式并降低函数数量，但优先保持局部；只有多个模块确实共享同一模式时才外提。

## 安全流程

1. 从 `jscpd` 选择一个 clone family。
2. 用 CodeGraph 读两个片段和它们的调用点。
3. 确认行为一致，包括错误和副作用。
4. 在最窄稳定边界复用或抽取最小 helper。
5. 更新 clone family 里所有等价调用点，不只修第一个匹配。
6. 重跑函数数量统计和 `jscpd`。
7. 剩余 clone 如果需要危险抽象或过度泛化，就停止并说明跳过原因。

## 验证

每个有意义的代码改动后至少跑：

```bash
rtk cargo fmt --check
rtk cargo check --locked --workspace --all-targets --all-features
```

代码改动收尾前跑完整 gate：

```bash
rtk cargo fmt --check
rtk cargo check --locked --workspace --all-targets --all-features
rtk cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
rtk cargo test --locked --workspace --all-targets --all-features
rtk git diff --check
```

最终汇报净行数变化、changed-Rust 函数数量 delta、剩余重复率，以及主动跳过的 clone family。
