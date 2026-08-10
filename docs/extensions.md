---
id: extensions
title: One-file Extensions
sidebar_label: One-file Extensions
---

# One-file Extensions

The shortest way to add behavior is one package directory. Keep the program
logic in normal executables; keep the wiring in one `cortexfs.yaml`:

```text
review-kit/
├── cortexfs.yaml
└── bin/
    ├── review-agent
    └── git-summary
```

```yaml
schema: cortexfs.package/v1
name: review-kit

tools:
  - name: git.summary
    run: bin/git-summary
    description: Summarize the current Git worktree
    schema: '{"type":"object"}'

agents:
  - name: reviewer
    run: bin/review-agent
    model: main
    tools: [git.summary]
    instructions: Review changes, use the tool when useful, and cite evidence.
    parent: agent:architect
```

Install it with one command:

```bash
ctx install ./review-kit
```

`ctx install` finds `cortexfs.yaml`, hashes every executable, validates the
whole package, then publishes each object through the existing atomic object
installer. Use `--source PATH` when the mounted tree is backed by a specific
generation, and use `--tier user` for tools that should only be visible to the
current user. Agent objects are system-tier because their runtime socket is
host-owned:

```bash
ctx install ./review-kit --source /var/lib/cortexfs/storage/current
```

The `run` file is the extension point. A tool implements the Tool SDK and an
agent implements the Agent SDK; both are ordinary executable files, so a Rust,
shell, or another host-language build can produce them. An SDK agent receives
one hosted envelope on stdin and returns JSONL events. It may yield a tool call;
the host performs the capability check and sends the observation back for the
next step. This is the custom execution loop, without a resident plugin daemon.

Topology is just the `parent` edge. Every agent names its parent as
`agent:NAME` (optional `session:` and `run:` qualifiers remain available), so a
tree is visible in the same control files that enforce ownership:

```yaml
agents:
  - name: planner
    run: bin/planner
    parent: agent:architect
  - name: builder
    run: bin/builder
    parent: agent:planner
```

The package file is authoring input, not a second `/ctx` namespace. After
installation the durable result is still only `agent/<name>.d/*`,
`tool/<name>.d/*`, ordinary session files, and the existing sockets. The raw
`ctx object install` command remains available for package builders that need
full manifest control; most users do not need to see it.

Refresh is explicit: commit the package or restart the process that consumes
the source generation. `ctx install` never starts a watcher, polling loop, or
background plugin service.
