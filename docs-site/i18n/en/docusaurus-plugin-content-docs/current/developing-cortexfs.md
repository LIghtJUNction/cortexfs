---
id: developing-cortexfs
title: Extending CortexFS
sidebar_label: Extending CortexFS
---

# Extending CortexFS

Start with one rule: CortexFS extension points are the current spec's objects,
sockets, control files, and tool commit semantics. They are not new root
directories or new workflow entrances.

## Read The Boundary First

Suggested order:

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

The root ABI only contains:

```text
/ctx/status
/ctx/bin
/ctx/model
/ctx/agent
/ctx/tool
/ctx/home
/ctx/shared
```

Do not add top-level directories such as `provider`, `workflow`, `job`, `hook`,
`mcp`, `skill`, or `audit`.

## Extend Tools

A tool is an executable capability endpoint. Users see:

```text
/ctx/tool/<name>
/ctx/tool/<name>.d/
```

Execution can happen in the Rust runner, an external program, or runtime
internals, but authority is still decided by the agent view, `CTX_PATH`, and
policy.

For asynchronous tools or tools with retrievable results, use the unified
commit semantics:

```text
1. Write a temporary file.
2. Atomically rename it in the same directory to *.req.json.
3. Read results from outbox.
4. Append facts to audit.
```

## Extend Agents

An agent is a policy-bound orchestrator. Stable paths are:

```text
/ctx/agent/<name>
/ctx/agent/<name>.sock
/ctx/agent/<name>.d/
/ctx/home/<uid>/agent/<name>/session/
```

Agents may organize tool loops, context, child tasks, and handoff, but those
orchestration concepts should not become new root ABI.

The current `ctx agent start` terminal path is:

```text
systemd-run --user
bwrap sandbox
ctxterm
tsh
```

By default, it mounts the caller's current directory at `/workspace` inside the
sandbox. Extra mounts must be declared with `--mount SOURCE TARGET ro|rw`;
`TARGET` must not replace `/` or `/ctx`. This path is the agent terminal
implementation, not a new background watcher, polling loop, or hot-reload
subcommand.

`ctxterm` owns the PTY and exposes `watch` and `attach` through the session
terminal socket:

```text
/ctx/home/<uid>/agent/<agent>/session/<session>/terminal/main.sock
```

`tsh` only looks up tools through `CTX_PATH`; it does not fall back to the host
`PATH`. If `CTX_PATH` is unset, it may read `CTX_HOME/.tshrc`, but that file
only supports data-form `CTX_PATH=...`.

## Extend Providers Or Local Models

The provider/model design must stay neutral. CortexFS does not make any vendor
a core default path, and it does not make Ollama a core special branch.

The lightweight local live-test fixture uses:

```text
smollm2:135m
```

If that model is missing, tell the user to install or pull it; do not silently
switch models. When a user explicitly asks to test their configured provider or
aggregation API, use the existing provider registry, routes, secret state, and
unified commit semantics.

Provider API key resolution order is fixed:

```text
1. environment variable named by provider config
2. system keychain, for example service=cortexfs:<provider> account=default
3. unconfigured, return a stable error
```

Do not write secrets into `/ctx/model/*`, `.d/default`, or any other ABI file.

When you need to test an OpenAI-compatible provider path without calling a
cloud API, use this repository's aimock fixture:

```bash
npm install
npm run aimock
npm run aimock:smoke
```

See [AIMock Testing](aimock-testing.md) for details. This is a local test
fixture, not a new `/ctx/provider` root namespace.

## Local Verification

Common checks:

```bash
cargo test
npm --prefix docs-site run build
```

The fixed FUSE integration test mount point is:

```text
tests/mounts/cortexfs
```

This directory is only a local test mount point. Do not put source, fixtures,
or persistent data there.
