# 同类项目反馈与取舍

检索日期：2026-10-29，Asia/Taipei。通过 GitHub 官方仓库、issue 正文、可获得的
评论和 GitHub REST API 核对状态；没有安装或复现这些项目。下列内容是有限样本，
不是完整 issue 审计或跨项目性能比较。问题根因若仅来自报告者推断或自动分诊，
明确保留该限制。不得把其他项目的报告写成 CortexFS 的已验证缺陷。

## 项目选择

| 项目 | 检索时 star 数 | 与本产品的关系 |
| --- | ---: | --- |
| [AgentFS](https://github.com/tursodatabase/agentfs) | 3,494 | Agent 文件系统、FUSE 与隔离工作区，技术邻近度最高。 |
| [OpenHands](https://github.com/OpenHands/OpenHands) | 92,109 | Agent 运行、会话、终端、工具结果和故障恢复，高使用量案例。 |
| [Sandbox Runtime](https://github.com/anthropics/sandbox-runtime) | 6,816 | 本机文件/网络隔离和 Linux sandbox 体验。 |
| [Open WebUI](https://github.com/open-webui/open-webui) | 157,063 | 多用户工具权限与审批交互，界面不作为 CortexFS 视觉模板。 |

star 是当日 `stargazers_count` 快照，仅用于样本选择，不证明设计、安全或反馈代表性。
本项目自身的 issue 检索与结果另在本文件末尾记录；没有拿 PR 数当 issue 数。

## 逐项来源与采用决定

| 来源 | 报告日期；核对时状态 | 实际反馈与证据级别 | 对本配方的决定 |
| --- | --- | --- | --- |
| [AgentFS #335](https://github.com/tursodatabase/agentfs/issues/335) | 2026-05-13；open | 报告者指出新文件 dentry、link count 与数据分步写入，并发读取可能看到不一致元数据；未见维护者确认。 | IMP-01：验证原子提交路径的可见性和元数据一致性，不强制相同数据库或事务实现。 |
| [AgentFS #332](https://github.com/tursodatabase/agentfs/issues/332) | 2026-04-11；open | 多 Agent 使用后报告数据库/Git 对象和文件内容损坏，附日志；根因未确认。 | IMP-02：故障注入、durable tail 校验、Git/文件完整性；不得断言并发本身就是根因。 |
| [AgentFS #344](https://github.com/tursodatabase/agentfs/issues/344) | 2026-09-14；open | macOS overlay 可写但 host 子模块/gitdir 被拒，overlay commit 与 host HEAD 不一致；报告者提供路径与操作。 | IMP-03：Linux 上验收获准挂载内的子模块/worktree，明确写入目标和权限；不要求取消隔离或照搬 macOS 问题。 |
| [AgentFS #228](https://github.com/tursodatabase/agentfs/issues/228) | 2026-01-11；open | 用户比较串行/多核负载；collaborator 评论确认当时同步等待限制并发。 | IMP-04：分开测元数据、并发吞吐、排队和锁；他人的倍率不成为本项目阈值。 |
| [OpenHands #18980](https://github.com/OpenHands/OpenHands/issues/18980) | 2026-10-26；open | 中途错误后无恢复入口，重新发送重复消息；正文含报告者记录，也含明确标为 AI 的自动分诊。 | IMP-05：失败状态可解释、兼容恢复入口、不靠重复用户消息恢复；不采信自动分诊为独立复现。 |
| [OpenHands #18941](https://github.com/OpenHands/OpenHands/issues/18941) | 2026-10-25；open | 用户记录运行环境重建前后工具调用/结果错配，原因仍属推断；正文含 AI 分诊。 | IMP-06：跨重启的调用关联和不确定副作用处理，不把猜测当确认根因。 |
| [OpenHands #18960](https://github.com/OpenHands/OpenHands/issues/18960) | 2026-10-26；open | 报告容器重启后 workspace 不保留，附部署条件；不意味着所有部署均如此。 | IMP-07：明确 durable/临时数据边界并实际重启验收，不要求本产品采用 Docker。 |
| [Sandbox Runtime #294](https://github.com/anthropics/sandbox-runtime/issues/294) | 2026-06-03；open | 用户希望可写隔离 tmp，避免为临时文件放开 host /tmp；提供命令和错误。 | IMP-08：可用的私有临时目录与清理、主机数据隔离并存。 |
| [OpenHands #18982](https://github.com/OpenHands/OpenHands/issues/18982) | 2026-10-26；open | 未继承 UTF-8 locale 时终端初始化失败；后续评论给出候选 PR，仍不等于合并发布。 | IMP-09：缺少/错误 locale 的终端和中文验收，不强制 tmux 或全局修改 locale。 |
| [Open WebUI #29249](https://github.com/open-webui/open-webui/issues/29249) | 2026-08-30；closed/completed，2026-10-10 关闭 | 报告临时聊天仍显示需审批却直接运行工具；标签 confirmed issue，member 讨论其状态前提。 | IMP-10：审批状态不可维持时拒绝或不提供能力，不能退化为执行。历史问题，不声称最新版仍有缺陷。 |
| [Open WebUI #13535](https://github.com/open-webui/open-webui/issues/13535) | 2025-05-05；closed/completed，2025-05-06 关闭 | 被隐藏工具设置可由搜索进入；member/contributor 讨论 API 校验并提供修复提交。 | IMP-10：权限必须在执行边界再次核对；隐藏 CLI/UI 入口不构成授权。 |

## 不采用的扩张

不因为参照项目使用 SQLite、容器、tmux、overlay 或某个模型，就强迫 CortexFS 使用
相同内部设计。不新增根命名空间、不承诺 Windows/macOS、不接入 Open WebUI 前端、
不复制其视觉模板，不把“全量重试”“自动修复历史”“所有工具都需人工审批”当默认。
与本产品无关的平台需求及未审查代码不进入重建任务。

以上教训转化为 `IMPROVEMENTS.md` 的 IMP-01 至 IMP-10，并在 `ACCEPTANCE.md`
形成逐项可执行验收。所有新案例均未在 CortexFS 原版或重建版运行。

## 本项目 issue 回归

GitHub issue 搜索共返回 87 条（含已关闭），筛选近期协议/认证案例并读取下列正文。
这是选样阅读，不是 87 条逐一复核。closed/completed 只表示 issue 管理状态，
不能替代当前产品、原基线或重建版的实际测试。下列大多是历史已关闭回归，
不得称为 CortexFS 当前未修复缺陷。

| 来源 | 报告日期；核对时状态 | 吸收与边界 |
| --- | --- | --- |
| [#294](https://github.com/LIghtJUNction/cortexfs/issues/294) | 2026-09-22；closed/completed | 无 ID 的同名 Gemini 并行调用不能以函数名合并；显式 ID 保留，缺省规则确定且相异。纳入 IMP-11。 |
| [#306](https://github.com/LIghtJUNction/cortexfs/issues/306) | 2026-09-22；closed/completed | 当前单分支事件合同无法表达多个完整候选时拒绝歧义，不静默选第一个；不擅自扩为流式多候选设计。纳入 IMP-11。 |
| [#310](https://github.com/LIghtJUNction/cortexfs/issues/310) | 2026-09-22；closed/completed | MCP structuredContent 不得在投影时丢失；保留已有块、避免相同 JSON 文本重复，不修改中立工具 ABI。纳入 IMP-11。 |
| [#316](https://github.com/LIghtJUNction/cortexfs/issues/316) | 2026-09-23；closed/completed | 历史 Responses 工具投影的 required-nullable strict 使用 null，不能将通用 schema 强制升级 strict=true；Chat 不跟随修改。纳入 IMP-11 的固定历史 fixture，真实最新供应商契约另外核对。 |
| [#284](https://github.com/LIghtJUNction/cortexfs/issues/284) | 2026-09-21；closed/completed | OAuth 认证/令牌/device endpoint 强制 TLS，loopback callback 的 HTTP 例外单独处理。纳入 IMP-12。 |
| [#280](https://github.com/LIghtJUNction/cortexfs/issues/280) | 2026-09-21；closed/completed | device flow 在首次及每次轮询前等待 interval，slow_down 后增加间隔，并受期限限制。纳入 IMP-12。 |
| [#318](https://github.com/LIghtJUNction/cortexfs/issues/318) | 2026-09-23；open | 后续迁移到 hosted CLI、退役自有循环是另一产品方向。仅采用不扩大权限、保持 Unix 边界、减少重复抽象的通用教训；本次不将后续源码功能混入 84f349c 重建基线。 |

这里的协议要求固定为已审查 issue 的历史合同和本包 ABI；它们不是对当下 provider
API 的未验证承诺。IMP-11/12 与前述十项一起构成本轮十二项改进/回归验收。

另核对了 [MCP 2026-07-28 Tools 的 Structured Content](https://modelcontextprotocol.io/specification/2026-07-28/server/tools#structured-content)
和 [RFC 8628 §3.5](https://www.rfc-editor.org/rfc/rfc8628.html#section-3.5) 的官方文本，
确认结构化结果可为任意 JSON 值及其文本兼容建议，以及 device flow 间隔/slow_down
规则。固定协议文本是兼容性依据，不能据此宣称真实服务调用通过。
