# CortexFS 规范

> 历史译文：本页保留的是 `84f349c` 源码快照对应的中文规范摘要。当前 `docs/spec/`
> 文档更新到上游 commit `7261cb3`，描述后续 hosted Agent CLI 迁移；两个快照有差异。
> 重建旧快照时请使用 [CortexFS Recipkg recipe](../../../../recipes/cortexfs/RECIPE.md) 中的完整规范。

本目录是 CortexFS ABI 的规范文档。产品实现已从当前仓库移除。

CortexFS 通过小型 Linux 文件系统接口暴露 agent runtime。路径、对象执行、持久
session 事实、权限和带版本的 socket 帧属于可观察契约。只要不改变这些契约，
provider、存储和内部实现都由重建者选择。

稳定根目录只有以下 8 个条目：

```text
/ctx/
  status
  bin/
  model/
  agent/
  tool/
  channel/
  home/
  shared/
```

核心边界：

```text
model 产生推理事件，但不执行工具
agent 决定是否请求工具；policy 决定是否允许执行
tool 是显式能力端点
session history 持久保存；prompt context 可重建
provider/API 细节不会扩展根命名空间
提交统一使用规范中的原子 request、outbox 和 audit 语义
```

## 规范文件

| 文件 | 契约 |
| --- | --- |
| [root-abi.md](root-abi.md) | 稳定 `/ctx` 根、参考树与文件规则 |
| [paths.md](paths.md) | 公开路径名与主机路径职责 |
| [fuse.md](fuse.md) | FUSE 投影与文件系统行为 |
| [object-abi.md](object-abi.md) | executable、socket 与 `.d/` 对象界面 |
| [model-abi.md](model-abi.md) | 模型执行、路由、认证与事件流 |
| [session-abi.md](session-abi.md) | 持久历史、run 身份与 session 索引 |
| [agent-tool-security.md](agent-tool-security.md) | agent 身份、可见性、挂载与权限 |
| [agent-runtime.md](agent-runtime.md) | runtime、context、tools、子任务与 sandbox |
| [tool-policy-abi.md](tool-policy-abi.md) | tool 文件、policy、shared 与审计事实 |
| [terminal-abi.md](terminal-abi.md) | terminal 资源、PTY 事件与 attach |
| [terminal-broker.md](../../../spec/terminal-broker.md) | terminal broker 认证与描述符授权 |
| [interaction-abi.md](interaction-abi.md) | 前端/runtime 交互帧与兼容性 |
| [channel-abi.md](channel-abi.md) | channel 实例、路由、driver 与 host 边界 |
| [module-abi.md](module-abi.md) | 外部 module 元数据、生命周期与 wire 格式 |
| [ctx-coreutils.md](ctx-coreutils.md) | `ctx` 命令与输出契约 |
| [rolling-upgrades.md](rolling-upgrades.md) | reference-tree 更新与 storage generation 切换 |
