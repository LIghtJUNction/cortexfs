---
id: extensions
title: One-file Extensions
sidebar_label: One-file Extensions
---

# One-file Extensions

CortexFS follows the same anti-framework extension rule as Pi: add behavior at
stable edges (packages, executables, skills, modules, channel adapters) without
a second root ABI or a resident plugin daemon. Product placement of those edges
is in [architecture.md](architecture.md) (*Extension points*). This page is the
shortest authoring path.

The shortest way to add behavior is one package directory. Keep the program
logic in normal executables; keep the wiring in one `cortexfs.toml`:

```text
review-kit/
├── cortexfs.toml
└── bin/
    ├── review-agent
    └── git-summary
```

```toml
schema = "cortexfs.package/v1"
name = "review-kit"
version = "0.1.0"

[[tools]]
name = "git.summary"
run = "bin/git-summary"
description = "Summarize the current Git worktree"
schema = { type = "object" }

[[agents]]
name = "kit_reviewer"
run = "bin/review-agent"
model = "main"
tools = ["git.summary"]
instructions = "Review changes, use the tool when useful, and cite evidence."
parent = "agent:architect"
```

Validate the complete package without writing a backing tree, then install it:

```bash
ctx install --check ./review-kit
ctx install ./review-kit
```

For a prebuilt package, bind each member to its distributed bytes by adding the
exact lowercase digest next to `run`, then require every member to have one:

```toml
[[tools]]
name = "git.summary"
run = "bin/git-summary"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
```

```bash
ctx install --check --require-hashes ./review-kit
```

Replace the example digest with the output of `sha256sum`. Every declared
`sha256` is checked, even without `--require-hashes`; the flag additionally
rejects a package when any tool or agent omits its digest.

`ctx install --check` finds `cortexfs.toml`, hashes every executable, renders
and checks every strict object manifest, then exits before choosing or writing
a backing tree. Normal `ctx install` repeats those checks before publishing
each object through the existing atomic object installer. A matching digest
binds the descriptor to the executable bytes, but does not authenticate a
publisher or registry. Obtain the descriptor and expected digest through an
authenticated trusted channel; `cortexfs.package/v1` does not define signatures.

Use `--source PATH` when the mounted tree is backed by a specific generation,
and use `--tier user` for tools that should only be visible to the current user.
Agent objects are system-tier because their runtime socket is host-owned:

```bash
ctx install ./review-kit --source /var/lib/cortexfs/storage/current
```

Agent Unix identity is host authority, not package metadata. The installer
derives it from its effective user and supplementary groups; package authors
cannot select a uid, gid, or privileged group.

The `run` file is the extension point. A tool implements the Tool SDK and an
agent implements the Agent SDK; both are ordinary executable files, so a Rust,
shell, or another host-language build can produce them. An SDK agent receives
one hosted envelope on stdin and returns JSONL events. It may yield a tool call;
the host performs the capability check and sends the observation back for the
next step. This is the custom execution loop, without a resident plugin daemon.

Topology is just the `parent` edge. Every agent names its parent as
`agent:NAME` (optional `session:` and `run:` qualifiers remain available), so a
tree is visible in the same control files that enforce ownership:

```toml
[[agents]]
name = "planner"
run = "bin/planner"
parent = "agent:architect"

[[agents]]
name = "builder"
run = "bin/builder"
parent = "agent:planner"
```

The package file is authoring input, not a second `/ctx` namespace. After
installation the durable result is still only `agent/<name>.d/*`,
`tool/<name>.d/*`, ordinary session files, and the existing sockets. The raw
`ctx object install` command remains available for package builders that need
full manifest control; most users do not need to see it.

Refresh is explicit: commit the package or restart the process that consumes
the source generation. `ctx install` never starts a watcher, polling loop, or
background plugin service.
