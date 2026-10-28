# 本轮新增改进合同

这是用户在 Recipkg 迁移中要求吸收同类项目 issue 后增加的需求。来源与取舍见
`RESEARCH.md`；以下验收由本项目设计，不是外部项目的实测成绩。全部尚未运行。
它们补充 REQ-001 至 REQ-021，保持八项根、v1 兼容、provider-neutral、单一事实
历史和主机权限边界。不得靠关闭隔离或编造成功来满足体验要求。

## IMP-01：提交可见性与一致元数据（P0，REQ-002/004）

对于规范要求原子发布的请求/控制更新，在已提交名称可发现时，读者必须能取得
有效且相互一致的内容和元数据。并发 readdir/stat/read 不得看到已经宣称提交成功
却未完成的 inode、link count 或请求。普通未关闭文件的分段写语义仍按 FUSE/POSIX
合同处理，不把所有普通写入强行变成事务。

验收：对合成对象并发列目录、stat、读取和同目录 rename 发布；在每个提交步骤注入
失败，结果只允许旧完整状态、新完整状态或规范错误；无空壳已提交请求、假 outbox
成功。检查 rename/unlink/mkdir 的规范可见性，不限制具体存储实现。

## IMP-02：中断恢复与数据保全（P0，REQ-008/017）

已确认 durable 的消息、事件、配置和用户文件必须在进程异常退出后保留。读取到
损坏/未完成 JSONL 尾部时按规范拒绝不可信历史，不自动截断、覆盖或重建用户文件。
恢复诊断说明受影响的 session/对象、最后可确认事实和下一步；不伪造完成。

验收：在追加、请求提交、工具返回和 generation 切换前后终止进程；重启核对已确认
事实、文件 hash 和合成 Git fsck。损坏样本保持可恢复原件，相同请求不重复副作用。
记录持久化保证的实际边界，不宣称防任意硬件故障。

## IMP-03：真实 Git 子模块与 worktree（P1，REQ-007/010/011）

Agent 使用 Git 时应明确 cwd、挂载来源、可写范围和写入目标。支持在已授权可写
挂载内的普通仓库、嵌套子模块和 linked worktree；gitdir 位于授权范围外时，先给出
缺失映射/权限诊断。不能静默改写 host 路径、放开整棵 home、自动增加挂载或把隔离
副本的 commit 说成 host 已更新。

验收：用本地合成仓库执行 status/diff、获准编辑/commit，核对目标 gitlink、对象库、
工作树和现存 dirty 文件；对只读或范围外 gitdir 保持拒绝且数据不变。测试无需联网
push，不触碰用户仓库。明确 core 元数据和用户工作区是不同的写入边界。

## IMP-04：并发、公平性和有界资源（P1，REQ-002/008/014）

同 session 保持规范的单写者；独立 session 的无关 IO 不应被不必要的全局等待串行
阻塞。缓冲、队列、慢消费者、超大帧和取消都必须有界并有可解释行为。具体界限由
规范或重建者明确配置，不能无限增长，也不能凭空声称某吞吐达标。

验收：按 PERFORMANCE 的元数据/1、2、8 session/慢消费者方案记录基线、等待与
RSS；一个慢连接不能让其他 session 无法状态查询或取消。使用实际数据决定预算，
不得引用 AgentFS 的性能倍率为本产品成绩。

## IMP-05：失败回合可恢复且不重复输入（P0，REQ-004/008/011/012）

运行中、等待工具、失败、取消与已完成必须由真实 run 状态驱动输出；失败后不能
继续显示正在思考。保留已经接收但未回答输入的可见关联，提供基线允许的显式
resume/status/cancel 或具体恢复指导，不能要求用户盲目重发同一消息来恢复状态。

相同 ID/相同内容重试仍严格按基线重放，不追加第二条输入，不隐式重跑工具。
需要新尝试时显式告知，保留原请求关联和不确定副作用；不得偷偷改变 v1 JSONL。
重试须有可取消的次数/时间边界和可见状态；无授权不能自动重复写入型操作。

验收：合成 provider 在首次响应前、部分输出后失败/超时；终端显示实际错误和恢复
办法，重复请求不增消息/事件，断线重连不丢原输入或伪报 done。

## IMP-06：工具调用关联与不确定结果（P0，REQ-005/008/010/014）

模型 tool call、Agent 请求、工具结果、durable facts 的 request/run/session/tool
关联必须核对；不按“最后一个请求”或数组位置猜配结果。迟到、重复、缺失、未知
或错误 session 的结果不可进入另一调用的 prompt。重启时不能把旧结果当新结果。

验收：合成两个 session 的并行调用，交换/延迟/重复工具响应并在工具副作用之后、
结果持久化之前终止进程。系统拒绝错配、重放可确认事实；不能证明是否完成时明确
报告不确定，阻止自动再次执行。v2 相关路径只在另行实现 v2 后验收，不冒充基线。

## IMP-07：durable 与临时数据边界（P0，REQ-008/017/018）

显式列出 private/shared session、获准工作区、配置和 secret 的持久位置，以及 temp
session、cache、PTY/runtime 状态的生命周期。服务重启/更新不能隐式重建或清空用户
工作区。缺失挂载或持久目录时拒绝声称恢复原状态，不能启动成一个同名空 session。

验收：创建合成工作文件、已确认历史与临时文件，重启 runtime 和切换失败的 generation；
durable 内容和权限保持，temp 按声明回收。故意去掉挂载后，doctor/resume 给出明确
错误，保留旧数据。容器仅是可选测试环境，不是新增部署要求。

## IMP-08：可写且隔离的临时目录（P1，REQ-007/013/018）

为获准执行提供明确的私有临时空间，支持临时文件、socket 与编译工具基本需求；
不可为修复只读 tmp 而暴露整个 host /tmp。不同身份/隔离实例的 tmp 可见性按其
权限边界隔离。容量、生命周期和异常退出清理必须说明，不能清理其他在用实例。

验收：两个隔离 Agent 创建相同临时文件名并使用 socket；互不覆盖，无法读取 host
假 secret。正常/异常退出及空间不足后无假成功；清理不删除其他活动目录。

## IMP-09：中文、终端与缺失 locale（P1，REQ-011/012/013/018）

在 LANG/LC_ALL 未设置、C/POSIX 与可用 UTF-8 locale 下，CLI/PTY 要么可靠处理
中文/多字节文本，要么在产生副作用前说明具体 UTF-8 前提。只调整本进程/子进程
所需环境，不修改用户系统配置；终端初始化失败不能只暴露内部异常名。

验收：双语输入、流式输出、多行编辑、复制/重连与非 TTY；缺 locale/终端后端时
显示可操作错误，取消恢复终端。不得要求重建者采用 tmux 才能满足此项。

## IMP-10：能力前提与执行授权一致（P0，REQ-007/010/012/015/016）

对已有 policy 要求审批的操作，临时 session、断连或无法持久化必要审批状态时应
拒绝或不提供该能力，不能把 ask 退化为 allow。CLI、socket、channel、SDK/MCP
走相同授权边界；隐藏入口/搜索项不是权限检查。撤销权限后新请求按新权限处理。

验收：合成受限身份在 private/shared/temp session 通过各入口请求同一工具，
包含审批允许、拒绝、断线、重启、状态丢失和权限撤销。未经批准副作用计数为零；
UI/CLI 显示与实际执行一致。此项不把所有工具改成人工审批，也不增加新 root。

## IMP-11：协议投影无静默丢失（P0，REQ-005/006/016）

将本项目 #294/#306/#310/#316 转成回归条件。供应商专用规则停留在适配器，
不把供应商状态或字段塞进 Agent/session/FUSE 核心。

验收使用固定合成 fixture：同名无 ID Gemini 调用产生确定且相异的中立关联 ID，
原显式 ID/参数/thought signature 保持；只有一个候选的完整响应按原语义解码，
零/多候选（非已识别 provider error）拒绝，不静默丢弃分支。这里不改变流式合同。

MCP 结果仅含 structuredContent 时仍投影出有效 JSON 文本；原 content 块顺序
保持，已有相同序列化文本时不重复追加。覆盖 object/array/scalar/null 和 isError。
固定历史 Responses 工具样例保留 strict:null，Chat 样例不增加该字段，通用 schema
不能擅自变 strict:true。当前服务 API 若变化，先明确版本和差异再更新 adapter，
不要为通过测试编造供应商支持或新中立协议。

## IMP-12：认证传输和有界 device flow（P0，REQ-006/018）

OAuth authorization/token/device/verification endpoint 在发送请求前验证 HTTPS；
HTTP loopback redirect 是另一个用途，可保留已定义本机回调例外，不误放行 HTTP
token endpoint。错误与日志不得带 secret，重定向也不能导致凭据明文传输。

按 RFC 8628 及本项目 #280 的固定合同，在首次和每次 token poll 前等待 interval，
缺省 5 秒，slow_down 后每次后续间隔加 5 秒；以 caller timeout 与 expires_in
较短者为截止。取消立即停止后续轮询；这是显式认证命令，不是后台文件监听。

验收使用本地 TLS 合成服务、可注入时钟与假凭据：错误 HTTP endpoint 在网络计数
为零时被拒；本机 HTTP callback 仍有效；核对 pending/slow_down/success/denied/
expired/timeout/cancel 的请求时刻和次数。不需要真实 provider 账号，也不放宽
证书验证。OAuth 2.0/Device 的标准依据是 RFC 6749、RFC 8628；loopback 例外见
[RFC 8252](https://www.rfc-editor.org/rfc/rfc8252.html)。认证之外的 provider endpoint
行为仍按 model-abi 和本地配置验收；轮询规则见
[RFC 8628 §3.5](https://www.rfc-editor.org/rfc/rfc8628.html#section-3.5)。
