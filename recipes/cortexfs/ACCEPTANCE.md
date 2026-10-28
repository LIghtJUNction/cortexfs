# CortexFS 重建验收合同

本次交付配方，不交付实现。下面的产品检查均未运行。Recipkg 的格式、递归闭包、
临时打包与完整上下文检查只验证交付结构，不能证明独立重建或产品运行成功。

## 执行方法

重建者在新的隔离 Linux 工作区生成实现、测试与合成 fixture，记录实现 commit、
平台、依赖版本、逐项命令、原始输出和结论。首先使用本地合成 provider、channel、
UID/挂载与受保护假 secret；不得对用户主机安装/更新或使用真实账户代替本地验收。
检查状态使用 passed/failed/blocked/not-run，失败和缺失前提必须保留。

下面每行的成功、失败/边界都必须覆盖；具体字段、限制、errno、退出码、默认值和
样例以 `contracts/docs/spec/` 为准。不是只实现表格中的少量 happy path。

## 基线行为矩阵

| ID | 成功输入/操作 | 失败或边界 | 预期结果与合同 |
| --- | --- | --- | --- |
| REQ-001 | 挂载并列出 /ctx | 未知根、另一 UID 私有目录 | 恰好八项稳定根；不存在/拒绝结果与 root-abi、paths、channel-abi 一致。 |
| REQ-002 | lookup/readdir/stat/read/write/xattr、并发与权限收紧 | 只读、受保护非空目录、链接逃逸 | 符合 fuse 合同，无越权、半成品提交或错误动态投影。 |
| REQ-003 | 读取/执行 model、agent、tool、channel 对象 | 非法/超长名称、缺 abi、超限帧、未配置 | triple、元数据、JSONL 和错误按 object/model 等 ABI；无 secret 泄漏。 |
| REQ-004 | 同目录临时请求 rename 提交，读 outbox/audit | 同 ID 同内容/异内容、取消、rename 失败 | 同内容幂等，冲突拒绝；副作用不重复，失败不追加假成功。 |
| REQ-005 | 合成推理、流式文本/usage/reasoning/tool call | context 超限、畸形 schema、provider 失败 | 保留事件与结束原因；model 不执行工具，不静默截断或伪造完成。 |
| REQ-006 | provider/model/route/key slot/alias、合成 OAuth | 缺 key、过期凭据、错误 endpoint | provider-neutral identity、unconfigured 与 host secret 隔离按 model-abi。 |
| REQ-007 | 获准身份访问挂载与工具，生命周期 create/apply/start/stop/remove | 越权、伪造身份、逃逸、policy 撤销 | 有效权限来自真实 host/Linux/policy 交集，消息不扩大权限。 |
| REQ-008 | send/resume、history/index、run/child 完成和重启 | 双写者、重复/冲突 ID、取消、损坏 tail | durable facts 与 context 分离，序号/锁/幂等遵守 session 与 runtime ABI。 |
| REQ-009 | 历史/文件引用、预算和规则生成 context | 无权引用、不可读文件、超限/畸形控制数据 | 可重建工作集有正确归属；引用不产生 authority。 |
| REQ-010 | 获准 fs.read 与 core tools 经 tsh 执行 | host PATH 同名程序、schema 错、无权限 | 仅 CTX_PATH 解析，policy 在执行处生效，统一结果/审计且无副作用。 |
| REQ-011 | ctx-coreutils 中全部命令/参数/环境/默认与格式 | 未知命令、缺参、缺 mount、格式与权限错 | stdout/stderr/退出码一致，诊断不混入 assistant 文本。 |
| REQ-012 | ctxchat/ctxterm/tsh、completion、多行、引用、attach、薄客户端 | 无 TTY/clipboard、断连、未知 session | 正确终端和历史归属；SH-01 至 SH-09 集成通过。 |
| REQ-013 | 获准 peer 使用 broker grant/FD attach PTY | 错 UID、过期/重放 grant、停止 terminal | 事件有序，拒绝未授权 attach，清理资源，遵守 terminal/broker ABI。 |
| REQ-014 | v1 send/resume/status/cancel 与 framing/关联 | 超限/畸形/未终止帧、错误版本/身份/ID | 基线兼容 v1；v2 的持久连接/多 attachment/replay 单独标 target，未交付不能报通过。 |
| REQ-015 | channels.md 列出平台的合成 payload、routing、capability、driver | 未授权 sender、签名错误、缺 capability、超时/重复消息 | 中立 socket/message 契约、隔离与幂等；合成 codec 通过不等于 live 平台通过。 |
| REQ-016 | 合成 SDK/module/MCP agent/tool package 安装/执行 | 错 hash/manifest/ABI、生命周期与 policy 错 | 保持现有有效对象、版本化 wire、能力声明不授予权限，无新根。 |
| REQ-017 | 验证 generation、原子切换和显式更新 | 构建/健康/校验/切换失败，重复更新 | 旧有效代可恢复，配置/secret/用户数据保留，无混合 reference tree。 |
| REQ-018 | Linux 各声明家族/架构预检、双语计划、TTY 精确确认后安装 | 非 Linux、缺 FUSE/systemd/bwrap、无控制终端 | 改动前失败或正确安装挂载，失败回滚；每个平台单独实机验收。 |
| REQ-019 | 合成 completed session 导出有效 ATIF，显式本地评估请求 | 非法/未完成 session、远端错误 | 无后台上传/监听、无假成功；外部账号另验。 |
| REQ-020 | 英中首页/文档/reel、对象切换、导航、视频 fallback | 窄屏、键盘、触屏、reduced-motion、媒体缺失 | 按 DESIGN/CONTENT 可读可操作；autoplay=0 有效；exact-media 与仅配方模式按 ASSETS 分开报告。 |
| REQ-021 | 显式发布或重启后新版本生效 | 未完成/错误更新与单纯文件变化 | 无后台 watcher/polling/热加载入口，无部分应用。 |

## 反馈驱动的新增验收

`IMPROVEMENTS.md` 中每项同时定义需求、fixture、故障点与可观察判据。逐项报告结果，
并连回 `RESEARCH.md` 的原始 issue：

| 改进 | 必须执行的检查 |
| --- | --- |
| IMP-01 | 并发 readdir/stat/read 与原子提交中断，不暴露已提交半成品。 |
| IMP-02 | JSONL/工具/升级故障注入、重启、文件 hash/Git 完整性，损坏时保全原件。 |
| IMP-03 | 普通 Git、嵌套子模块、linked worktree 的授权写入与范围外拒绝。 |
| IMP-04 | 并发与慢消费者下测吞吐/等待/RSS，同时验证独立会话和取消仍可用。 |
| IMP-05 | provider 失败与断线后状态真实，显式恢复不重复追加输入或执行工具。 |
| IMP-06 | 工具乱序、重复、错 session、重启后的响应关联与不确定副作用。 |
| IMP-07 | 重启/切换/缺失挂载时 durable 保留、temp 回收和可操作错误。 |
| IMP-08 | 两实例私有 tmp/socket、容量错误、异常清理和 host 假 secret 隔离。 |
| IMP-09 | 空/C/POSIX/UTF-8 locale 下中文、PTY、非 TTY 和终端恢复。 |
| IMP-10 | private/shared/temp、CLI/socket/channel/MCP 的审批与权限撤销，无隐式放行。 |
| IMP-11 | provider 多候选/调用 ID、MCP structuredContent、固定 strict wire fixture 无静默丢失。 |
| IMP-12 | HTTP endpoint 发送前拒绝、loopback callback、TLS device flow 的时钟/期限/取消。 |

## 设计、性能和独立验收

站点采用固定 DESIGN 的纸色/深墨/薄荷、衬线标题/可读正文/等宽技术内容，不改成
通用 AI dashboard。检查正常、空、加载、错误、成功、禁用状态，以及键盘焦点、
语言回退、窄视口和 reduced-motion。测试视口可选 360、768、1440 CSS px；这是本次
选择的验收样本，不是原站截图宽度。没有浏览器结果就标记未验证。

性能按 PERFORMANCE 先测真实基线，再提出有依据的预算；没有原始命令/结果不能
声称达标。应用 quality 的代码、错误、资源和可读性合同。

独立重建评估应只使用本配方闭包和明示外部前提，在全新工作区执行，不访问原实现、
原测试或调用原程序伪装。构建者自测、独立功能比对、真实服务/发行版验收分别报告。
