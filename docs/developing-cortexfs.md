---
id: developing-cortexfs
title: 二次开发
sidebar_label: 二次开发
---

# 二次开发

二次开发时先守住一个原则：CortexFS 的扩展点是当前规范里的对象、socket、控制文件和
tool 提交语义，不是新的根目录或新的 workflow 入口。

## 先读规范边界

建议顺序：

```text
DESIGN.md
spec/README.md
spec/root-abi.md
spec/object-abi.md
spec/model-abi.md
spec/session-abi.md
spec/tool-policy-abi.md
spec/ctx-coreutils.md
aimock-testing.md
```

根 ABI 只包含：

```text
/ctx/status
/ctx/bin
/ctx/model
/ctx/agent
/ctx/tool
/ctx/home
/ctx/shared
```

不要新增 `provider`、`workflow`、`job`、`hook`、`mcp`、`skill`、`audit` 这类顶层目录。

## 扩展 tool

tool 是可执行能力端点。用户看到的是：

```text
/ctx/tool/<name>
/ctx/tool/<name>.d/
```

具体执行可以在 Rust runner、外部程序或 runtime 内部完成，但权限仍然由 agent view、
`CTX_PATH` 和 policy 决定。

涉及异步或需要结果回收的 tool，使用统一提交语义：

```text
1. 写临时文件
2. 同目录原子 rename 成 *.req.json
3. 从 outbox 读取结果
4. 向 audit 追加事实
```

## 扩展 agent

agent 是 policy-bound orchestrator。稳定路径是：

```text
/ctx/agent/<name>
/ctx/agent/<name>.sock
/ctx/agent/<name>.d/
/ctx/home/<uid>/agent/<name>/session/
```

agent 可以组织 tool loop、上下文、child task 和 handoff，但不要把这类编排概念提升成
新的根 ABI。

`ctx agent start` 的当前终端路径是：

```text
systemd-run --user
bwrap sandbox
ctxterm
tsh
```

默认把调用者当前目录挂到 sandbox 内 `/workspace`。额外挂载必须通过
`--mount SOURCE TARGET ro|rw` 显式声明；`TARGET` 不能替换 `/` 或 `/ctx`。这条路径是
agent 终端实现，不是新的后台监听、轮询或热加载子命令。

`ctxterm` 拥有 PTY，并通过 session terminal socket 暴露 `watch` 和 `attach`：

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

`tsh` 只按 `CTX_PATH` 查找 tool，不回退到 host `PATH`。如果 `CTX_PATH` 未设置，可以读
`CTX_HOME/.tshrc`，但该文件只能包含数据形式的 `CTX_PATH=...`。

## 扩展 provider 或本地模型

provider/model 设计必须保持中立。CortexFS 不把某个供应商写成核心默认路径，也不把
Ollama 作为核心特殊分支。

本地轻量 live test fixture 使用：

```text
smollm2:135m
```

如果该模型不存在，提示用户安装或拉取；不要静默换模型。用户明确要求测试自己配置的
供应商或聚合 API 时，走现有 provider registry、route、secret 状态和统一提交语义。

供应商 API key 的解析顺序固定为：

```text
1. provider 配置指定的环境变量
2. 系统 keychain，例如 service=cortexfs:<provider> account=default
3. 未配置，返回稳定错误
```

不要把 secret 写入 `/ctx/model/*`、`.d/default` 或其他 ABI 文件。

需要测试 OpenAI-compatible provider 路径而不调用云 API 时，使用本仓库的 aimock fixture：

```bash
npm install
npm run aimock
npm run aimock:smoke
```

详细说明见 [AIMock Testing](aimock-testing.md)。这是本地测试 fixture，不是新的
`/ctx/provider` 根命名空间。

## 本地验证

常用检查：

```bash
cargo test
npm --prefix docs-site run build
```

FUSE 集成测试挂载点固定为：

```text
tests/mounts/cortexfs
```

该目录只作为本地测试挂载点，不放源码、fixture 或持久化数据。
