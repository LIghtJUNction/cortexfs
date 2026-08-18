# AIMock 测试

CortexFS 可以使用 `@copilotkit/aimock` 作为本地兼容 OpenAI 的提供者
用于不应调用真实云 API 的提供程序路径测试。

启动模拟服务器：

```bash
npm install
npm run aimock
```

服务器监听于：

```text
http://127.0.0.1:4010/v1
```

默认夹具位于：

```text
tests/fixtures/aimock/cortexfs-openai-chat.json
```

它返回 `cortexfs aimock ok` 对于 `hi` 和 `hello cortexfs`。

运行冒烟测试：

```bash
npm run aimock:smoke
```

要将 CortexFS 指向模拟对象，请在本地运行时添加一个提供者配置
环境：

```json
{
  "name": "aimock",
  "base_url": "http://127.0.0.1:4010/v1"
}
```

然后将设备密钥存储在 CortexFS 系统秘密存储中：

```bash
printf '%s\n' mock | sudo ctx provider secret set aimock
```

这保持在`/ctx`根ABI之外，并且在进程环境之外。它是一个
本地提供者测试工具，而不是新的 CortexFS 提供者命名空间。
