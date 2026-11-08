# CortexFS 内部架构（历史归档）

[历史文档目录](README.md) · [架构](architecture.md) · [English](../../internal-architecture.md) · [当前范围](../../../PROJECT_SCOPE.md)

> **已退役工程合同：不要用于新实现。** 本文记录截至
> [7261cb3](https://github.com/LIghtJUNction/cortexfs/tree/7261cb399ba1f8efa387736938ea3d8eec1a0acb)
> 的托管 CLI 迁移工程边界，仅为历史追溯保留。当前没有获批的替代架构。

## 历史分层

旧实现将基础数据类型、FUSE 投影、协议转换、执行层和二进制/适配器分开。
库不得依赖二进制；FUSE 不依赖启动编排。

## 历史进程职责

| 进程 | 旧职责 |
| --- | --- |
| cortexfs-mount | FUSE 投影与文件系统权限检查 |
| ctx | CLI 控制入口与启动请求 |
| ctxterm | PTY、子进程生命周期与 attach/watch |
| ctxmcp | MCP 适配 |
| Channel adapter | 单个外部平台传输 |
| Hosted Agent CLI | 原生模型、认证、工具循环、审批与会话行为 |

这些进程是退役设计元素，不是建议的新进程拓扑。

## 历史执行与权限

旧设计在启动托管 CLI 前推导身份、资源访问、网络权限、进程限制和生命周期行为，
依赖 FUSE、Linux 身份与权限、bwrap、PTY 和宿主策略。

默认不应重新建设这些边界。未来产品必须针对一个具体用户任务，逐项证明每个特权或
运维依赖的必要性。

## 历史持久化与适配器

旧系统持久化对象定义、策略、启动回执、进程状态、终端元数据、审计事实和兼容会话状态。
适配器把外部合同转换为 CortexFS 专用原语。

没有明确消费者，不要把这些存储、管理器、注册表或适配器带入新设计。

## 历史测试说明

旧测试计划覆盖调用、环境、身份、限制、PTY、取消、FUSE 权限、网络隔离和 MCP 一致性。
当前 checkout 不包含可运行这些测试的 runtime 或测试运行器。

## 当前规则

下一版从单一用户问题出发，优先复用现有组件。不得仅为兼容旧设计而继承旧架构。
详见 [PROJECT_SCOPE.md](../../../PROJECT_SCOPE.md)。
