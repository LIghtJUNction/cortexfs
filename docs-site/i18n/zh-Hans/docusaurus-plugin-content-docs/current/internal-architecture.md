# CortexFS 内部架构

本文件是 [architecture.md](architecture.md) 的**工程结构**配套文档。产品/ABI 规则
仍在前者定义，本文件规定 **Rust 代码如何分层**、哪些边界可依赖、错误与可执行文件应如何组织，
以及如何在不破坏冻结根 ABI 的前提下重构当前单体。

规范 ABI：`[spec/](spec/)`。命名见 [naming-guide.md](naming-guide.md)。
贡献者规则见仓库根目录 `AGENTS.md`。

---

## 1. 我们要解决的问题

当前几乎所有生产逻辑都在一个 crate（`cortexfs`）中，包含：

| 症状 | 成本 |
| --- | --- |
| 一个包内约 `75k LOC` 的库 + `20k LOC` 的可执行部分 | 编译慢；任意改动都要重建全部 |
| 始终链接重型依赖（`fuser`、`arrow`/`parquet`、`reqwest`…） | SDK 与窄工具会拉入 FUSE 文件系统栈 |
| 大量平铺 `pub use` / `use crate::*` | 隐式耦合，难以识别允许边界 |
| 大量域内 `*Error` + 现存 `Result<_, String>` | 恢复路径不一致，`source()` 链条薄弱 |
| God module（`object/`、`agent/`、`runtime/`、`bin/ctx`） | 认知负担高，评审困难 |

产品架构（文件、policy、session）已清楚。**内部架构**是缺失层：进程角色、
crate 角色、模块层级与错误策略。

---

## 2. 进程架构（运行时形态）

CortexFS 是**一组长期进程与短生命周期 Unix 进程**，而非单进程 AI 框架。

```text
┌─────────────┐     FUSE      ┌──────────────────┐
│  user tools │──────────────▶│ cortexfs-mount   │  (generation projection)
│  ctx / tsh  │               └────────┬─────────┘
└──────┬──────┘                        │ reads
       │ JSONL socket                  ▼
       │                    /var/lib/cortexfs/storage/current
       ▼
┌──────────────────┐   spawn / unit   ┌─────────────────────────┐
│ agent runtime    │─────────────────▶│ cortexfs-object-runner  │
│ (per agent)      │   exec model/    │ (model / agent / tool)  │
│                  │   agent          └─────────────────────────┘
└──────────────────┘
       │
       │ 可选 MCP projection（显式适配器，不是根 ABI）
       ▼
┌──────────────────┐
│ ctxmcp           │  maps ordinary tools ↔ MCP stdio
└──────────────────┘
```

| 进程 | 工作 | 不能做 |
| --- | --- | --- |
| `cortexfs-mount` | 项目生成树作为 `/ctx` 投影 | 拥有会话、调用 provider |
| `cortexfs-agent-runtime` | Socket 接受、持久化记录、启动策略 | 成为第二套 ABI 根 |
| `cortexfs-object-runner` | 一次性 model/agent/tool 执行 | 长驻 daemon 状态 |
| `ctx` / `tsh` / `ctxchat` / `ctxterm` | 主机端 UX，复用同一组文件/sockets | 发明平行控制平面 |
| `ctxmcp` | 显式 MCP 适配器 | 创建 `/ctx/mcp` 根类 |

**不变式：** 开发刷新是 **Git 提交或进程重启**。不允许后台 watcher、
poller 或 hot-reload 子命令。

---

## 3. 目标 crate 架构

从“单库万能”转向**薄而单职责 crate**。优先采用**feature flags**，物理拆分次之；
保持同一模块树可降低风险。

### 3.1 目标依赖图

```text
cortexfs-abi          pure types, path grammar, request frames, policy enums
        ▲
cortexfs-support      plain fs, jsonl, layout, path checks (no FUSE, no HTTP)
        ▲
cortexfs-runtime      socket, session record, egress, control handshakes
        ▲
cortexfs-object       install / replace / executor / runner helpers
        ▲
cortexfs-fuse         fuser projection only
        ▲
cortexfs (facade)     re-exports + optional features; bins depend on facade
        ▲
bins: ctx, tsh, mount, runner, agent-runtime, ctxmcp
sdks: tool-sdk, agent-sdk, runtime-client (depend only on narrow crates)
```

### 3.2 Crate 规则

| Crate | 允许依赖 | 禁止 |
| --- | --- | --- |
| `cortexfs-abi` | `serde`、小型纯库 | `fuser`、`nix` 进程、HTTP、parquet |
| `cortexfs-support` | `abi`、`nix` 文件系统工具、`serde_json` | FUSE、provider HTTP、agent launch |
| `cortexfs-runtime` | `abi`、`support` | FUSE mount loop、object install 阶段 |
| `cortexfs-object` | `abi`、`support`、runtime-client、tool-sdk | FUSE server |
| `cortexfs-fuse` | `abi`, `support`, `fuser` | object executor、provider HTTP |
| SDKs | `runtime-client`（+ 最小 abi 类型） | 可避免时不允许直接依赖完整 `cortexfs` |

**MCP 保持适配器二进制**（`cortexfs-mcp` / `ctxmcp`）。它可以调用 support helper，
但不能强制形成新的根 ABI 类。

### 3.3 近期（不动目录）: Cargo features

在 `cortexfs` 上用 feature gate 分离重型栈，直到物理 crate 完成：

```toml
[features]
default = ["fuse", "columnar", "runtime", "object"]
fuse = ["dep:fuser"]
columnar = ["dep:arrow-array", "dep:arrow-schema", "dep:parquet"]
runtime = []
object = []
cli-support = []
```

| Feature | 模块（示例） |
| --- | --- |
| `fuse` | `fuse/**`、mount 投影 |
| `columnar` | `support/columnar.rs` 与调用点 |
| `runtime` | `runtime/**` socket 与 record |
| `object` | `object/**` install + executor |
| `cli-support` | `cli/**` 供各 bin 共享 |

验收要求：`cortexfs-runtime-client` 与 SDK 可在 `default-features = false` 下，仅
依赖必要项，无需链接 `fuser` 或 parquet（当未启用时）。

---

## 4. `crates/cortexfs/src` 的模块分层

层是**有方向**的。模块只能依赖同层或更低层。越界时必须显式设计说明，
不能静默 `use`。

```text
L0  abi, policy          纯语法与 allowlist 类型
L1  support              plain files, jsonl, layout, path, process helpers
L2  authority, context   身份、pack、control inspection
L3  provider, tool       model 注册、tool schema/state（不含 FUSE）
L4  reference, mount     版本树、挂载表
L5  agent, runtime       启动、子代理、socket、持久会话
L6  object               install/swap/residue + executor/runner
L7  fuse                 仅投影
L8  bin/*                进程入口；可使用 L0–L7，不可反向
```

### 4.1 允许/禁止边（硬性）

| From → To | 规则 |
| --- | --- |
| `fuse` → `object::executor` | **禁止**（projection 不得执行工具） |
| `support` → `agent` / `runtime` / `fuse` | **禁止** |
| `abi` → L0 以上任意层 | **禁止** |
| `object::executor` → `fuse` | **禁止** |
| `bin/*` → library 模块 | 允许 |
| library → `bin/*` | **禁止** |
| `runtime` → `object::install` | 避免；优先使用边界回调/trait |
| SDK / mcp → `support::plain` | 优先后续收窄的公开 `pub` façade；当前仅临时允许 `#[doc(hidden)]` |

### 4.2 每模块一任务（执行目标）

| 模块 | 单一职责 | 拆分时机 |
| --- | --- | --- |
| `object/executor` | 仅执行 model/agent/tool 一次 | 已拆分为多个文件；先补齐 `ExecError` 后再扩展 |
| `object/install` + `swap` | 原子对象生命周期 | 保持 residue/replace 为兄弟模块 |
| `runtime/socket` | 接收 + 帧 + peer policy | 分阶段切分 `exec` / `stream` / `spawn` |
| `agent/launch` | systemd/user unit 生命周期 | 拆分 unit / receipt / alias 文件 |
| `support/columnar` | 持久 JSONL 后端存储 | wal / manifest / shard / claim |
| `bin/ctx` | 主机 CLI 解析与 UX | 领域逻辑保留在 library 模块 |

**尺寸策略（与 clippy 对齐）：**

- 新函数：除 `#[expect]` 指明问题外，函数长度 ≤120 行。
- 新增生产文件硬上限 120 行；超过 120 行的现有生产债务要在后续提交中降低。
- `scripts/source-budget.sh` 是 Rust 及生产代码的唯一可执行线；不要将快照指标抄到文档。
- Ratchet 禁止把生产代码移入 `tests`、one-line 化、source-path/include 技巧、或用
  薄包装文件绕过预算。
- 不要在新代码上加 `too_many_lines` 的 `expect`。

### 4.3 导入策略

| 模式 | 政策 |
| --- | --- |
| `use crate::*` | **不新增。** 触碰文件时同步收敛。 |
| `lib.rs` 的 `pub use imports::*` | 冻结，不再扩展；新模块优先显式路径。 |
| `exports.rs` | 仅公开 ABI 面向重导出，不可作为内部杂项收纳桶。 |
| 生产代码中的 glob 导入 | 禁止（workspace lint）；测试仅保留文档化例外。 |

### 4.4 机制与 policy 分离

权限执行分成两层：

| 边界 | 拥有者 | 不应做 |
| --- | --- | --- |
| Mechanism（`authority`） | 主语类、路径查找、Linux 身份与 mode bits、mount 可见性/选项、稳定拒绝映射 | 解析 policy 格式或假设单一 policy 实现 |
| Policy（`policy`） | 使用 `PolicyEvaluator` 做主语/对象/权限决策；通过 `PolicyV0` 解析文本 | 绕过机制检查，或从 prompt/schema/skill/model 输出授予权限 |

`PolicyV0` 是内置 evaluator，不是执行机制本体。替代 evaluator 必须作为已加载、主机拥有的
policy 状态注入。policy 决策不能绕过主语、路径、Linux 或 mount 检查；任何拒绝都保留为拒绝。

该分层同样适用于工具执行外。`requires` 校验、路由后模型鉴权、命名网络出口网关都要走
`PolicyEvaluator`；仅 control file adapter 解析 `PolicyV0`。provider egress 路由在运行时分配目录、
socket、relay 线程或上游 HTTP 进程前，必须产出已验证的不可变计划。

大模块保持相同所有权划分。例如：
`runtime/egress/{plan,secret,target}.rs` 拥有出口决策；
`runtime/egress.rs` 拥有转发生命周期；`runtime/record/schedule/{record,complete,advance}.rs`
持有子信道 receipt 机制之外的状态迁移。

---

## 5. 错误架构

### 5.1 三层

```text
Tier A — 稳定域枚举
  SocketRuntimeError、AgentLaunchError、InstallError 等
  测试要求 Eq/PartialEq；必须实现 Display + std::error::Error。
  优先使用 source() / 嵌套变体，避免 map_err(|_e| UnitVariant)。

Tier B — 进程内类型壳
  ExecError（object runner）、StopError、CLI 简单错误
  用户可见文本稳定；实现 Display + Error。
  用于“主要是字符串化”但仍仅进程内的失败场景。

Tier C — 二进制边界
  Exit code + 一行 stderr。可在主入口对 Tier A/B 统一 .to_string() 或 message() 映射一次。
```

### 5.2 规则

1. **库 / runner 内部**：不新增 `Result<T, String>` 错误。若 `String` 作为成功 payload
   的 `Result<String, E>` 合法可用。
2. 当测试或文档固定用户可见文本时，文本是 ABI；改写措辞需明确产品决策。
3. **IO 映射**：优先使用类似 `with_io("prefix", &error)` 的辅助函数，使文本保持
   `prefix: {error}`，避免样式发散。
4. **不重复定义 Empty/Missing/Invalid 枚举**：复用 `ControlLineIssue` /
   `PathLayoutIssue`（见 AGENTS.md）。
5. `object/executor` 的迁移顺序：
   `path/policy/wire` → `model/inference` → `agent` shell → `tool` + `run`
   （进行中；在 executor 无关重构前先完成这条线）。

### 5.3 Executor 目标状态

```text
object/executor/**  错误类型仅允许 ExecError
executor::run       Result<ExitCode, ExecError>
bin/runner main     map_err 一次后输出 stderr + exit code
From<String> for ExecError  在迁移完成后移除
From<ExecError> for String  仅二进制侧可选便利
```

---

## 6. Object / tool / MCP 放置

产品规则未变：**MCP 不是根类。** 能力以内嵌路径显示为普通工具：
`/ctx/tool/...`，可选 `.d/mcp` 作为定位器控制。

```text
install path:  object manifest + controls (description, schema, cap, policy, mcp?)
runtime path:  ctxmcp reads locator → stdio MCP server → Tool SDK frames
agent path:    与其他工具一致的执行 + policy
```

内部分层：MCP 客户端代码在 **`cortexfs-mcp`**，不放在 `fuse` 或根投影内。
`object/mcp` 仅可用于 locator JSON 的共享校验（仅 install/bootstrap 阶段）。

---

## 7. 数据与持久性架构

产品设计已定义；内部实现必须遵守：

| 关注点 | 机制 | 代码归属 |
| --- | --- | --- |
| 控制面提交 | 写入临时文件 → `rename` 到 `*.req.json` / control file | `support::plain`、权限 helper |
| 会话历史 | 仅追加 JSONL（可选列存） | `runtime/record`、`support/columnar` |
| Generation 切换 | clone → 校验 → 原子 `current` symlink | `reference/storage` |
| 工具/代理安装 | stage + receipt + swap | `object/install`、`swap`、`residue` |
| 取消/停止 | receipt 约束计划 | `agent/stop`，runtime stop traits |

不要引入绕开这些文件的内存“workflow 引擎”。

### 7.1 性能工程边界

性能改动是“先测量再改造”的架构工作，不是放宽 ABI 或安全模型的理由。该小节仅定义验收边界，
不表明任何候选优化已落地。具体流程见 `.agents/skills/cortexfs-performance/SKILL.md`。

| 边界 | 必要规则 |
| --- | --- |
| 证据 | 在等价发布负载对照下测 baseline 噪声，收益需超过 `max(3%, 2 × noise)`，并满足任务声明的 p95 与 RSS 门槛 |
| 安全 / 可移植 | 保持 `unsafe` 禁用；不使用 `target-cpu=native`、全局 `target-feature`、默认 CUDA。可选加速必须有可测的 CPU fallback |
| 缓存真相 | 缓存必须绑定 generation、commit/restart、policy、provider、model、或配置身份；缓存不能授予权限 |
| 语义 | 保留路径、帧、限制、错误、排序、持久性、权限、取消与 fallback 行为 |
| 审查 | 需要不同撰写者与审查者；看原始运行与差异，不只看汇总加速结论 |

Socket/JSONL framing、agent 输出、context 渲染、FUSE 元数据、provider catalog/config cache、
线程池和可选加速器仅是待排查面；先性能分析后才选实现路径，不得把未测量重写写成已实现优化。

---

## 8. 公开 API 表面

`lib.rs` 出于历史原因导出较多。目标是：

| 可见性 | 受众 |
| --- | --- |
| `pub` in `exports` 或已文档模块 | 外部 crate、集成测试 |
| `pub(crate)` | cortexfs 内跨模块 |
| `pub(super)` / 私有 | 模块内部 |
| `#[doc(hidden)] pub` | 给同级 bin 的临时共享（如 mcp）；需记录并收缩 |

**收敛清单（进行中）：**

1. 停止新增 `pub use` 实现模块导出到 `lib.rs`。
2. 给 `cortexfs-mcp` 提供一个窄 facade（`cortexfs::fsutil` 或 support façade），
   而不是继续在随机 plain helper 上加 `#[doc(hidden)]`。
3. SDK 依赖 `runtime-client` + abi 类型，不依赖完整 projection 栈。

---

## 9. 测试架构

| 层 | 位置 | 角色 |
| --- | --- | --- |
| Unit | `src/**/tests*.rs`、`tests/unit/**` | 纯逻辑、解析、policy |
| Executor | `object/executor/tests/**` | 工具循环、进程限制 |
| 集成 | `tests/*.rs`、FUSE 目录 `tests/mounts/cortexfs` | mount + ABI |
| Live | 使用 `smollm2:135m` 的 skills / scripts | 仅在显式要求时走真实模型 |
| MCP e2e | `cortexfs-mcp/tests` | 仅适配器 |

规则：

- 挂载点固定为 `tests/mounts/cortexfs`（该目录不存放 fixture）。
- 触碰到“热”生产文件时优先移出巨型 `#[cfg(test)]` 块。
- 不扩展 AGENTS.md 里的 test-parent glob 冻结例外。

---

## 10. 迁移路线图

按顺序执行。每一阶段都必须保持 `cargo clippy -D warnings` 与相关测试通过。
优先**纵向切片**而非横向重写。

### 阶段 A — 错误一致性（进行中）

- [x] 引入 `object/executor::ExecError`
- [x] 迁移 `args`、`call`、`exec`、`path`、`policy`、`wire`、（部分）`output`/`agent` loop
- [ ] 完成 `model`、`inference`、`agent` shell、`tool`、`executor::run`
- [ ] 为剩余稳定域枚举补齐 `Display` + `Error`，不变更 variant
- [ ] 在无调用者时移除 `From<String> for ExecError`

**退出标准：** `object/executor` 下不再有 `Result<_, String>` 错误（`String` 作为成功 payload
时除外）。

### 阶段 B — 层卫生

- [ ] 禁止新增 `use crate::*`；触碰文件时一并收敛
- [ ] 在 `lib.rs` rustdoc 记录每个顶层模块层级
- [ ] 仅在功能变更已要求编辑时拆分下一个 god file（顺序：`runtime/socket/exec` → `agent/launch` → `support/columnar`）
- [ ] 用单一 façade 替换临时 `#[doc(hidden)]` 导出

**退出标准：** 新代码里 `fuse`/`support` 不出现向上依赖；CI/PR 中由 CodeGraph 或人工复核确认。

### 阶段 C — Feature flags

- [ ] 新增 `fuse` / `columnar` feature 并在 `cfg` 中生效
- [ ] CI 矩阵：完整 features + `no-default-features` + minimal SDK 构建
- [ ] 测量 `cargo build -p cortexfs-runtime-client` / tool-sdk 链路集

**退出标准：** SDK 在无 `fuser` 和无 parquet 下可构建。

### 阶段 D — 物理 crate 拆分

- [ ] 抽取 `cortexfs-abi`（仅类型）
- [ ] 抽取 `cortexfs-support`
- [ ] 把 runtime/object/fuse 移到独立 crate 或保留 feature（若拆分成本过高）
- [ ] Facade crate 保留 bin 所需的版本化路径依赖

**退出标准：** workspace 编译图匹配 §3.1，行为不变。

### 阶段 E — Bin 打包

- [ ] 每个 bin 仅依赖自己需要的 feature/crate
- [ ] `ctx` 保持 UX，领域逻辑留在 library 模块
- [ ] 与 §2 的进程表一致整理 systemd 单元

---

## 11. PR / Review 清单（架构）

评审者应检查：

1. **ABI：** 是否新增根路径或编排入口？（必须是“没有”。）
2. **层：** 模块是否只调用同层或更低层？
3. **错误：** 是否出现新的 `Result<_, String>`？公共错误缺 `Display`/`Error`？
4. **规模：** 是否新增了 `too_many_lines`/`too_many_arguments` 的 expect 而无拆分？
5. **依赖：** 新增重型依赖是否合理，可选依赖是否做了 feature gate？
6. **过程：** 是否引入 watcher / poller / hot reload？（必须为 No。）
7. **复用：** 是否先查了 `support::plain` / `path` / `process` / layout helper？

---

## 12. “完成” 的样子

```text
Product ABI        不变，仍是 files + sockets 的组合
Internal graph     分层 crate/feature；SDK 保持轻量
Errors             类型化；字符串仅用于成功载荷或最终 stderr
Modules            一模块一职责；god file 按自然边界切分
Bins               进程角色与 §2 一致；无仅在 bin 中实现领域逻辑
MCP                仅适配器；tools 保持普通对象
```

该文档是 `ExecError`、crate feature、模块拆分等重构的北极星。优先做只完成
§10 中一个 checkbox 的小 PR，而不是几千行“架构重写”。
