不要使用mod.rs
请使用cargo add新增依赖，不要手动编辑文件添加依赖
先去阅读规范（如有）：docs/DESIGN.md（Google Labs visual design system 格式）、docs/architecture.md（工程架构入口）、docs/spec/（规范性 ABI）。
文件 / 模块 / 函数命名约定见 docs/naming-guide.md（`crates/cortexfs/src` 新模块用内核风格单 token 文件名，禁止 `-`/`_` 作模块 stem，禁止 `mod.rs`；函数仍 `snake_case` 且宜短）。
开发触发事件以 Git commit 为唯一边界；不要新增后台监听、轮询或热加载子命令。
文件系统 ABI 只使用当前规范里的短单数顶层目录；不要新增 chan/job/hook/workflow 这类第二套提交或编排入口。
统一提交语义是：写临时文件，同目录原子 rename 成 `*.req.json`，从 outbox 读取结果，向 audit 追加事实。

去重与复用约定：
- 新增或修改 Rust 逻辑前，先查已有 helper、模块和相邻实现；有 `.codegraph/` 时先用 CodeGraph 定位同义实现，再决定是否写新代码。
- 新增函数前必须确认没有同义函数；若只是为改名、换错误包装或迁移位置而新增 helper，优先改调用点复用现有实现。
- 不要复制一段逻辑后只改变量名、错误包装或所在模块；存在等价逻辑时优先复用，确实需要抽取时只抽窄边界 helper。
- 需要继续减少重复代码时，先使用 `.agents/skills/cortexfs-dry-refactor` 恢复现场、跑 jscpd、统计函数数量，再改代码。
- 优先复用已有公共 helper：`plain_fs`（含 `create_plain_dir_with`）、`host_path`、`process_helpers`（`read_limited_bytes`/`read_limited_text`/`terminate_process_group`）、`ControlLineIssue`/`inspect_control_line`（control/index/schema 文本）、`PathLayoutIssue`/`LayoutPathRole`/`require_plain`（layout 路径 kind；共享队列目录用 `require_symlink_dir`）、`jsonl_line`（`for_each_jsonl_line`/`parse_jsonl_line`）、`bin/shared/*`、provider registry/route/secret/model alias 相关路径。
- 领域 report 可用 type alias（如 `AgentControlIssue`/`ToolSchemaIssue` = `ControlLineIssue`，`ObjectLayoutIssue` = `PathLayoutIssue`），不要再复制 EmptyValue/MissingFile/InvalidJson 一类枚举；不要再加 `require_*` 同义薄包装。
- 只有语义、错误类型、用户可见错误文本、超时/终端/进程行为一致时才合并；否则保留重复实现并说明原因。
- 不要为了少量重复引入带大量 flag、闭包或泛型的过度抽象；合并后代码必须更短或更容易审计。

测试约定：
- FUSE 集成测试挂载点固定使用 `tests/mounts/cortexfs`。
- 该目录只作为本地测试挂载点，不要在其中放源码、fixture 或持久化数据。
- provider/model 设计必须保持中立；不要把 Ollama 写成核心默认路径、核心能力或特殊分支。
- 需要调用真实本地模型做 live test 时，当前测试 fixture 使用 Ollama 模型 `smollm2:135m`。
- 除本地轻量模型 fixture 外，用户也会使用自己配置的 provider/本地聚合 API 做 live test；这类测试必须走现有 provider registry、route、secret 状态和统一提交语义，不要把具体供应商写成核心默认路径或特殊分支。
- 默认 live test 不依赖外部云 API；如果 `smollm2:135m` 不存在，先提示用户安装/拉取，不要静默换模型。只有用户明确要求测试自己配置的供应商/聚合 API 时，才使用该已有配置。
