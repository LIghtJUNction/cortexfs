# CortexFS 命名指南

`crates/cortexfs/src` 下文件、模块、函数的持久约定。风格目标：**Linux 内核源代码风格**，
文件短小、全小写、单 token 模块名，避免装饰性分隔符。

贡献者入口：
[AGENTS.md](https://github.com/LIghtJUNction/cortexfs/blob/main/AGENTS.md)、  
[DESIGN.md](DESIGN.md)、[developing-cortexfs.md](developing-cortexfs.md)。

## 1. 文件命名

- **大小写**：仅小写。
- **模块 stem**：优先单 token（如 `rules.rs`、`skills.rs`、`snapshot.rs`、`history.rs`）。
  **不要**新增带 `-` 或 `_` 的 module stem。
- **复合语义**：选择更强的名词，不要用短语。优先 `snapshot`
  而非 `load-snapshot` / `load_snapshot`。
- **扩展名**：`.rs`。
- **不允许 `mod.rs`**：永不新增。目录模块通过同级文件声明（如
  `agent.rs` + `agent/…`）。重命名遗留文件，不要保留显式 `#[path]`。
- **测试**：既有约定下，邻近生产代码的测试可用 `tests` 后缀；新覆盖优先放在
  `tests/unit/**`，避免引入额外路径约定。
- **范围**：生产与 `bin` 源代码位于 `crates/cortexfs/src`。`crates/cortexfs/tests/unit/**`
  下的树可在单独重命名前暂存旧路径习惯。

### 规范模块名称

每个模块只能有一个与文件 stem 对应的标准名称。模块移动或重命名时，必须在同一次变更内更新
所有调用点，不可通过 `pub use new as old` 或同等兼容别名维持第二路径。

不要新增 `#[path]` 或测试 `include!` 绕路。生产代码不允许：

- 全局导入；
- 以调用方/域前缀命名的共享 helper 别名（`use helper as caller_helper`）；
- 仅做重命名或透传的新函数 wrapper。

`OUT_DIR` 生成的 `include!`、消歧义所需的类型别名、`src/tests/**` 与
`object/executor/tests/**` 的冻结父模块 glob 例外，以及现有
`tests/unit/ctx*` 的扁平 harness，是唯一允许例外。该例外仅用于共享固定 fixture、
稳定子进程测试名与共享词法作用域，不得扩展到其他范围。

## 2. 模块标识符

- 文件 stem 为单 token 时，Rust 模块名即该 stem：`pub mod snapshot;` → `snapshot.rs`。
- 多词 Rust 标识符仍使用 `snake_case`（`write_run_snapshot`），这是 Rust 语法，
  不作为磁盘上 `load_snapshot.rs` 的理由。
- 更倾向于用父文件（如 `support.rs`、`prompt.rs`）列出子项，而不是目录内 `mod.rs`。
- 公共根同理：更新调用方到规范路径，而不是通过重导出别名保留旧根模块名。

## 3. 函数命名

- **大小写**：`snake_case`（Rust 约定）。
- **长度**：动宾短句。优先 `write_snapshot`，避免 `write_agent_load_snapshots_for_run`。
- **内核风格动词**：

  | 前缀 | 用途 |
  | --- | --- |
  | `new_` / `alloc_` | 构造或分配 |
  | `destroy_` / `free_` | 拆除 |
  | `get_` / `put_` | 获取或释放引用 |
  | `read_` / `write_` | 输入/输出 |
  | `parse_` / `format_` | 文本形状转换 |
  | `is_` / `has_` | 谓词 |

- **重命名策略**：只在移动符号、完成已开始的重命名、修复双名实现时重命名；
  不为了风格全树同步改名。

## 4. 共享质量模型

在 `support/` 下共享问题/报告形状：

| 域别名 | 共享基础 |
| --- | --- |
| `AgentControlIssue`、`SessionControlIssue`、`SessionIndexIssue`、`ToolSchemaIssue` | `ControlLineIssue`（`support/control.rs`） |
| `ObjectLayoutIssue`、`SessionLayoutIssue`、`SharedQueueLayoutIssue` | `PathLayoutIssue` + `LayoutPathRole`（`support/layout.rs`） |

优先复用 `inspect_control_line` / `inspect_control_lines`、`for_each_jsonl_line` /
`parse_jsonl_line`、`require_plain`（或用于符号链接元数据目录的 `require_symlink_dir`）与
`create_plain_dir_with`，而不是每次都写本地复制。
域内类型别名可保留；不应再并行定义 `EmptyValue` / `MissingFile` / `InvalidJson`。

## 5. 新代码检查清单

1. 新模块文件必须是**单小写 token stem**，且不含 `-` / `_`。
2. 禁止新增 `mod.rs`。
3. 禁止仅改名或透传的二次 helper。
4. 模块移动后所有调用点统一更新为标准路径，不新增兼容别名。
5. 优先复用已有 helper：`support::plain`、`support::path`、`support::process`、`support::control`、`support::layout`、`support::jsonl`、`cli/*` 及 provider/route/secret 路径。
6. 新增控制/布局问题应映射到共享基础（或薄别名）。
7. 名称应贴近内核风格短模块（如 `rules`、`skills`、`snapshot`），不该像框架短语。
8. 禁止新增 `#[path]`、测试 `include!` 变通、兼容模块别名、调用方前缀 shared helper 别名、生产代码全量导入，以及仅做改名/透传 wrapper
   （除了 `OUT_DIR` 生成包含、类型歧义消解、迁移 test 树下冻结的父级 glob、以及历史遗留
   `tests/unit/ctx*` 的扁平 harness）。
