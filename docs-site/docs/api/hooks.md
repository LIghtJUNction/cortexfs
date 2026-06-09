---
title: 定时任务与 Hook
---

# 定时任务与 Hook

CortexFS 不内置后台轮询、热加载或 scheduler。推荐分工：

```text
systemd timer / cron / git hook / CI   负责触发
CortexFS hook 文件 ABI                  负责输入、输出、状态、日志、审计
cortexd                                后续负责真正的异步执行面
```

## 文件形状

```text
home/<uid>/hook/
  count
  list
  <id>/
    trigger
    spec
    req
    out.json
    status
    last
    log.jsonl
```

`trigger` 只是声明触发器来源，例如 `manual`、`systemd.timer`、`cron`、`git.pre-commit`。CortexFS 不会因为它自动启动后台监听。

`spec` 描述要做什么。当前第一版支持把 hook 绑定到结构化任务：

```text
kind=job
job=translate.zh
from=en
to=zh
fields=text,from,to,input
```

写入 `req` 会生成 `out.json`，并更新 `status`、`last` 和 `log.jsonl`。

## 示例：systemd timer 触发翻译

先创建 hook：

```bash
hook="/ctx/home/$(id -u)/hook/daily-translate"
mkdir "$hook"

printf 'systemd.timer\n' > "$hook/trigger"
cat > "$hook/spec" <<'EOF'
kind=job
job=translate.zh
from=en
to=zh
fields=text,from,to,input
EOF
```

外部 timer 只需要写请求：

```bash
systemd-run --user --on-calendar='daily' \
  sh -c 'cat ~/todo.txt > /ctx/home/$(id -u)/hook/daily-translate/req'
```

读取结果：

```bash
cat "$hook/out.json"
cat "$hook/status"
cat "$hook/log.jsonl"
```

## 持久化

hook 的 `trigger` 和 `spec` 会写穿到：

```text
~/.config/cortexfs/hook.d/<id>.conf
```

重启挂载时自动恢复。`req`、`out.json`、`status`、`last` 和 `log.jsonl` 是运行态投影。
