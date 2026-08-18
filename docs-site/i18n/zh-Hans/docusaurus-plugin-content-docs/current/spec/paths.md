# 路径 ABI

`cortexfs-paths` 是 CortexFS 集成的公共路径 ABI。主 workspace 与该 crate 使用同一
发布版本。需要定位 CortexFS 文件或 socket 的应用应依赖该 crate，而非复制实现中的
路径字面量。

## 稳定根

公共 FUSE 根为 `/ctx`。其唯一稳定顶层条目为：

```text
status  bin  model  agent  tool  home  shared
```

该 crate 导出 `CTX_ROOT`、`ROOT_ENTRIES`，并为每类根对象、控制文件、model、agent、
tool、home、shared 以及持久会话路径提供组合函数。`root_entry_path` 不接受未知根条目。

## Agent socket 角色

同一 agent 可有三种不同的 socket 路径，不能混用：

| 角色 | Crate 函数 | 示例 |
| --- | --- | --- |
| 公共客户端 ABI | `agent_client_socket` | `/ctx/agent/coder.sock` |
| 私有系统运行时 | `system_agent_runtime_socket` | `/run/cortexfs/agent/coder.sock` |
| 持久 backing tree | `agent_backing_socket` | `/var/lib/cortexfs/storage/current/agent/coder.sock` |

公共客户端路径是通道适配器契约，除非部署显式配置其他运行时合同。channel
适配器不得从主机文件系统推断第二个 socket 路径。

终端资源同理区分 durable 的 `/ctx/home/.../terminal` 路径与实时的
`/run/user/<uid>/cortexfs/...` 传输路径。

## 主机路径

该 crate 管理稳定的主机位置，包括：

- 持久存储及其所选 `current` generation；
- provider 配置、provider secrets 与 model 缓存；
- `/etc/cortexfs/channels` 下的文件化通道适配器；
- system agent runtime 与 control sockets。

这些函数仅组合 `PathBuf` 值，不会创建文件、跟随链接、启动服务或授予权限。
调用方仍保持现有的明文文件检查、权限控制、原子写入临时文件加重命名规则，以及
audit 追加语义。

## 动态组件

`validate_component` 会拒绝空值、类似 `.` 的值、包含分隔符、NUL 字符以及过长值。它
是通用路径组件校验器；主实现可在对象 ABI 要求处施加更严格的对象名规则。

该 crate 没有运行时依赖，可安全地被 channel host、SDK 扩展、agent runtime 与运维
工具使用。
