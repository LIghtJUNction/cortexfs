# Inspect AI CortexFS Agent Benchmark

这个目录用 Inspect AI 和真实 `ctx` CLI 评估 CortexFS agent。它同时覆盖答案质量、运行时健康、请求延迟、TTFT、吞吐、token 和错误分类。默认 5 条短答案 JSONL 只是协议、计分和开销 smoke，不是 coding benchmark，也不能支持“全面胜出”结论。benchmark 不负责启动或停止 agent，因此启动耗时不可用，报告为 `n/a`；preflight 会单独记录每个角色的 ping 和 status 耗时。

## 安装

```bash
cd /home/lightjunction/Documents/GITHUB/cortexfs/inspect_benchmark
./run_benchmark.sh bootstrap
```

执行 benchmark 不会自动联网安装依赖；只有显式 `bootstrap` 才会创建环境并安装依赖。

## 全角色系统评测

默认按顺序测试当前内置的 `architect`、`executor` 和 `product-manager`，避免并发负载污染基线；`--agents` 也接受安全命名的已安装自定义 agent：

以下命令从仓库根目录执行：

```bash
./inspect_benchmark/run_benchmark.sh system
```

常用参数：

```bash
./inspect_benchmark/run_benchmark.sh system \
  --agents executor product-manager \
  --repeat 3 \
  --timeout 180 \
  --output-dir results
```

运行前必须已有可用的 canonical agent listener。benchmark 会在创建输出目录和发送请求前依次要求：

- `ctx status` 显示 running、ready、mounted
- `ctx doctor` 全部为 `ok`
- `/ctx` 是带 `default_permissions` 和 `allow_other` 的 `cortexfs` FUSE mount
- 每个角色的 `ctx ping agent/ROLE` 返回单个 JSON `pong`
- 每个角色的 `ctx agent status ROLE` 第一行严格为 `idle`

任一检查失败都会立即退出。benchmark 不会自动执行 `ctx agent start` 或 `ctx agent stop`。

每次运行会生成：

```text
results/<run-id>/samples.jsonl
results/<run-id>/summary.json
results/<run-id>/report.md
```

汇总包含 runtime success rate、exact accuracy、错误分类、平均/p50/p95 latency、平均/p50/p95 TTFT、分层 token usage、tokens/s 和内存证据。默认内存证据取一次 agent service invocation 的 systemd `MemoryPeak`，并同时记录持久 `cortexfs.service` 的请求前后端点；由于后者不是独立 cgroup peak，`complete_stack_peak=false`，不得据此宣称完整栈 RSS 获胜。启动耗时为 `n/a`，preflight ping/status timing 单独保存在 lifecycle metadata 中。

每个请求使用运行前确认不存在的唯一 session。请求结束后只在 session 属于本次 benchmark、非 `default`、非 current、状态非 active、没有 child reference，且精确 GC preview 只包含该 session 时归档。清理不能得到证明时保留 session、写入 cleanup receipt，并让 benchmark 以非零状态结束。

## Inspect 质量评测

通过 Inspect 的 custom agent API 调用真实 `ctx agent send --raw`：

```bash
./inspect_benchmark/run_benchmark.sh ctx executor
./inspect_benchmark/run_benchmark.sh ctx product-manager --limit 2
```

如果已经执行 `cd inspect_benchmark`，对应命令应写为：

```bash
./run_benchmark.sh ctx executor
./run_benchmark.sh ctx product-manager --limit 2
```

Inspect 使用 `mockllm/model` 作为框架占位模型；实际推理仍由被测 CortexFS agent 当前配置的 model/provider 完成。日志写入 Inspect 默认 `logs/`，可用 `inspect view` 查看。

## Pi JSONL 公平基线

Inspect 质量路径：

```bash
MODEL_ID=<shared-upstream-model-id>
PI_BENCH_PROVIDER=openai-codex \
PI_BENCH_MODEL="$MODEL_ID" \
PI_BENCH_THINKING=max \
./inspect_benchmark/run_benchmark.sh pi --limit 2
```

可重建的系统 summary 路径：

```bash
PI_RESULT=$(./inspect_benchmark/run_benchmark.sh pi-system \
  --provider openai-codex \
  --model "$MODEL_ID" \
  --thinking max \
  --repeat 1 \
  --timeout 180 \
  --output-dir results/pi)

./inspect_benchmark/run_benchmark.sh system \
  --agents executor \
  --provider codex \
  --model "$MODEL_ID" \
  --thinking max \
  --repeat 1 \
  --timeout 180 \
  --compare-summary "$PI_RESULT/summary.json" \
  --output-dir results/ctx
```

Pi runner 强制 JSON mode，并关闭 tools、extensions、skills、prompt templates、themes、context files、approval 和 session。TTFT 取第一个非空白 `text_delta`，最终非空 assistant `message_end` 后必须出现 `agent_settled`；`stopReason=error/aborted` 即使进程退出码为 0 也算失败。响应 provider/model 必须与请求一致。每条样本记录脱敏生命周期证据、usage 覆盖率和 20 ms 有界冷进程树 RSS；后者会漏掉短命/逃逸子进程，所以明确标记为 descriptive/incomplete，不进入内存胜负门禁。

`fingerprint` 包含 dataset、repeat、timeout、规范化 provider family、model、thinking、任务协议和实际 observation plan；角色、精确二进制和 benchmark-source SHA-256、稳定的 system/prompt/context controls、tools/root/cwd/mount/policy/perm hash、内存方式等进入 `treatment_fingerprint`。ctx 的声明 provider/model/thinking 会与实际 agent route 和 model effort 逐项核对。使用多个 ctx 角色对一个 Pi subject 会产生不同 workload fingerprint。小于 100 个成功独立 task 时 p95 只能视为描述值。

## 配对 ABBA 与 A/A 噪声

单独运行两个 summary 只能给出点估计；正式延迟/usage 对比使用 `paired`，每个 task/epoch 严格按 `A B B A` 串行。compare 模式中 A=一个指定的 ctx agent endpoint、B=Pi；A/A 模式两边使用同一 runtime。它不等于多-agent team 端到端评测；shell pipeline 的离线 smoke 只证明编排 ABI 和易用性，不能替代团队质量/延迟/usage benchmark：

```bash
COMMON=(
  --ctx-provider codex
  --pi-provider openai-codex
  --model "$MODEL_ID"
  --thinking max
  --credential-scope-attested
  --agent executor
  --repeat 1
  --timeout 180
  --retain-frames
)

./inspect_benchmark/run_benchmark.sh paired --mode aa-ctx "${COMMON[@]}" \
  --output-dir results/aa-ctx
./inspect_benchmark/run_benchmark.sh paired --mode aa-pi "${COMMON[@]}" \
  --output-dir results/aa-pi
./inspect_benchmark/run_benchmark.sh paired --mode compare "${COMMON[@]}" \
  --output-dir results/paired
```

每个 mode 每个 task/epoch 发出 4 次计分请求；默认 5 条 dataset、`repeat=1` 即 20 次，另有每个实际 runtime 1 次不计分 warm-up（compare 为 2 次，单 runtime A/A 为 1 次）。warm-up 必须成功、具有完整 input/output token 语义、保留完整协议证据并完成 session/主进程组/取消 helper 进程组清理，否则 gate 失败。先用这一档做授权后 smoke，不能把它当 p95 结论。每个 task/epoch 产生两个反向 pair；bootstrap 按 `sample_id` 整簇重采样，不能把同一 task 的 epoch/pair 伪装成独立样本。A/A 对每个有效反向 pair 的原始 ratio 分布直接计算 `exp(P95(abs(log(A/B))))-1`；不能先做几何平均，否则会压低尾部噪声。少于 100 个独立 task 时 p95 会标记为 descriptive。失败请求的 usage/cost 仍计入 attempt 总量和 per-success 分母；跨 runtime token 指标只比较两边都有的 visible input+output，并单独报告 cache/reasoning component coverage。保留帧中的非秘密长文本不截断，秘密字段的 redacted field/byte 数与整帧 drop 数都会进入样本；所有冷 CLI 与取消 helper 进程组必须用 PGID+leader start-time 验证无残留。当前 runner 还会哈希 systemd unit fragment/drop-in/ExecStart、完整 model/agent controls，并在请求前后验证活动 FUSE/agent MainPID、InvocationID、可执行文件和非秘密环境 identity 稳定；每个 ctx 请求同时从 cgroup 捕获实际执行进程。当前 runner 不自动合并三次 run 成胜负结论，必须由独立 reviewer 从 `warmups.jsonl`、`samples.jsonl`、`pairs.jsonl` 和 SHA-256 重建，并检查 improvement lower bound 是否超过 `max(3%, 2×noise)`。

成功发布时，runner 会在同一 run 目录生成覆盖 raw JSONL、`summary.json` 和 `report.md` 的 `manifest.sha256`，逐文件重读校验后 `fsync`，再把文件和目录封成 owner-read-only；任一步失败都不会返回成功路径。`immutable_artifacts=true` 表示这种生成时原子 no-replace、hash manifest 和只读封存已经完成，属于可检测篡改的 evidence seal，不宣称内核 `FS_IMMUTABLE_FL` 或对文件所有者绝对不可变。独立 reviewer 仍须重新校验 manifest，而不能只信 summary 中的布尔值。

## 同一 Codex 订阅账户

Pi 与 ctx 应分别持有同一账户签发的 OAuth grant，而不是复制同一个长期 credential 文件。先从本地 registry 核对两边确有同一个上游 model ID：

```bash
pi --list-models openai-codex
ctx ls model/codex
```

然后安装 preset，并把 ctx 登录写入运行时默认读取的 `default` profile：

```bash
sudo ctx provider preset install codex
sudo ctx auth login codex --profile default
sudo ctx auth status codex --profile default
sudo ctx set agent/executor.d/model "codex/$MODEL_ID"
sudo ctx set "model/codex/$MODEL_ID.d/effort" max
```

Pi 在交互会话中执行 `/login openai-codex`，并选择同一个账户。benchmark Python 不读取、解析、复制或打印 `~/.pi/agent/auth.json`；Pi 子进程本身仍通过自己的 `HOME`/agent dir 读取 OAuth。只有用户在两次 OAuth 登录中选择同一个订阅账户，才能称为“同一订阅账户”；compare 还要求显式传入 provider-neutral 的 `--credential-scope-attested`；对 Codex 场景，它表示操作者确认两次授权属于同一订阅账户，对无凭据的本地 provider 则表示两边处于同一无凭据范围。它只是记录人工确认，并不等于机器验证或共享 token owner。

订阅没有可归因到单次请求的账单，因此实际增量成本固定报告为 `n/a`。如需 API 目录价等价估算，必须显式提供带版本的价格来源和费率，例如：

```bash
./inspect_benchmark/run_benchmark.sh pi-system \
  --provider openai-codex \
  --model "$MODEL_ID" --thinking max \
  --price-source openai-2026-08-31 \
  --input-usd-per-million 1.25 \
  --output-usd-per-million 10.00
```

示例数字不是项目默认价格；使用者必须替换为已核验、带日期的目录价。缺失任一请求（包括失败请求）的完整 usage，成本即为 `n/a`，不会按 0 计算。

生成只读点估计对照：

```bash
CTX_RESULT=results/ctx/<run-id>
COMPARE_RESULT=$(./inspect_benchmark/run_benchmark.sh compare \
  --ctx-summary "$CTX_RESULT/summary.json" \
  --pi-summary "$PI_RESULT/summary.json" \
  --output-dir results/compare)
```

当前 comparator 只消费 summary，无法验证 raw pairing、bootstrap CI 或 reviewer 身份，因此 `all_pass` 恒为 false；`--require-all-gates` 会按设计返回非零。没有 A/A 噪声、AB/BA 或 ABBA 原始顺序、足够样本、完整 cgroup memory 和独立 raw-data reviewer receipt 时，报告一律保持 `inconclusive`，不得写“碾压”。

## 其他桥接模式

以下示例同样从仓库根目录执行：

直接模型 baseline：

```bash
./inspect_benchmark/run_benchmark.sh model --model openai/gpt-5.6
```

通用 CLI bridge：

```bash
./inspect_benchmark/run_benchmark.sh agent \
  -S command=agent_benchmark/examples/openai_cli_agent.py \
  --model openai/gpt-5.6
```

In-process OpenAI bridge：

```bash
./inspect_benchmark/run_benchmark.sh process --model openai/gpt-5.6
```

## 真实调用说明

`ctx` 和 `system` 模式通过 `/ctx` 的 canonical agent socket 以及现有 provider registry、route 和 secret 发起真实请求。它们可能产生费用，只有用户明确授权真实 configured-provider 调用后才能运行。benchmark 不会读取或打印 secret，也不会把某个 provider 写成默认特例；需要离线 smoke test 时使用项目已有的 `debug/echo` 路径。
