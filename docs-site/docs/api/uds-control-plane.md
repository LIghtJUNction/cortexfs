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
```

未来会增加会话级 stream socket：

```text
home/<uid>/thread/<id>/io.sock
home/<uid>/job/<id>/stream.sock
```

## 设计要求

- FUSE 线程不做远程 API 调用。
- 网络请求进入 Tokio worker 线程池。
- FUSE 与 API worker 通过队列通信。
- `.sock` RPC 可以承载高频控制、状态查询和流式 token。
- UDS 可以用 `SCM_RIGHTS` 接收外部进程传入的文件描述符，避免大文件经 FUSE 复制。
- stream socket 支持双向交互，允许中断、追加输入和工具结果回传。
- telemetry 通过 socket 读取运行态延迟、缓存命中、额度和队列深度。

## 未来流式体验

```bash
nc -U /ctx/home/$(id -u)/job/translate.zh/stream.sock
```

LLM 返回第一批 token 后立即写入 socket。终端会像打字机一样看到增量输出。

## 约束

开发期刷新仍以 Git commit 为唯一边界。不要新增热加载、轮询或后台监听子命令；socket listener 只能属于已安装服务/daemon 的运行面。
