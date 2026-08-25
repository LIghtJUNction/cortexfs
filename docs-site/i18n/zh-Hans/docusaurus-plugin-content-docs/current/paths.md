---
sidebar_position: 6
---

# 路径 ABI 与 cortexfs-paths

`cortexfs-paths` 是 CortexFS 集成的单一路径布局 crate。它被独立发布，便于
channel host、agent runtime、SDK 扩展和运维工具在不从主实现复制字面量的情况下
推导路径：

~~~toml
cortexfs-paths = "0.1.7"
~~~

其版本与 CortexFS 的 workspace 版本保持一致。CortexFS 的每次发布会同步发布主
包与 `cortexfs-paths`，版本号相同。该 crate 没有运行时依赖，只做路径拼装；
它不创建目录、不打开文件、不挂载 FUSE，也不启动守护进程。

## 三类路径角色

同一个 agent 可以有多个 socket 路径，且它们故意不同：

| 角色 | API | 示例 |
| --- | --- | --- |
| 公共客户端 ABI | `agent_client_socket` | /ctx/agent/executor.sock |
| 私有 systemd listener | `system_agent_runtime_socket` | /run/cortexfs/agent/executor.sock |
| 持久回写树 | `agent_backing_socket` | /var/lib/cortexfs/storage/current/agent/executor.sock |
| 用户终端 ABI | `session_terminal_path` | /ctx/home/1000/agent/executor/session/default/terminal/main.sock |
| 终端 Broker 端点 | `BROKER_SOCKET` | /run/cortexfs/terminal/broker.sock |

IM 适配器通常从 `agent_client_socket` 或显式配置的 runtime contract 读取其
`agent_socket`。它不应推断第二个 socket 路径。Discord 的 host 配置文件本身即
`channel_config_path("discord")`，该路径解析为 `/etc/cortexfs/channels/discord.toml`。

## 公共 `/ctx` 布局

冻结的顶层条目通过 `ROOT_ENTRIES` 导出：

~~~text
/ctx/status
/ctx/bin
/ctx/model
/ctx/agent
/ctx/tool
/ctx/home
/ctx/shared
~~~

对对象与会话路径应使用对应角色函数：

~~~rust
use cortexfs_paths::{
    agent_control_file_path, agent_socket_path, ctx_root, model_path,
    session_file_path, tool_path, validate_component,
};

let root = ctx_root();
validate_component("executor")?;
let socket = agent_socket_path(&root, "executor");
let status = agent_control_file_path(&root, "executor", "status");
let model = model_path(&root, "openai", "gpt-5.6");
let tool = tool_path(&root, "fs.read");
let messages = session_file_path(&root, "1000", "executor", "default", "messages.jsonl");
~~~

动态名称故意保持为 `&str`，使 crate 保持无额外依赖。调用方应当在拼装不可信值
前先执行 `validate_component`（或使用主机应用等价的 object-name 校验策略）。
拼装辅助函数不会访问文件系统，也不会跟随符号链接。

## 主机路径

该 crate 还集中管理 `/ctx` 之外的路径：

- `SYSTEM_STORAGE_DIR` 与 `storage_root_path()` 标识持久化存储。
- `SYSTEM_STORAGE_CURRENT` 与 `storage_current_path()` 标识选定的 generation。
- `provider_config_path`、`provider_secret_path`、`provider_model_cache_path`
  暴露 provider 状态，不通过 `/ctx` 泄露 secrets。
- `channel_config_path` 用于文件化 IM 适配器配置。
- `SYSTEM_RUNTIME_DIR`、`RUN_CONTROL_SOCKET` 与 agent 运行时辅助路径用于私有运行平面。

调用方负责权限与原子写入语义。配置更新仍遵循 CortexFS 规则：写入同级临时
文件、`fsync`、原子重命名为最终文件，并将结果事实追加到对应 audit 流。

## 兼容性规则

`cortexfs-paths` 是路径名的 ABI 边界。新的集成应直接依赖它：

~~~toml
cortexfs-paths = "0.1.7"
~~~

不要在适配器中重复出现 `/ctx`、`/run/cortexfs`、`/var/lib/cortexfs` 或
`/etc/cortexfs` 字面量。路径 ABI 变更时 crate 会随主项目变更版本并在此处以及
`docs/spec/` 规范文档中记录迁移。
