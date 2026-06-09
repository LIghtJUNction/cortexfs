---
title: UDS 控制平面
---

# UDS 控制平面

CortexFS 使用双平面设计：

```text
FUSE data plane     cat/echo/ls/mv，稳定文件 ABI
UDS control plane   .sock 双向 IPC、流式输出、telemetry、FD 传递
```

文件树是可审计 source of truth。Unix socket 是低延迟 fast path，不能绕过 policy、route、secret、store、audit。

## Socket 位置

当前 ABI 暴露：

```bash
cat /ctx/home/$(id -u)/api/unix/path
ls -l /ctx/home/$(id -u)/api/unix/api.sock
ls -l /ctx/home/$(id -u)/thread/demo/io.sock
```

不会增加 `home/<uid>/job/<id>/stream.sock`。持续会话走 `thread/<id>/io.sock`；一次性任务走 inbox/outbox。

## 设计要求

- FUSE 线程不做远程 API 调用。
- 网络请求进入 daemon 队列或 worker 线程池。
- FUSE 与 API worker 通过队列通信。
- `.sock` RPC 可以承载高频控制、状态查询和流式 token。
- UDS 可以用 `SCM_RIGHTS` 接收外部进程传入的文件描述符，避免大文件经 FUSE 复制。
- socket 必须写同一 store、audit 和 export，不能形成旁路。

## 约束

开发期刷新仍以 Git commit 为唯一边界。不要新增热加载、轮询或后台监听子命令；socket listener 只能属于已安装服务/daemon 的运行面。
