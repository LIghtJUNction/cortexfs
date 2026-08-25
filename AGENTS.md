不要使用mod.rs
请使用cargo add新增依赖，不要手动编辑文件添加依赖
先去阅读规范（如有）：docs/DESIGN.md（Google Labs visual design system 格式）、docs/architecture.md（工程架构入口）、docs/internal-architecture.md（内部层/crate/错误/迁移）、docs/spec/（规范性 ABI）。
文件 / 模块 / 函数命名约定见 docs/naming-guide.md（`crates/cortexfs/src` 新模块用内核风格单 token 文件名，禁止 `-`/`_` 作模块 stem，禁止 `mod.rs`；函数仍 `snake_case` 且宜短）。
模块依赖只允许同层或向下（见 internal-architecture.md §4）；禁止 fuse→executor、support→agent/runtime、library→bin。库内新增错误不要用 `Result<_, String>`；进程本地可用 `ExecError` 一类 shell，稳定领域失败用 enum + Display + Error。
开发触发事件以 Git commit 为唯一边界；不要新增后台监听、轮询或热加载子命令。
Git 分支约定：默认和当前工作均留在 `main`；除非用户明确要求，否则不得创建或切换分支；若用户明确要求使用其他分支，任务结束前须切回 `main`，除非用户另有说明。
文件系统 ABI 只使用当前规范里的短单数顶层目录；不要新增 chan/job/hook/workflow 这类第二套提交或编排入口。
统一提交语义是：写临时文件，同目录原子 rename 成 `*.req.json`，从 outbox 读取结果，向 audit 追加事实。

文档单一真相：`docs/` 是 canonical 源；`docs-site/i18n/en/` 只保留与 canonical 内容不同的真实翻译。禁止复制逐字相同的英文占位文件，缺失条目由 Docusaurus locale fallback 提供。
Rust 规模统一由 `scripts/source-budget.sh` 门控：新增/变更需遵守 120 行上限、测试底线与 all/prod 预算。
性能改动必须使用 `.agents/skills/cortexfs-performance`：先建立可复现 release 基线与噪声，收益须超过 `max(3%, 2×noise)` 且满足任务的 p95/RSS 门槛；禁止 `unsafe`、`target-cpu=native`、全局 `target-feature` 和默认 CUDA，任何可选加速都必须保留等价 CPU fallback 与缓存一致性，并由独立 reviewer 审核原始数据和 diff。

去重与复用约定：

- 新增或修改 Rust 逻辑前，先查已有 helper、模块和相邻实现；有 `.codegraph/` 时先用 CodeGraph 定位同义实现，再决定是否写新代码。
- 新增函数前必须确认没有同义函数；若只是为改名、换错误包装或迁移位置而新增 helper，优先改调用点复用现有实现。
- 不要复制一段逻辑后只改变量名、错误包装或所在模块；存在等价逻辑时优先复用，确实需要抽取时只抽窄边界 helper。
- 需要继续减少重复代码时，先使用 `.agents/skills/cortexfs-dry-refactor` 恢复现场、跑 jscpd、统计函数数量，再改代码。
- 优先复用已有公共 helper：`support::plain`（`support/plain.rs`，含 `create_plain_dir_with`）、`support::path`（`support/path.rs`）、`support::process`（`support/process.rs`，含 `read_limited_bytes`/`read_limited_text`/`terminate_process_group`）、`ControlLineIssue`/`inspect_control_line`（`support/control.rs`）、`PathLayoutIssue`/`LayoutPathRole`/`require_plain`（`support/layout.rs`；共享队列目录用 `require_symlink_dir`）、`support::jsonl`（`support/jsonl.rs`，含 `for_each_jsonl_line`/`parse_jsonl_line`）、`cli/*`、provider registry/route/secret/model alias 相关路径。
- 领域 report 可用 type alias（如 `AgentControlIssue`/`ToolSchemaIssue` = `ControlLineIssue`，`ObjectLayoutIssue` = `PathLayoutIssue`），不要再复制 EmptyValue/MissingFile/InvalidJson 一类枚举；不要再加 `require_*` 同义薄包装。
- 只有语义、错误类型、用户可见错误文本、超时/终端/进程行为一致时才合并；否则保留重复实现并说明原因。
- 不要为了少量重复引入带大量 flag、闭包或泛型的过度抽象；合并后代码必须更短或更容易审计。
- 禁止新增 `#[path]`、测试 `include!` 模块绕行、兼容模块别名、调用方/领域前缀的共享 helper `use ... as ...`、生产代码 glob import，以及只改名或透传的薄 wrapper。仅允许构建脚本生成的 `OUT_DIR` `include!`、解决 trait/type 名称冲突所必需的别名、为保持旧 flat-suite 共享夹具而冻结在 `src/tests/**` 与 `object/executor/tests/**` 内的测试父模块 glob，以及尚待独立重构且必须保持测试全名和共享词法作用域的既有 `tests/unit/ctx*` flat harness；不得扩大这些例外。

测试约定：

- 构建与测试必须串行：使用 `scripts/serialize-cargo.sh` 或 `scripts/test.sh`；`.cargo/config.toml` 已通过共享 `flock` 锁串行化 `rustc`。禁止同时启动多个 Cargo、Clippy 或测试命令，先等待当前命令结束。
- FUSE 集成测试挂载点固定使用 `tests/mounts/cortexfs`。
- 该目录只作为本地测试挂载点，不要在其中放源码、fixture 或持久化数据。
- provider/model 设计必须保持中立；不要把 Ollama 写成核心默认路径、核心能力或特殊分支。
- 需要调用真实本地模型做 live test 时，当前测试 fixture 使用 Ollama 模型 `smollm2:135m`。
- 除本地轻量模型 fixture 外，用户也会使用自己配置的 provider/本地聚合 API 做 live test；这类测试必须走现有 provider registry、route、secret 状态和统一提交语义，不要把具体供应商写成核心默认路径或特殊分支。
- 默认 live test 不依赖外部云 API；如果 `smollm2:135m` 不存在，先提示用户安装/拉取，不要静默换模型。只有用户明确要求测试自己配置的供应商/聚合 API 时，才使用该已有配置。
