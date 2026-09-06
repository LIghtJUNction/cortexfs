---
title: Harness 评估
description: 运行确定性的 Agent Harness 契约测试，检查可复现的原始证据。
---

# 评估 Harness

从可靠运行所需的契约开始：事件顺序、上下文边界、工具授权、取消、持久历史与进程边界。评估环境直接运行现有 Rust 实现和测试夹具，使用 Python 标准库，不依赖模型 API。

## 本地运行

在 Linux 下检出仓库，安装仓库指定的 Rust 工具链和系统构建依赖，具体参见[开发指南](developing-cortexfs.md)。请以普通用户运行；部分文件系统与执行测试会检查所有权。

```sh
# 查看契约，无需编译。
python3 evals/harness/run.py --list

# 串行运行全部确定性契约分组。
python3 evals/harness/run.py

# 修改协议或上下文后，先做针对性检查。
python3 evals/harness/run.py --suite wire --suite context

# 要求依赖已经缓存，并禁止 Cargo 访问网络。
python3 evals/harness/run.py --offline

# 保留完整的现有 CI 测试门槛，同时收集相同格式的报告。
python3 evals/harness/run.py --workspace
```

Cargo 使用 `--locked`；首次下载依赖可能需要网络。上面的命令均不调用模型或供应商。每次 Cargo 调用都经过 `scripts/serialize-cargo.sh`，每个 Rust 测试程序使用一个测试线程。使用多个 worktree 时，将 `CORTEXFS_CARGO_LOCK` 设置为同一个共享锁路径。运行器不会安装依赖或修改供应商设置。

结果写入 `target/harness-eval/` 下的新目录。每次运行生成 `report.json`、便于阅读的 `report.md`，以及每次 Cargo 调用的原始日志。可通过 `--output /path/to/new-directory` 指定位置。运行器拒绝已有目录，防止覆盖之前的证据。报告记录 Git 版本、工作区未提交状态、清单摘要、工具链、完整命令、测试数量与退出状态。分享证据时请保留整个目录。

默认超时为**每次 Cargo 调用** 3,600 秒，包含编译与等待锁的时间。工作区首次构建可使用 `--timeout 6600`。超时或中断会终止该调用的进程组，并保留失败报告。耗时字段测量测试或构建命令的运行时间，不能用来表示 Agent 推理延迟。

退出码：`0` 表示所选契约均有通过证据；`1` 表示测试、超时、编译或必需证据检查失败；`2` 表示环境或参数无效；`130` 表示执行被中断。一次调用失败后，后续分组停止执行，报告中明确标记为 `not_run`。只选择部分分组时标记为 `selected`，不得作为完整 Harness 评估展示。

## 夹具能够证明什么

`evals/harness/suites.json` 将每项契约映射到已有 Rust 夹具文件及必需测试名称。每个必需测试都必须实际出现 `ok` 结果。必需测试被改名、缺失或忽略时，即使 Cargo 成功退出，对应契约仍判定失败。没有任何通过测试的调用同样失败。

| 分组 | 验证行为 | 验证边界 |
| --- | --- | --- |
| `wire` | 首次与后续请求封装、观察记录身份与大小、有序流式事件与命令 | 运行时客户端与真实 Unix 流夹具 |
| `context` | 极小 UTF-8 字节预算、输出预留、保留最新观察、原始历史不变 | Context crate |
| `authority` | 联合检查策略、挂载与身份；拒绝符号链接；提示词无法授予工具权限 | 主机授权机制 |
| `cancellation` | 子 Agent 权限收缩；取消追加持久事实并保留历史 | 受所有权约束的子 Agent 生命周期 |
| `recording` | 终端事实仅影响所属运行；重放与幂等；工具成功与拒绝转为观察记录 | 持久会话记录器 |
| `ownership` | 取消和完成的两种先后顺序、并发完成、故障回滚保留替换内容 | 以回执绑定的会话操作 |
| `sockets` | 实际 socket 读取、变更前拒绝未授权对端、空闲超时、取消索引检查 | 单请求 v1 socket 服务 |
| `senders` | Telegram、Discord、Slack 发送者身份；路由默认拒绝 | 渠道事件适配器 |
| `routing` | 被拒绝的用户不会触发分发；获准用户保留独立会话 | 主机桥接层 |
| `schedule` | 委派工作选择已安装的 executor，并要求相应创建权限 | 调度验证 |
| `sdk` | 安装后的 Agent 与 Tool SDK 可执行文件完成两次已声明的原生工具调用；标准 CLI 失败与超大输入 | 安装器、SDK 进程与主机工具循环 |

夹具继续与 Rust 模块放在一起。新增契约时扩展这些测试，并在清单注册证据，无需在评估器中实现另一套 Agent 循环。运行器回归测试检查失败报告、零测试拒绝、覆盖缺失与进程清理：

```sh
python3 -m unittest discover -s evals/harness -p 'test_*.py' -v
```

CI 使用 `--workspace`，保留原有 `--locked --workspace --all-targets --all-features` 门槛。格式、源码预算、Clippy 与文档检查仍独立执行。测试失败时，CI 也会上传证据。

## 证据的适用范围

通过这些契约无法衡量模型推理能力、编程任务成功率、费用、Token 数量、TTFT、p95 延迟或峰值 RSS。本环境不提供综合“智能”分数。各分组职责不同，将测试数量合并为排行榜会掩盖具体失败。

针对性测试不能证明挂载后的 FUSE 行为、systemd/cgroup 约束、完整内核沙箱隔离或持久并发 interaction v2 行为。仅验证 v2 类型无法证明已部署 v2 服务。一些已有工作区测试包含平台条件分支；libtest 的 `ok` 无法反映某个分支是否提前返回。判断这些较广范围的证据时，请阅读对应夹具源码和原始日志。必需契约测试选自已实现边界上的具体断言。

## 可选的真实模型评估

确定性契约通过后，在可丢弃且已经配置好的部署上单独运行真实模型评估。沿用当前 [Harness 替换边界](harness.md)、供应商注册表、路由和密钥配置。明确记录用户选择的供应商与模型。不要绕过挂载后的 Agent 直接调用供应商 SDK，也不要将 `debug/echo` 作为模型质量证据。

使用本地轻量夹具时，先明确安装 `smollm2:135m`，再配置供应商与路由。如果模型不可用，说明原因并停止，不能静默替换模型。调用已配置的付费供应商需要用户明确授权。确定性运行器不会触发这类调用。

使用专用评估 Agent、必要的工具策略和可丢弃工作区，通过正常客户端提交，例如：

```sh
ctx --root /ctx agent send evaluator --session eval-001 \
  "Read input.txt, double its integer, write output.txt, then read output.txt to verify."
ctx --root /ctx agent send evaluator --session eval-001 \
  "Continue from the previous result and explain which tool observations verified it."
```

这些示例要求事先配置好 `evaluator` Agent 与输入夹具。客户端使用标准 Agent socket；文件队列客户端继续遵守同目录原子提交 `*.req.json` 及 outbox/audit 语义。独立检查输出文件和持久会话历史；仅凭最终回复声称成功，证据不足。

记录固定的源码或安装包版本、供应商与模型、生成设置、输入夹具哈希、任务判定规则、工具 schema、策略、并发数、步骤与 Token 预算、超时、每次尝试（含失败）、工具和模型调用数量，以及原始事件。私有轨迹保留在本地，分享前移除密钥。取消与恢复实验应在该可丢弃部署上执行。

与其他 Harness 比较时，需要相同任务、工具和预算，以及重复测量和不确定性报告。本环境不作比较性性能或质量声明。

## 替换旧基准测试

旧的 `inspect_benchmark/` Inspect/Pi 运行器、数据集和 Python 依赖锁文件、不等价的协议计时示例与生成的性能卡片已移除，仍可从 Git 历史恢复。新环境不依赖 Inspect、云 SDK 或独立评分组件。已有 Rust 正确性测试全部保留。[2026-09-05 验证报告](reports/2026-09-05-harness-validation.md)继续作为历史证据保留，其中原有的局限和失败实验不变。
