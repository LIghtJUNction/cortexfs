---
title: 挂载
---

# 挂载

CortexFS 的挂载树是公开 ABI。外部程序应该通过发现文件读取能力，而不是写死 provider、model、uid 或 demo thread。

## 推荐挂载点

```text
/ctx
```

`CTX_HOME` 是当前 Linux 用户的 CortexFS 工作入口：

```bash
export CTX_HOME="/ctx/home/$(id -u)"
```

常用入口：

```text
$CTX_HOME/api
$CTX_HOME/thread
$CTX_HOME/model
$CTX_HOME/route
$CTX_HOME/memory
$CTX_HOME/export
```

## 单用户和多用户

默认挂载是 single-user。多用户挂载需要明确使用 multi-user 模式，并配合系统 FUSE 的 `allow_other`、目录 owner、group、mode 策略。

```bash
cortex mount --multi-user /ctx
```

在 Arch Linux 上，先确认 `/etc/fuse.conf` 中启用了 `user_allow_other`。如果没有该配置，FUSE 会拒绝 `allow_other` 挂载选项。

路径只是命名空间，不是安全边界。真实访问决策应基于 host credential、external subject、object context 和 policy。

## 刷新边界

开发期刷新以 Git commit 为唯一事件边界。已挂载实例暴露一个确定实现版本；观察新 ABI 时需要提交后重建并重新挂载。
