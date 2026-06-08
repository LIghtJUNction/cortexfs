# Bun CortexFS 客户端模板

这个模板是一个零依赖的 CortexFS Bun 客户端。

它支持两种传输方式：

- `file`：通过稳定的 CortexFS 文件 ABI 提交，并写入 `control/drain`。
- `http`：调用从 `home/<uid>/api/http/listen` 派生出的本地 OpenAI-compatible API。

默认使用 `file` transport，因为它适配当前 CortexFS 投影。`http` transport 用于已经运行本地 API daemon 的场景。

## 运行

```bash
cd templates/bun-cortexfs-client
bun run chat -- "Reply with exactly: cortexfs-ok"
```

使用已经挂载的仓库测试挂载点：

```bash
export CORTEXFS_MOUNT=../../tests/mounts/cortexfs
export CORTEXFS_UID=1000
bun run route
bun run chat -- "Reply with exactly: cortexfs-ok"
```

使用生产式挂载点：

```bash
export CTX_HOME=/ctx/home/$(id -u)
bun run models
bun run chat -- "hello"
```

使用本地 HTTP API：

```bash
export CORTEXFS_TRANSPORT=http
bun run chat -- "hello"
```

## 环境变量

```text
CTX_HOME             显式 CortexFS 用户 home，例如 /ctx/home/1000。
CORTEXFS_MOUNT      挂载根目录。默认：/ctx。
CORTEXFS_UID        home/ 下的用户 id。默认：进程 uid，然后回退到 1000。
CORTEXFS_FORMAT     API format。默认：openai.chat。
CORTEXFS_TRANSPORT  file 或 http。默认：file。
CORTEXFS_BASE_URL   可选，本地 API base URL 覆盖值。
CORTEXFS_API_KEY    可选，HTTP bearer token。
CORTEXFS_PROMPT     未提供 CLI prompt 时使用的 prompt。
```

这个模板不会读取 provider API key。CortexFS 应该在 `cortexd` 内解析真实 provider secret；挂载树只暴露 route 和 secret 状态。
