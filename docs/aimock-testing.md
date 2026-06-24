# AIMock Testing

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
  "base_url": "http://127.0.0.1:4010/v1",
  "api_key_env": "CORTEXFS_AIMOCK_API_KEY"
}
```

Then set:

```bash
export CORTEXFS_AIMOCK_API_KEY=mock
```

This stays outside the `/ctx` root ABI. It is a local provider test fixture,
not a new CortexFS provider namespace.
