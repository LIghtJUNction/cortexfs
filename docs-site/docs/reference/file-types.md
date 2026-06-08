---
title: 文件类型
---

# 文件类型

## 命名

```text
无扩展名        小文本属性或控制节点
*.req.json      原生 API 请求
*.resp.json     原生 API 响应
*.error         错误对象
*.jsonl         append-only 日志、消息、审计、训练数据
*.md            人类可读视图
*.sock          Unix domain socket fast path
schema.json     大结构 schema
manifest.json   大结构 manifest
```

## 错误语义

```text
非法写入返回 EINVAL
无权限返回 EACCES
只读写入返回 EROFS
不支持返回 ENOSYS
```

## 提交

`*.req.json` 必须通过 staged tmp 文件加原子 rename 提交：

```bash
printf '%s\n' "$json" > "$inbox/001.tmp"
mv "$inbox/001.tmp" "$inbox/001.req.json"
```

普通 write 不触发 provider 调用。
