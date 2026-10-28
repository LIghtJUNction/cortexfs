# AIMock Testing

> 存档说明：本文描述 CortexFS `v0.1.21` 的本地测试流程；测试源码与脚本已移除，
> 本文不是当前验证结果。重建入口见
> [CortexFS Recipkg recipe](../recipes/cortexfs/BUILD_PROMPT.md)。

CortexFS can use `@copilotkit/aimock` as a local OpenAI-compatible provider
for provider-path tests that should not call a real cloud API.

Start the mock server:

```bash
npm install
npm run aimock
```

The server listens on:

```text
http://127.0.0.1:4010/v1
```

The default fixture lives at:

```text
tests/fixtures/aimock/cortexfs-openai-chat.json
```

It returns `cortexfs aimock ok` for `hi` and `hello cortexfs`.

Run the smoke test:

```bash
npm run aimock:smoke
```

To point CortexFS at the mock, add a provider config in your local runtime
environment:

```json
{
  "name": "aimock",
  "base_url": "http://127.0.0.1:4010/v1"
}
```

Then store the fixture key in the CortexFS system secret store:

```bash
printf '%s\n' mock | sudo ctx provider secret set aimock
```

This stays outside the `/ctx` root ABI and outside process environments. It is a
local provider test fixture, not a new CortexFS provider namespace.
