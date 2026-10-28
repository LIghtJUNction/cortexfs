# CortexFS 提取证据

## 基线

- 分析时间：2026-10-28，Asia/Taipei。
- 对象：`LIghtJUNction/cortexfs`，本地 Git checkout。
- 源提交：`84f349c97bc2f6a611109686a02e21de3800042a`，版本标记
  `v0.1.21-10-g84f349c9`；工作区在分析前干净，`main` 与 `origin/main` 无分歧。
- 子模块 `agent.sh`：`2226376cae430b96a6298b7a786d38be689a1bfb`，工作区干净。
- 输入类型：多 crate Rust 源码、shell/Python 工具、Docusaurus TypeScript/React 页面、
  用户文档、规范、静态资源及 `agent.sh` 子模块。
- 分析机：Linux `7.2.11-arch1-1`，Omarchy 4.0.6，x86_64 GNU/Linux。
- 当前任务未构建、启动或挂载原版；未调用 provider、channel、systemd 安装器或 updater；
  未运行 cargo、浏览器站点或任何测试。环境信息仅描述分析机，不能证明目标软件运行。
- 凭据与用户数据：未收集或复制。提示词包只有文档、图像/视频资源和分析说明，不含
  CortexFS 实现源码。

## 发现方法与范围

从规范目录、README、命令入口、FUSE/mount/runtime/object-runner 入口、公开 SDK crate、
channel host、安装脚本、systemd unit 与文档网站入口建立清单。按可观察边界追踪主要调用；
未逐行复核所有 1,108 个 Rust 文件，也未执行覆盖率探测，因此不能声称发现了所有源码
分支。公共 crate README 只作为分析证据保留在 `source-docs/`。

提示词覆盖产品界面、`ctx` 命令与交互、model/provider、agent/runtime/session、tool/policy、
terminal、channels、SDK/modules、安装更新、Future AGI 适配器和文档站点。CI、性能 benchmark
与仓库开发助手属于工程工具，未要求另一个构建者重造。

## 证据目录

| 证据 | 类型 | 定位 | 支持的结论 | 限制 |
| --- | --- | --- | --- | --- |
| E-001 | 详细规范 | `contracts/docs/spec/root-abi.md`、`channel-abi.md` | 根 ABI 与 channel 是外部契约 | 文档静态证据 |
| E-002 | 实现静态证据 | `crates/cortexfs-paths/src/lib.rs::ROOT_ENTRIES` | 实际常量含 `status, bin, model, agent, tool, channel, home, shared` 八项 | 未挂载 `/ctx` 运行验证 |
| E-003 | 实现静态证据 | `crates/cortexfs/src/bin/ctx/main.rs::main` 与 `run` | `ctx` 命令入口将 OS 参数分派并定义错误输出边界 | 未运行 help/子命令 |
| E-004 | 实现静态证据 | `crates/cortexfs/src/bin/mount/main.rs::main`、`crates/cortexfs/src/mount/` | 独立挂载入口转入 FUSE mount 行为 | 未 mount |
| E-005 | 实现静态证据 | `crates/cortexfs/src/bin/runner/main.rs::main`、`crates/cortexfs/src/object/executor.rs` | executable object 由统一 runner/executor 路径承接 | 未执行 object |
| E-006 | 实现静态证据 | `crates/cortexfs/src/bin/runtime/main.rs::serve`、`crates/cortexfs/src/agent/` | agent socket/runtime、identity 和系统服务生命周期由 host 管理 | 未启动 systemd unit |
| E-007 | 实现静态证据 | `crates/cortexfs/src/authority/`、`agent/launch/`、`fuse/projection/` | authority、sandbox、mount view 与 FUSE projection 参与可见性和执行判断 | 未做攻击/越权运行探测 |
| E-008 | 实现静态证据 | `crates/cortexfs-protocol/src/` | provider wire formats 通过协议适配和规范化边界处理 | 未连真实 provider |
| E-009 | 实现静态证据 | `crates/cortexfs-runtime-client/src/interaction/`；`interaction-abi.md` | runtime/client interaction 有版本化 frame；规范注明 v2 与当前实现状态差异 | 未做协议对比 |
| E-010 | 实现静态证据 | `crates/cortexfs-channels/src/`、`crates/cortexfs-channel-*/src/` | 通道模型、registry、driver 和各平台 adapter 分层存在 | codec/源码存在不等于 live 服务通过 |
| E-011 | 实现静态证据 | `crates/cortexfs-agent-sdk/`、`cortexfs-tool-sdk/`、`cortexfs-module/`、`cortexfs-mcp/` | 有 SDK、module、MCP adapter 边界 | 未运行第三方扩展 |
| E-012 | 文档+源码静态证据 | `docs/spec/rolling-upgrades.md`、`scripts/update-linux.sh`、`packaging/` | generation/update/回滚与包安装是产品运维契约 | 未在发行版执行 |
| E-013 | 文档+源码静态证据 | `docs/agent-sh.md`、`agent.sh/README.md`、子模块 commit | `agent.sh` 是薄客户端；session/history 属于 `/ctx` ABI | 未运行该脚本 |
| E-014 | 文档+源码静态证据 | `docs/DESIGN.md`、`docs-site/src/pages/index.tsx`、`CortexCurtain.tsx`、`reel.tsx`、CSS | 文档网站存在交互对象 curtain、首页、reel、双语内容和对应视觉 token | 未启动或截图；像素级布局未知 |
| E-015 | 资源 | `contracts/site/assets/`，资源 SHA 在 `SHA256SUMS` | 标识、海报和演示视频可随提示词包分发 | 浏览器解码/许可以当前仓库 MIT 与来源为背景；未播放验证 |
| E-016 | 用户约定 | 仓库 `AGENTS.md` 与 `docs/spec/` | provider-neutral、原子 request/outbox/audit 和无后台轮询是约束 | 已删除的开发实现规范未作为语言/模块要求 |

## 功能覆盖矩阵

`REQ-*` 定义于 `BUILD_PROMPT.md`；“静态覆盖”只表示文件和代码路径被检查，不表示运行。

| 功能 | REQ | 规范/指南 | 代码入口证据 | 静态规格 | 已运行 |
| --- | --- | --- | --- | --- | --- |
| Root 与 FUSE | 001–002 | root, paths, fuse | `cortexfs-paths/src/lib.rs`; `cortexfs/src/fuse/` | 是 | 否 |
| executable objects 与请求提交 | 003–004 | object, tool-policy | `cortexfs/src/object/`; `support/atomic` callers | 是 | 否 |
| model、协议、secret、route | 005–006 | model, root | `cortexfs-protocol/src/`; `cortexfs/src/model/` | 是 | 否 |
| identity、policy、sandbox | 007 | agent-tool-security, agent-runtime | `cortexfs/src/authority/`, `agent/launch/` | 是 | 否 |
| run、session、context、child | 008–009 | session, agent-runtime | `cortexfs/src/agent/`, `runtime/`, `context/` | 是 | 否 |
| tools、tsh、CLI | 010–011 | tool-policy, ctx-coreutils | ctx bin, `cortexfs-tools/` | 是 | 否 |
| Chat UI、PTY、terminal broker | 012–013 | chatui, agent-runtime, terminal | ctxchat/ctxterm bins, broker bin | 是 | 否 |
| interaction protocol | 014 | interaction, session | `runtime-client/src/interaction/` | 是 | 否 |
| channels | 015 | channel-abi, channels.md | `cortexfs-channels/`, `cortexfs-channel-*/` | 是 | 否 |
| SDK/modules/MCP | 016 | module, object, tool-policy, extensions | SDK/module/MCP crates | 是 | 否 |
| storage update/package/installer | 017–018 | rolling-upgrades, packaging | `scripts/install*.sh`, `update-linux.sh`, `packaging/` | 是 | 否 |
| Future AGI | 019 | futureagi | `cortexfs-futureagi/` | 是 | 否 |
| public docs website | 020 | DESIGN, getting-started, docs-site sources | docs-site pages/components/CSS | 是，源码静态 | 否 |
| agent.sh | 012 | agent-sh | submodule README and pinned commit | 是，README/代码路径 | 否 |
| Git hot reload boundary | 021 | object-abi, user AGENTS | hook/object behavior | 是 | 否 |

## 冲突、未知与影响

| ID | 问题 | 证据 | 处理 | 影响 |
| --- | --- | --- | --- | --- |
| UNKNOWN-001 | 详细 root ABI 与实际常量包含 `channel`，旧 spec index、中文翻译和 landing/reel 写七个根 | E-001, E-002, E-014 | 产品按八项实现；旧网页文案差异单独显式记录，绝不用于产品路径 | 若需要逐字重建网站，旧文案与产品 ABI 有意不一致 |
| UNKNOWN-002 | interaction v2 多 attachment 标为 target，基线主要生产路径仍是 v1 | `interaction-abi.md` implementation status；runtime-client source | 保持 v1 基线兼容，不把 target 功能宣称为运行验证 | v2 完整投产面待独立版本确认 |
| UNKNOWN-003 | provider API、模型目录、认证流程随外部服务变化 | `model-abi.md` | 固定外部条件，用本地合成 endpoint；真实供应商列作未验证 | 不承诺当前实时服务状态 |
| UNKNOWN-004 | channel codecs/driver 文件不证明平台账户、WebSocket、webhook、媒体上传或 rate-limit 的 live 兼容 | `channels.md`、channel crates | 覆盖文档接口；逐平台 live 验证留给独立评估 | 无真实平台结果 |
| UNKNOWN-005 | Linux 发行版依赖、FUSE capability、systemd 和回滚实机结果未收集 | `getting-started.md`、`packaging.md`、installer source | 依文档定义接受条件，未知平台不得报告通过 | 安装器重建需发行版实机验收 |
| UNKNOWN-006 | 文档站点没有浏览器截图、视口/DPI/字体或动画录屏 | `docs-site/src/`, `DESIGN.md` | 使用设计 token、静态页面结构及资产；标为未达像素级确认 | 精细布局误差仍未知 |
| UNKNOWN-007 | 未构建或运行源码，因此内部源可见分支、错误文本和并发时序未全部转成观察事实 | 本次分析范围 | 只把规范明确契约写成要求，其他源码推断留在分析证据 | 提示词完整度不能等同原版全分支实测 |
| UNKNOWN-008 | 安装文档/README 是 v0.1.21 快照，当前 docs-only 分支不再可构建/安装 | baseline 与本仓库转换 | 按快照提取；当前状态在 README 和 docs index 明示 | 不能把旧安装命令当成当前支持服务 |

## 文档包变更记录

`contracts/docs/spec/` 中的详细规范来自基线。为解决导航遗漏，提示词包的
`contracts/docs/spec/README.md` 汇总了详细规范文件并采用八项 root 列表；它是本次提取的
导航修订，不是原 commit 的逐字拷贝。公开 crate/example README 仅存于
`analysis/source-docs/`。没有把核心实现、测试实现、二进制、原版 wrapper 或私密数据放入
构建提示词。

状态：**已生成，尚未通过独立重建验证。**
