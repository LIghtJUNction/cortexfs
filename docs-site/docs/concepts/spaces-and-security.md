---
title: 空间和安全
---

# 空间和安全

Space 是权限、审计、记忆和执行的边界。

## 用户入口

```text
home/<uid>/
  policy/
  route/
  model/
  api/
  thread/
  memory/
  export/
```

`home/<uid>` 是用户工作入口。`space/` 是只读安全上下文索引，不是第二个提交入口。

## 安全输入

访问决策应基于：

- `HostActor`：Linux `uid/gid/pid`。
- `Subject`：被代表的外部用户，例如 `chat:user:123456`。
- `Object`：文件系统对象、provider、model、tool、memory、thread 等资源。

## 安全流程

```text
FUSE request
  -> host credential
  -> optional verified external subject
  -> object context
  -> Unix mode check
  -> Cortex policy check
  -> allow/EACCES
  -> audit
```

密钥、OAuth token、session token 不进入挂载树。挂载树只暴露 secret 状态、active key id 和 rotate 控制节点。
