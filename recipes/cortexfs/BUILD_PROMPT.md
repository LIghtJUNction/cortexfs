# Recipkg 构建入口与本轮改进

先读 `RECIPE.md`、`ENVIRONMENT.md`、`OBSERVATIONS.md`、`ACCEPTANCE.md`、
`IMPROVEMENTS.md`、`RESEARCH.md`、`PERFORMANCE.md`、`ASSETS.md`，再按下面的
顺序读固定 ABI。目标是在保持基线公开接口的同时，落实用户本轮要求的同类项目反馈。
REQ-001 至 REQ-021 是基线兼容合同；IMP-01 至 IMP-12 是新增改进和对应验收，
不能把它们描述成原版已有保证。新增安全/恢复要求优先于历史指南中的不安全推断，
但不得偷偷更改根目录、v1 字段、退出码或持久格式；发现真正不兼容时列出冲突并提供
明确版本化迁移，不以猜测扩张协议。

## 递归依赖的职责和集成

| 别名 | 用途与使用范围 | 使用规则 | 集成验收 |
| --- | --- | --- | --- |
| quality | 根配方全部实现、CLI/终端/网站、错误和性能质量 | 阅读其完整 BUILD_PROMPT 与 ACCEPTANCE；项目具体 ABI 优先，未知预算不能编造 | 在交付中对应质量条款提供实际设计、错误、资源与测量证据；无结果则标未验证。 |
| agent-shell | 可选的 agent.sh 兼容薄客户端 | 按该配方独立构建，CTX_BIN 指定父产品重建的 ctx；不引入第二套运行时、历史或权限 | 执行 SH-01 至 SH-09，真实 ctx 集成读取同一 session；替身测试不能代替真实集成。 |

agent-shell 自己依赖同一个 quality 叶子；完整闭包没有循环。库、编译器、Linux、
provider/channel 账号和媒体属于外部前提，源码仓库与研究项目属于参考，均非隐藏的
配方依赖。运行时、provider-neutral 协议、通道、SDK、安装和网站作为 CortexFS 的
内部产品组成部分按本包完整合同重建，不要求沿用原 crate graph。

原始实现和原程序不得用于替代重建。根包包含全部 Markdown 合同和参考文本；
只有 exact-media 与真实远程服务验收需要 ENVIRONMENT/ASSETS 列出的外部前提。

# CortexFS 独立重建提示词

## 任务

仅凭本仓库中的提示词与合同资料，独立重建 CortexFS 在基线版本中的可观察行为。
目标不是恢复原作者曾使用的提示词、逐字复刻源代码，或沿用原项目内部结构；目标是
让同一外部输入在规定环境下产生相同的文件系统界面、命令结果、状态变化、协议行为、
权限结果和可见文档体验。

基线源码为 `LIghtJUNction/cortexfs` 的 commit
`84f349c97bc2f6a611109686a02e21de3800042a`，版本标记
`v0.1.21-10-g84f349c9`。重建完整产品范围，不交付只覆盖主流程的 MVP。

## 必须先读的资料

按以下顺序读取。`contracts/docs/spec/` 是稳定 ABI 的详细规范，必须全部阅读；它们
彼此引用时按本包中的相对路径解析。

1. `contracts/docs/spec/README.md`
2. `contracts/docs/spec/root-abi.md`、`paths.md`、`fuse.md`、`object-abi.md`
3. `model-abi.md`、`session-abi.md`、`agent-tool-security.md`、`agent-runtime.md`
4. `tool-policy-abi.md`、`terminal-abi.md`、`terminal-broker.md`、`interaction-abi.md`
5. `channel-abi.md`、`module-abi.md`、`ctx-coreutils.md`、`rolling-upgrades.md`
6. `contracts/docs/architecture.md`、`CTX.md`、`paths.md`、`using-cortexfs.md`、
   `extensions.md`、`channels.md`、`chatui.md`、`agent-sh.md`、`futureagi.md`、
   `getting-started.md`、`packaging.md` 与 `DESIGN.md`
7. `contracts/translations/zh-Hans/` 用来了解现有中文表达；英文 `contracts/docs/`
   是 canonical 内容。中文页不覆盖 ABI 详细规范。
8. `contracts/site/CONTENT.md` 描述公开文档网站的页面与交互；`ASSETS.md` 登记配方外品牌媒体、校验和及缺失时的验收边界。

`contracts/docs/internal-architecture.md`、`naming-guide.md`、`harness.md`、
`aimock-testing.md`、`proofs/` 以及 `analysis/` 仅供了解来源和历史取舍。除下文列出的
外部兼容契约外，不要把其中的 Rust 模块布局、语言、测试工具、性能基线或原工程内部
目录变成实现要求。`analysis/source-docs/` 也只是分析材料，不得要求复用其中的代码
（本包没有附带实现代码）。

## 范围、自由度与证据优先级

- 产品运行于 Linux；FUSE 文件系统、Unix 用户/组权限、Unix socket、PTY 与 systemd
  行为属于对外环境契约。具体语言、库、数据库、算法、进程拆分、部署拓扑和内部目录
  由你选择。
- 必须保持已公开的 `/ctx` 路径、对象文件格式、JSONL/socket ABI、命令参数、对外配置、
  持久状态和兼容行为。详细字段、错误、边界及退出码以列出的规范文件为准。
- provider 与 model 设计必须中立。不得将 Ollama、某一家云厂商、某个聚合服务写成核心
  默认路径或特殊分支。不同服务通过同一 provider registry、route、secret 状态与提交
  语义工作。
- 不得把 provider、workflow、job、hook、MCP registry、skill registry、audit 或 control
  加成新的 `/ctx` 根目录。详细规范明确允许的 `/ctx/channel/` 是唯一相应扩展。
- `/ctx` 可见性和执行权限来自 Linux 身份、挂载、路径、policy、noexec 与主机 secret；
  prompt、skill、channel 消息或模型返回值本身不授予权限。
- 同一请求 ID 与相同内容重试必须遵守规范中的幂等语义；同一 ID 携带不同内容必须失败，
  不能重复执行副作用。
- 证据冲突处理：具体详细 ABI 与基线源码中明确实现的公开契约优先于索引、旧指南和
  营销文案。`analysis/EVIDENCE.md` 标出的未知项须保留为未知，不得自行补全。
- 不调用原版二进制、远端主机、真实 provider 账号或用户私有数据作为实现替代。演示与
  测试使用合成身份、文件、会话和凭据。

## 编号需求与验收

每项需求都必须实现并交付测试。下面的用例是公开验收输入；规范中的其他例子、边界值、
错误和兼容规则也都是要求。成功路径和失败/边界路径都必须实际运行并报告结果。

### REQ-001 — 稳定根 ABI（P0）

挂载后的根为 `/ctx/`，恰好提供八个稳定条目：`status`、`bin/`、`model/`、`agent/`、
`tool/`、`channel/`、`home/`、`shared/`。保持这些名称的定义、可见性、别名和对象类型；
provider/API 变化不得扩展根目录。

- 正常：挂载后列出根，八个条目与 `contracts/docs/spec/root-abi.md` 一致，所有规范
  定义的对象都可发现。
- 边界：访问未知的根子目录返回规范定义的“不存在”结果；普通用户不能看到或改写其他
  UID 的私有 home。
- 详见：`root-abi.md`、`paths.md`、`channel-abi.md`。

### REQ-002 — FUSE 文件语义（P0）

遵守规范中的 lookup、readdir、read/write、stat、权限检查、符号链接、动态投影、删除、
目录移除、扩展属性与并发可见性。文件路径是公共接口；动态内容只需在符合外部观察条件
时存在，不得要求用户了解内部缓存或存储表。

- 正常：对一个已安装对象执行 `ls`、`stat`、读取控制文件及规范定义的 xattr，结果与
  ABI 对应；修改权限后新请求按新权限执行。
- 失败：向只读或无权限路径写入、穿越不允许的链接、移除非空受保护目录时，返回规范
  对应的错误且不留下半成品状态。
- 详见：`fuse.md`、`root-abi.md`、`agent-tool-security.md`。

### REQ-003 — 可执行对象与控制目录（P0）

保持 `name`、可选 `name.sock`、`name.d/` 的对象语义。可执行对象读取得到规范规定的
可检查元数据，不暴露安装二进制或私有实现。模型、agent、tool、channel 的名字、别名、
控制字段、执行输入和 JSONL 输出按各自规范验证。

- 正常：读取合法对象得到元数据；执行合法对象产生规范的 start/结果/done 事件或相应
  JSONL 响应。
- 失败：非法名称、缺少必需 ABI 字段、畸形控制文件、超限帧和未配置对象按规范返回
  对应错误，不把原始 secret 或内部错误栈写入结果。
- 详见：`object-abi.md`、`model-abi.md`、`agent-runtime.md`、`channel-abi.md`。

### REQ-004 — 单一提交语义与审计（P0）

所有模型/工具/控制提交均使用同一公开语义：在目标目录写临时请求文件，在同一目录原子
rename 为 `*.req.json`；提交者从 outbox 读取该请求对应结果，并向 audit 追加事实。
失败、取消、超时、重复提交和进程中断必须遵守规范定义的可恢复状态，不能额外创建
第二套 workflow/job 提交入口。

- 正常：一次工具调用原子发布请求，读取对应结果，追加一条审计事实，客户端得到同一
  request ID 的完成结果。
- 失败：相同 ID、相同输入的重试返回已有/进行中结果而不重复执行；相同 ID 的不同输入
  被拒绝，rename 失败时 outbox 和 audit 不得伪报成功。
- 详见：`object-abi.md`、`tool-policy-abi.md`、`agent-runtime.md` 及本文范围规则。

### REQ-005 — Model 推理与事件流（P0）

Model 是 provider-neutral 推理端点。支持规范中声明的单次执行、socket、能力声明、上下文
归属、请求/响应编码、流式事件、reasoning/tool-call/usage/结束状态与错误行为。模型可
产生 tool-call 事件，但不得直接执行工具。

- 正常：用合成文本请求完成一次推理，文本、usage 和 done 状态按选定 wire dialect 映射；
  模型请求 tool-call 时只产生事件，由 agent 决定下一步。
- 边界：超过 hard context limit、无效 schema、未支持的 provider 专属字段或 provider
  不可用时，返回规范错误，不静默截断或伪造成功。
- 详见：`model-abi.md`、`object-abi.md`、`interaction-abi.md`。

### REQ-006 — Provider、路由、认证与 secret（P0）

配置 provider/model、别名、模型 route、API key、PKCE/device-code 认证及模型元数据缓存，
遵守 `model-abi.md`。配置与 credentials 分离；secret 仅由主机侧安全存储读取，不写入
`/ctx` 可读文件、agent prompt、子进程不必要的环境、audit 或错误文案。route 可选择
直连、HTTP/Unix socket 聚合入口、key slot 与规则优先级。

- 正常：为合成 provider 配置多个模型和 key slot，通过 route 选到指定 endpoint；别名
  解析到稳定 provider/model identity。
- 失败：缺少 key 时对象为 `unconfigured`；错误凭据、过期认证或不可用 endpoint 不泄露
  secret，也不切换到隐式供应商。
- 详见：`model-abi.md`、`root-abi.md`、`rolling-upgrades.md`。

### REQ-007 — Agent 身份与权限（P0）

Agent 的有效权限由 host identity、UID/GID、mode、policy、visible mounts、cwd、环境和
工具路径共同决定。实现规范规定的 agent create/apply/start/stop/remove 生命周期、身份
验证、system/user tier、mounted view 与权限上限。prompt、技能、外部消息与子代理输出不
可扩大权限。

- 正常：具有读权限的 agent 能读取获准挂载中的合成文件并调用获准工具。
- 失败：未授权路径、越权工具、伪造身份、逃逸挂载或无权 child operation 被拒绝且无
  副作用；`chmod`/policy 收紧后新动作立即按规范受限。
- 详见：`agent-tool-security.md`、`agent-runtime.md`、`tool-policy-abi.md`。

### REQ-008 — Agent Runtime、run 与 session（P0）

实现一个由 agent 负责的工具循环与 run 生命周期。持久 messages/events 是追加事实，序号
单调；context 是可重建工作集。按规范处理 session index/current、显式 resume、锁、单写者、
请求去重、流事件、cancel、失败、重启恢复及 agent/parent 结束时 child 生命周期。

- 正常：连续 send 后 session history 按顺序追加；显式 resume 继续指定 session；完成的
  child 结果可从约定路径读取。
- 失败：另一个并发写者争用同一 session 时 fail closed；使用相同 request ID 重发不执行
  第二次；取消后状态和子进程按规范收敛。
- 详见：`session-abi.md`、`agent-runtime.md`、`interaction-abi.md`。

### REQ-009 — Context、引用与记忆边界（P0）

按照 agent runtime 规范，从 durable history 生成可丢弃的 prompt working set；保持压缩、
token/context 预算、system/project/agent rules、skill 元数据、文件引用与 history 引用的
读取和排序规则。引用只提供上下文，不授予文件或 tool authority。未被写入 durable facts
的 context 不得声称重启后仍存在。

- 正常：同一合成历史与预算得到规范的 context 内容和统计；有效 `@path`/history 引用被
  纳入请求。
- 失败：无权限引用、畸形控制数据、超限内容和不可读文件以规范方式报告，不能把内容塞入
  越权 agent 的上下文。
- 详见：`agent-runtime.md`、`session-abi.md`、`agent-tool-security.md`。

### REQ-010 — Tools、`tsh` 与能力执行（P0）

工具以可执行能力端点暴露；`tsh` 仅依 `CTX_PATH` 查找和调用，不能回退 host `PATH`。
实现 core tools、工具 schema/验证、输入/输出限制、policy 决策、共享队列、命令和日志事实，
以及 SDK 执行边界。运行时不得把凭据交给不需要它的工具。

- 正常：允许的 `fs.read` 读取授权挂载路径；结果经统一 outbox/audit 契约返回。
- 失败：host PATH 中存在但 `CTX_PATH` 未授权的同名程序不可运行；无权限或错误 schema
  的请求不产生外部副作用。
- 详见：`tool-policy-abi.md`、`agent-runtime.md`、`ctx-coreutils.md`。

### REQ-011 — `ctx` 命令行与可见输出（P0）

完整实现 `ctx-coreutils.md` 所列公开命令、参数、默认值、输入校验、stdout/stderr 分流、
输出格式、退出码、环境变量和路径解析。覆盖 status、doctor、bootstrap、object/file、
agent/session、provider/auth、storage、terminal、package、schedule 等命令；以该规范为
命令目录来源，不能从此清单省略子命令。

- 正常：`ctx status`、`ctx ls`、`ctx doctor`、`ctx --help` 在有效 `/ctx` 状态下输出
  符合规范的结果。
- 失败：未知命令、缺参数、错误文件格式、无权限与缺少 mount 时返回规范定义的错误流和
 退出码；不得把诊断信息误写为 assistant 内容。
- 详见：`ctx-coreutils.md`、`paths.md`。

### REQ-012 — 人类聊天、`tsh` shell 与 CLI agent helper（P1）

重现 `ctxchat`、`ctxterm`、`tsh` 和 `agent.sh` 的公开交互。人类聊天 UI 与 agent 持久
PTY/工具 shell 分离；多行输入、completion、`:tsh`、`@path`、`@history:N`、raw JSONL、
clipboard、登录委托、终端 attach 和 session 选择按指南工作。`agent.sh` 是可选轻量前端，
所有状态仍由 `ctx` 和稳定 ABI 管理。

- 正常：chat UI 发送消息并显示 assistant 流；attach 后可使用持久 PTY；`agent.sh` 无
  prompt 时进入交互 chat、有 prompt 时走 send。
- 失败：终端断开、session 不存在、无效命令或不可用 clipboard/provider 时显示安全且可
  理解的失败，不创建第二份私有历史。
- 详见：`chatui.md`、`agent-sh.md`、`agent-runtime.md`、`terminal-abi.md`。

### REQ-013 — Terminal 资源和 broker（P0）

实现 durable terminal control、PTY 输出/输入事件、attach 身份、终端授权与生命周期；
通过 broker 的 Unix socket peer credentials 和已授权文件描述符完成客户端连接。终端文件
和 agent session 的关系按 `terminal-abi.md`。

- 正常：授权客户端附加到指定 session terminal，读写 PTY 并看到有序事件。
- 失败：无效/过期 grant、错误 peer UID、重放 descriptor 或被停止的 terminal 不产生
  attach 权限，状态按规范终止或清理。
- 详见：`terminal-abi.md`、`terminal-broker.md`、`agent-runtime.md`。

### REQ-014 — Runtime/frontend interaction protocol（P0）

实现 Unix stream、JSONL framing、版本协商、request/run/session 关联、attachment、replay、
ordering、backpressure、disconnect、权限校验、cancel、approval/input command 和兼容规则。
实现状态以 `interaction-abi.md` 中明确标注为当前支持的版本为准；规范标为 target 的能力
不能宣称已在基线运行验证。

- 基线正常：v1 send/resume/status/cancel 与 request/run/session 对应关系兼容。
- 仅当另行交付 v2 时：一个连接上的多个 attachment 按 ID 隔离，合法 frame 得到关联
  响应并可按序恢复 durable facts；v2 目标不作为基线已部署能力。
- 失败：畸形、超限、错误版本、未授权 peer、重复 ID 内容冲突和断线 frame 被拒绝或按
  规范重放，不串 session。
- 详见：`interaction-abi.md`、`session-abi.md`。

### REQ-015 — Channels 与外部消息适配器（P1）

保持通用 message、event、receipt、effect、command、capability、health、delivery 和
`cortexfs.channel.socket/v1` driver 边界。channel conversation/thread/session 路由、私聊
sender isolation、message ID 幂等、附件/回复/反应、typing、edit/delete/read receipt、重连
和进度表现按文档执行。平台凭据与平台 wire payload 留在 adapter 内。

为 `channels.md` 与 `channel-abi.md` 中列出的平台/driver 保留声明的完整功能范围；不得把
“codec 存在”误写成已验证 live 服务，也不得让某平台类型进入 provider-neutral ABI。

- 正常：用合成 Telegram/Discord 事件映射到确定 session 并发送规范响应；同一事件重投
  不重复执行 agent。
- 失败：未授权 sender、错误签名/无效事件、平台超时或 adapter 缺少 capability 时安全
  拒绝/重试，且不暴露 token。
- 详见：`channel-abi.md`、`interaction-abi.md`、`channels.md`。

### REQ-016 — Public modules、SDK、MCP 与 extension packages（P1）

兼容模块元数据、声明 capabilities、生命周期、稳定 wire protocol、tool/agent SDK
envelope、package manifest、hash 验证和安装边界。MCP 是外部 adapter，可把获准 server
tools 投影为普通 CortexFS tools；不得添加 MCP 根、绕过 tool policy，或把 MCP server
registry 写入稳定 ABI。

- 正常：安装带完整 hash 的合成 agent/tool package 后，普通 ABI 可发现并执行它。
- 失败：manifest、签名/hash、ABI 或生命周期状态错误时拒绝安装，不覆盖已有有效对象。
- 详见：`module-abi.md`、`object-abi.md`、`tool-policy-abi.md`、`extensions.md`。

### REQ-017 — Durable storage 与升级回滚（P0）

实现 reference tree 版本、storage generations、验证、原子切换、旧代保留/清理、boot 与
runtime 协调、重复更新和失败恢复。配置、用户数据、provider secrets 与系统对象在更新后
按规范保留。read-only plan 与 apply 的结果不可混淆。

- 正常：对新 generation 执行完整检查后原子切换，运行时看到完整新版本。
- 失败：校验、安装、health check 或切换失败时仍能恢复旧有效 generation；不出现半套
  reference tree，也不删除用户状态。
- 详见：`rolling-upgrades.md`、`root-abi.md`、`packaging.md`。

### REQ-018 — Linux 安装与发布包（P1）

重现指南规定的 Linux/systemd/FUSE 安装、Arch/Debian/Ubuntu/Fedora/RHEL/openSUSE/SLES
平台判定、native package/tar 包、依赖预检、双语提示、TTY 精确确认、计划展示、权限、
数据保留和失败回滚。安装是否成功必须由实机服务状态和 `/ctx` 行为判定；仅生成包不算
安装验证。

- 正常：受支持发行版上先显示变更计划，经精确确认后安装并挂载。
- 失败：非 Linux、缺少 systemd/FUSE/bwrap 或没有控制终端时，先行退出且不改变主机；
  更新健康检查失败时回滚到之前版本。
- 详见：`getting-started.md`、`packaging.md`、`rolling-upgrades.md`。

### REQ-019 — ATIF 与 Future AGI 评估适配器（P2）

从符合规范的 session trajectory 生成/验证 ATIF 输入，支持一次性导出或显式请求兼容的
Future AGI endpoint；不得后台上传、监视或创建额外 root ABI。凭据与错误内容按 host
secret 规则处理。

- 正常：导出合成 completed session 后得到 schema-valid trajectory。
- 失败：不完整 session、非法字段或远端错误不会被当成成功评估结果。
- 详见：`futureagi.md`、`agent-runtime.md`。

### REQ-020 — 双语文档站点（P1）

重建公开文档门户的已知外部页面：英文和简体中文内容、首页、规范文档导航、安装/使用
入口、品牌视觉系统、可交互的 `/ctx` 对象 curtain、产品演示视频，以及 20 秒产品 reel。
颜色、字体、间距、按钮、卡片、terminal panel 与响应式规则遵循 `contracts/docs/DESIGN.md`；
原图片与视频的配方外来源及缺失边界见 `ASSETS.md`；它们不随 Markdown 包分发。保留键盘操作、清晰的 aria label、视频
controls/fallback、窄屏排版与 reduced-motion 行为。

- 正常：切换 model/agent/tool/session 视图会更新说明；导航可进入规范和安装页；视频
  缺少 WebM 时可播放 MP4，且提供链接 fallback；`?autoplay=0` 关闭 reel 自动播放。
- 边界：中文/英文切换显示对应文案；prefers-reduced-motion 或触屏模式禁用 curtain
  pointer 动画；窄视口仍可导航和读取内容。
- 视觉限制：提取时未运行或截图对比原站。遵循 token 与包内静态信息，不要声称像素级
  验收已通过。
- 详见：`contracts/site/CONTENT.md`、`DESIGN.md` 与 assets。

### REQ-021 — 无监听式热加载（P1）

开发期变更边界是明确发布的新版本。对象更新或 hook 内容改变后，通过显式操作或运行时
重启生效；不创建后台文件 watcher、轮询更新器或热加载子命令。

- 正常：发布新 generation/对象后，新启动的操作看到新版本。
- 失败：无效更新不会被后台进程悄悄拾取或部分应用。
- 详见：`object-abi.md`、`rolling-upgrades.md`。

## 实现交付

交付可运行软件、适合目标 Linux 平台的安装/启动方式、完整用户文档、所选实现的版本锁定、
配置样例与合成数据、按 `REQ-*` 编号的测试矩阵、实际命令与原始结果、失败和未实现项。
提供开发者和使用者可复制的构建/启动命令。真实 provider 凭据不可作为测试前提；provider
路径先用本地合成服务验证，外部账户能力单独标成未运行。

独立评估者须在全新上下文和工作区中只使用本提示词及本包资料。不得访问历史源码、原版
测试实现、开发者的分析记录之外内容，或调用原版程序伪装重建。构建和自测不等于独立
行为对比。没有完成独立评估前，只能报告“按包内规格实现并已自测”，不能报告与原版
一比一通过。

## 已知差异和未确认项

1. 基线实现及详细 ABI 定义 8 个根条目；旧宣传页和旧中文规范索引曾写 7 个并漏掉
   `channel`。重建产品以 8 项 ABI 为准。若重建营销页要求逐字还原，则该页的旧文案
   与真实 ABI 不一致，应在构建说明中注明。
2. `interaction-abi.md` 将 v2 多 attachment 部分标为 target，基线主要使用 v1。不得把
   target 设计写成已运行验证的基线行为；具体状态见 `analysis/EVIDENCE.md`。
3. 这次提取没有启动原版、挂载 FUSE、登录 provider、运行 channel 或构建网站。静态证据
   不能证明动态行为、外部服务兼容、性能和视觉逐像素结果。
4. 安装器/更新器文档给出主机边界和事务规则；未在真实发行版执行安装/回滚。对未记录的
   文案、时序和平台差异保持未知，不能编造原版保证。
