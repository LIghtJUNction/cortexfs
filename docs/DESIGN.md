# CortexFS Design

This document is the entry point for the CortexFS v1 ABI specification.
Normative details live under [spec/](spec/).

CortexFS v1 is intentionally small:

```text
/ctx is a FUSE Agent OS ABI view.
model is a pure inference file.
agent is the policy-bound orchestrator.
tool is a capability endpoint.
session is ordinary file history.
policy is a minimal SELinux-like allowlist.
Rig removes provider and API-format differences.
CortexFS does not express provider/API formats as root ABI.
MCP servers are tool sources; MCP capabilities are ordinary tools.
CortexFS controls agent visibility, execution, and sharing, not framework config formats.
```

The root rule is frozen for v1:

```text
root only contains stable object classes
root never mirrors provider, database, workflow, memory, or orchestration internals
MCP must not become a root namespace
MCP configs, skills, project rules, and prompt packages are ordinary visible files
```

Core invariants:

```text
Context is a working set, not the full history.
Raw history is durable.
Prompt context is disposable and rebuildable.
Compaction must not destroy raw messages.
Independent tasks should run in child agents.
Child agents are owned by their parent unless explicitly detached by policy.
Owned children die when the parent dies.
```

Read these files in order:

```text
spec/README.md
spec/root-abi.md
spec/fuse-v1.md
spec/object-abi.md
spec/model-abi.md
spec/session-abi.md
spec/16-context.md
spec/agent-tool-security.md
spec/tool-policy-abi.md
spec/17-child-agents.md
spec/ctx-coreutils.md
spec/phase-1.md
```

The v1 red line:

```text
Do not let /ctx become a directory mirror of an AI platform database.
It should stay small, hard, boring, and scriptable.
```
