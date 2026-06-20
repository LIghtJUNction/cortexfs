---
title: 快速开始
---

# 快速开始

这篇教程按一次真实上手流程走：安装 CortexFS，启动 `/ctx` 挂载，配置第一个 OpenAI-compatible provider，发送第一条请求，然后用 `yazi` 浏览上下文、响应和审计记录。

CortexFS 的核心心智模型很简单：AI 执行面是一棵文件树。你不需要先学一套 SDK，先学会读写这棵树就能开始。

## 0. 你会用到什么

本文假设你在 Linux 上使用已安装的 `cortex` 命令。Arch Linux 推荐从 AUR 安装：

```bash
paru -S cortexfs-git
```

常用辅助工具：

```bash
paru -S jq yazi
```

确认 CLI 可用：

```bash
cortex status
```

你应该看到类似字段：

```text
status=ready
recommended_mount=/ctx
home=/ctx/home/<uid>
```

`/ctx` 是推荐的实机挂载点。仓库里的 `tests/mounts/cortexfs` 只用于开发和集成测试，不要当作日常数据目录。

## 1. 启动并认识挂载树

启动后台挂载：

```bash
cortex start
```

设置当前用户入口：

```bash
export CTX_HOME="/ctx/home/$(id -u)"
```

确认挂载正常：

```bash
findmnt /ctx
cat /ctx/status
cat /ctx/cap/format
cat /ctx/provider/list
cat "$CTX_HOME/model/list"
```

你正在看的不是普通项目目录，而是 FUSE 暴露出来的 ABI。很多文件像 `/proc` 或 `sysfs`：读取它们是在观察运行时状态，写入某些控制文件是在提交动作。

先看几个关键入口：

```bash
ls /ctx
ls "$CTX_HOME"
cat "$CTX_HOME/api/pipeline"
cat "$CTX_HOME/route/openai.chat/provider"
cat "$CTX_HOME/route/openai.chat/model"
cat "$CTX_HOME/route/openai.chat/reason"
```

`api/pipeline` 是本地统一 API 的语义路径。无论以后请求来自文件、HTTP 还是 Unix socket，都必须进入同一套 route、policy、secret、store、audit 和 export 管线。

## 2. 配置第一个 provider

Provider 是一个后端实例，不是厂商品牌。一个中转站、一个 OpenAI-compatible relay、一个本地模型服务，都可以是 provider。

下面用 `relay-a` 作为示例 provider id。把 URL 和模型替换成你的实际服务：

```bash
sudo cortex provider add \
  --id relay-a \
  --name "My first OpenAI-compatible relay" \
  --family openai-compatible \
  --format openai.chat \
  --format openai.responses \
  --base-url "https://api.example.com/" \
  --model "gpt-4o-mini" \
  --priority 80
```

导入 API key。不要把 key 写进命令行参数，避免进入 shell history：

```bash
read -rsp "Provider API key: " API_KEY
printf '\n'
printf '%s' "$API_KEY" | sudo cortex provider key refresh --provider relay-a --stdin
unset API_KEY
```

检查 provider 投影：

```bash
cat /ctx/provider/relay-a/name
cat /ctx/provider/relay-a/format
cat /ctx/provider/relay-a/url/effective
cat /ctx/provider/relay-a/enabled/effective
cat /ctx/provider/relay-a/secrets/status
cat /ctx/provider/relay-a/model/list
```

`secrets/status` 应该显示 `configured`。真实 key 不会出现在挂载树里，挂载树只暴露状态和 secret reference。

把当前用户的 OpenAI chat 默认路由切到这个 provider：

```bash
printf 'relay-a\n' > "$CTX_HOME/route/default_provider"
```

确认 route 已 ready：

```bash
cat "$CTX_HOME/route/openai.chat/provider"
cat "$CTX_HOME/route/openai.chat/model"
cat "$CTX_HOME/route/openai.chat/reason"
```

如果 `reason` 不是 `ready`，优先看：

```bash
cat /ctx/provider/relay-a/enabled/effective
cat /ctx/provider/relay-a/secrets/status
cat "$CTX_HOME/policy/allowed_providers"
```

## 3. 第一条请求：文件 API

文件 API 是 CortexFS 的最基础提交方式。规则只有一条：普通写文件不触发请求，只有同目录原子 rename 到 `*.req.json` 才表示提交。

```bash
api="$CTX_HOME/api/openai.chat"
id="hello-001"

cat > "$api/inbox/$id.tmp" <<'JSON'
{
  "messages": [
    {
      "role": "user",
      "content": "Reply with exactly: cortexfs-ok"
    }
  ]
}
JSON

mv "$api/inbox/$id.tmp" "$api/inbox/$id.req.json"
```

提交后先看入队事实：

```bash
cat "$api/outbox/$id.fingerprint"
cat "$api/outbox/$id.route.json" | jq .
tail -5 /ctx/audit/events.jsonl | jq .
```

触发一次 drain：

```bash
printf '1\n' > /ctx/control/drain
```

读取结果：

```bash
cat "$api/outbox/$id.resp.json" | jq .
```

如果失败：

```bash
cat "$api/outbox/$id.error"
tail -20 /ctx/audit/events.jsonl | jq .
```

这个流程适合 shell、cron、CI、外部 workflow engine。外部系统只需要写临时文件，然后 rename 成请求文件。

## 4. 第一条请求：写入 thread 上下文

如果你希望请求进入一个长期对话上下文，写 `thread/<id>/inbox`：

```bash
thread="$CTX_HOME/thread/demo"
id="turn-001"

cat > "$thread/inbox/$id.tmp" <<'JSON'
{
  "messages": [
    {
      "role": "user",
      "content": "Remember that this thread is a CortexFS quick-start demo."
    }
  ]
}
JSON

mv "$thread/inbox/$id.tmp" "$thread/inbox/$id.req.json"
printf '1\n' > /ctx/control/drain
```

查看 thread 视图：

```bash
cat "$thread/messages.jsonl"
cat "$thread/latest.md"
cat "$thread/fingerprint"
cat "$thread/state"
```

`messages.jsonl` 是可审计历史，`latest.md` 是适合人看的最新回复，`fingerprint` 用于导出、去重和追踪。

## 5. 第一条请求：本地聚合 API smoke

CortexFS 也定义了本地聚合 API 投影：

```bash
cat "$CTX_HOME/api/status"
cat "$CTX_HOME/api/endpoints"
cat "$CTX_HOME/api/http/listen"
cat "$CTX_HOME/api/unix/path"
cat "$CTX_HOME/api/pipeline"
```

当前稳定语义仍以文件 ABI 为 source of truth。生产级 HTTP/Unix socket listener 由运行面提供；如果还没有常驻 daemon，`api/http/status` 或 `api/unix/status` 可能显示 `need-daemon`。

你可以先用 CLI 的一次性本地 API smoke 验证请求形状：

```bash
cortex daemon --once \
  --endpoint /v1/chat/completions \
  --body '{"messages":[{"role":"user","content":"Reply with cortexfs-daemon-ok"}]}' \
  --thread demo
```

它会走本地执行面并返回 OpenAI-compatible 形状的响应。用于真实 provider 的长期使用时，优先使用上面的文件 API，或者启动对应的本地 daemon/listener 后再走 HTTP 或 Unix socket。

如果 HTTP listener 已经运行，你可以按 OpenAI-compatible 方式请求：

```bash
base="$(cat "$CTX_HOME/api/http/localurl")"
curl -sS "$base/chat/completions" \
  -H 'Content-Type: application/json' \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Reply with cortexfs-ok"}]}' \
  | jq .
```

如果 Unix socket listener 已经运行，socket path 在：

```bash
cat "$CTX_HOME/api/unix/path"
```

`.sock` 文件是实时 fast path，不是普通响应文件。不要用 `cat` 读取 socket；响应、审计和上下文事实仍应回到文件树中检查。

## 6. 用 yazi 查看历史上下文

`yazi` 很适合学习 CortexFS，因为你可以像浏览普通目录一样浏览 AI 运行时。

从当前用户入口开始：

```bash
yazi "$CTX_HOME"
```

建议先看这些位置：

```text
api/openai.chat/inbox/
api/openai.chat/outbox/
thread/demo/
thread/demo/tool-loop/
memory/
export/
```

看某个 thread：

```bash
yazi "$CTX_HOME/thread/demo"
```

在 `yazi` 里移动到这些文件，预览窗会直接显示内容：

```text
messages.jsonl          完整消息历史
latest.md               最新人类可读回复
fingerprint             当前上下文指纹
state                   thread 状态
tool-loop/steps.jsonl   tool loop 步骤和结果
io.sock                 实时 fast path，占位为 socket，不当普通文件读
```

看所有对话导出：

```bash
yazi "$CTX_HOME/export"
```

常用文件：

```text
conversations.jsonl     对话训练/审计导出
preference.jsonl        偏好数据
tool_calls.jsonl        工具调用轨迹
agent_traces.jsonl      agent 轨迹
```

看全局审计：

```bash
yazi /ctx/audit
```

重点文件：

```text
events.jsonl            每次 staged、queued、drained、denied 等事实
fields                  审计字段说明
usage                   token/调用用量视图
cost                    成本视图
```

当一次请求结果不符合预期时，按这个顺序查：

```bash
cat "$CTX_HOME/route/openai.chat/provider"
cat "$CTX_HOME/route/openai.chat/model"
cat "$CTX_HOME/route/openai.chat/reason"
cat "$api/outbox/$id.route.json" | jq .
cat "$api/outbox/$id.error"
tail -20 /ctx/audit/events.jsonl | jq .
```

## 7. 你刚刚学会了什么

你已经走过 CortexFS 的核心路径：

1. `/ctx` 是系统级 CortexFS 挂载点。
2. `CTX_HOME=/ctx/home/$(id -u)` 是当前用户唯一工作入口。
3. provider 是后端实例，通过 `provider/inbox` 和 secret store 配置。
4. 请求通过 `tmp -> *.req.json` 的原子 rename 提交。
5. outbox、thread、export、audit 是结果和事实的 source of truth。
6. HTTP 和 `.sock` 是 fast path，不能绕过同一条 pipeline。
7. `yazi` 可以直接浏览上下文、响应、工具轨迹和审计日志。

继续深入时，可以按这个顺序读：

- [第一条请求](./first-request)
- [Provider 实例](../providers/provider-instances)
- [密钥](../providers/secrets)
- [本地聚合 API](../api/local-api)
- [Thread 与批处理](../api/threads-and-batch)
