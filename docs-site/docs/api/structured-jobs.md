---
title: 结构化任务
---

# 结构化任务

结构化任务用于“写规范、写请求、读 JSON 输出”的简单工作流。翻译、抽取字段、分类、改写都应走同一类文件 ABI。

## 文件形状

```text
home/<uid>/job/
  count
  list
  <id>/
    spec
    req
    out.json
    status
```

`spec` 是小文本规范，`req` 是请求输入。写入 `req` 后，CortexFS 按 `spec` 生成 `out.json`。

## 翻译示例

```bash
job="/ctx/home/$(id -u)/job/translate.zh"
mkdir "$job"

cat > "$job/spec" <<'EOF'
kind=translate
from=en
to=zh
out=json
fields=text,from,to,input
EOF

printf 'hello world\n' > "$job/req"
cat "$job/out.json"
cat "$job/status"
```

输出：

```json
{"text":"你好，世界","from":"en","to":"zh","input":"hello world"}
```

当前实现是同步确定性执行，后续会把同一 ABI 接到 worker 线程池和 LLM 流式输出。用户脚本不需要改变。
