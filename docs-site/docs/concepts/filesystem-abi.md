---
title: Filesystem ABI
---

# Filesystem ABI

CortexFS 的核心不是 UI，而是一组稳定文件路径和读写语义。脚本、agent runtime、workflow engine 和本地工具都应该依赖这些 ABI。

## ABI Rules

- 顶层目录使用单数短名词，如 `provider/`、`model/`、`thread/`。
- 小配置使用小文本属性文件。
- JSON 用于原生 API request/response。
- JSONL 用于消息、审计和训练数据导出。
- Socket 只作为低延迟 fast path，不是 source of truth。
- 慢操作进入 daemon/execution plane。

## File Kinds

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

## Small Text Semantics

```text
一个文件一个值
多值一行一个
布尔值 0/1
整数为十进制
读取带结尾换行
非法写入返回 EINVAL
无权限返回 EACCES
只读写入返回 EROFS
不支持返回 ENOSYS
```
