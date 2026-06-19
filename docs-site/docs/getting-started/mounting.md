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

## 后台挂载

默认生产挂载使用 systemd：

```bash
cortex start
```

`cortex start` 会请求系统授权管理一个 `cortexfs@<owner>.service` 实例，自动准备 `/ctx`，修正 owner/mode，并在停止时向前台 mount 进程发送退出信号。该 systemd 挂载默认使用 multi-user FUSE 模式；同一台机器不要为多个 Linux 用户同时启动多个 `/ctx` 挂载实例。

## 多用户挂载

前台调试时也使用 multi-user 模式：

```bash
cortex mount --multi-user /ctx
```

路径只是命名空间，不是安全边界。真实访问决策应基于 host credential、external subject、object context 和 policy。

## 刷新边界

开发期刷新以 Git commit 为唯一事件边界。已挂载实例暴露一个确定实现版本；观察新 ABI 时需要提交后重建并重新挂载。
