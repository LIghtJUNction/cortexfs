# Inspect AI CortexFS Agent Benchmark

这个目录用 Inspect AI 和真实 `ctx` CLI 评估 CortexFS agent。它同时覆盖答案质量、运行时健康、请求延迟、TTFT、吞吐、token 和错误分类。benchmark 不负责启动或停止 agent，因此启动耗时不可用，报告为 `n/a`；preflight 会单独记录每个角色的 ping 和 status 耗时。

## 安装

```bash
cd /home/lightjunction/Documents/GITHUB/cortexfs/inspect_benchmark
./run_benchmark.sh bootstrap
```

执行 benchmark 不会自动联网安装依赖；只有显式 `bootstrap` 才会创建环境并安装依赖。

## 全角色系统评测

默认按顺序测试 `architect`、`coder`、`reviewer` 和 `worker`，避免并发负载污染基线：

以下命令从仓库根目录执行：

```bash
./inspect_benchmark/run_benchmark.sh system
```

常用参数：

```bash
./inspect_benchmark/run_benchmark.sh system \
  --agents coder reviewer \
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

汇总包含 runtime success rate、exact accuracy、错误分类、平均/p50/p95 latency、平均/p50/p95 TTFT、input/output tokens、tokens/s；provider 不返回 token 时回退为 chars/s。启动耗时为 `n/a`，preflight ping/status timing 单独保存在 lifecycle metadata 中。

每个请求使用运行前确认不存在的唯一 session。请求结束后只在 session 属于本次 benchmark、非 `default`、非 current、状态非 active、没有 child reference，且精确 GC preview 只包含该 session 时归档。清理不能得到证明时保留 session、写入 cleanup receipt，并让 benchmark 以非零状态结束。

## Inspect 质量评测

通过 Inspect 的 custom agent API 调用真实 `ctx agent send --raw`：

```bash
./inspect_benchmark/run_benchmark.sh ctx coder
./inspect_benchmark/run_benchmark.sh ctx reviewer --limit 2
```

如果已经执行 `cd inspect_benchmark`，对应命令应写为：

```bash
./run_benchmark.sh ctx coder
./run_benchmark.sh ctx reviewer --limit 2
```

Inspect 使用 `mockllm/model` 作为框架占位模型；实际推理仍由被测 CortexFS agent 当前配置的 model/provider 完成。日志写入 Inspect 默认 `logs/`，可用 `inspect view` 查看。

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
