# CortexFS 规范

本目录是 CortexFS ABI 的规范文档（canonical source）。

CortexFS 将 AI 运行时转换为轻量 Linux 文件系统接口：
路径本身是 ABI，可执行文件仍是文件，控制状态位于
`<name>.d/`，有状态交互使用 `<name>.sock`。

稳定形状：

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

核心原则：

```text
root 已冻结
root 只包含稳定对象分类
model 是纯推理端点
agent 拥有编排与权限
tool 是可执行能力端点
session 是普通文件
context 是可重建的工作集
原始历史是持久的
独立任务应在子代理执行
归属子代理在父代理结束时终止
sockets 使用 JSONL
control 文件是小型文本文件
provider/API 细节不进入根 ABI
```

工具边界：

```text
model 可发出 tool_call 事件
model 不能直接执行工具
agent 决定是否执行工具
agent policy 决定是否允许执行
```

协议边界：

```text
CortexFS 负责：文件 ABI、agent 生命周期、socket 会话、权限、chroot、bind mount、CTX_PATH、shared/home
CortexFS 协议适配层负责：provider 连接、API 格式兼容、model 调用、流/事件适配、底层 provider 异常行为
agent 负责：工具循环、context 组织、子任务交接、是否执行工具
```

CortexFS **不将以下定义为根 ABI**：

```text
provider registry
API format registry
database backend
vector database backend
workflow/job/hook DSL
spawn/factory/agent-template root
cluster scheduler DSL
MCP registry root；MCP 服务器是外部配置并可投影成普通工具
skill registry root；skill 文件是普通可见文件，不授予权限
audit root
control root
```

规范文件：

```text
root-abi.md             冻结 /ctx 根、稳定 reference tree 与基础文件规则
fuse.md                 FUSE 投影形态
object-abi.md           executable、socket、.d 对象三元组
model-abi.md            模型 ABI、模型执行、模型 socket、事件流
session-abi.md          持久会话和会话索引
agent-tool-security.md  agent 身份、视图、挂载与创建
agent-runtime.md        端到端 agent runtime、REPL、终端、tsh、sandbox
tool-policy-abi.md      tool ABI、MCP 投影、shared、policy、日志
ctx-coreutils.md        ctx 命令契约
rolling-upgrades.md     rolling reference-tree 更新与 storage 切换规则
```

## 外部参考

- CortexFS 规范文档及本仓库实现。
- [模型上下文协议](https://modelcontextprotocol.io/specification/)
- [Linux FUSE 文档](https://www.kernel.org/doc/html/latest/filesystems/fuse/fuse.html)
- [mcp-filesystem 实现](https://github.com/search?q=mcp-filesystem)
