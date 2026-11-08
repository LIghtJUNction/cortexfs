# CortexFS internal architecture (historical)

[Documentation archive](README.md) · [Architecture](architecture.md) · [Current scope](../PROJECT_SCOPE.md)

> **Retired engineering contract — do not use for new implementation.** This page records internal boundaries from the hosted Agent CLI migration retained through [7261cb3](https://github.com/LIghtJUNction/cortexfs/tree/7261cb399ba1f8efa387736938ea3d8eec1a0acb). It is historical material only; no replacement architecture has been approved.

## Historical layers

The former implementation separated foundation data types, FUSE projection, protocol conversion, execution, and binaries/adapters. It prohibited library dependencies on binaries and kept FUSE independent of launch orchestration.

## Historical process roles

| Process | Former responsibility |
| --- | --- |
| cortexfs-mount | FUSE projection and filesystem enforcement |
| ctx | CLI control surface and launch requests |
| ctxterm | PTY, child lifetime and attach/watch mechanics |
| ctxmcp | MCP adaptation |
| Channel adapter | One external platform transport |
| Hosted Agent CLI | Native model, authentication, tool loop, approval and session behavior |

These processes are retired design elements, not a proposed process topology.

## Historical execution and authority

The former design derived identity, resource access, network authority, process limits, and lifecycle behavior before spawning a hosted CLI. It relied on FUSE, Linux identity and permissions, bwrap, PTY handling, and host policy.

None of these boundaries should be recreated by default. A future product must justify each privileged or operational dependency against one concrete user task.

## Historical persistence and adapters

The old system persisted object definitions, policy, launch receipts, process state, terminal metadata, audit facts, and compatibility session state. Adapters translated external contracts into CortexFS-specific primitives.

Do not carry these stores, managers, registries, or adapters into a new design without a demonstrated consumer.

## Historical testing notes

The old test plan covered invocation, environment, identity, limits, PTY, cancellation, FUSE permissions, network isolation, and MCP conformance. This checkout contains no runtime or test runner with which to execute those tests.

## Current rule

The next design starts from a single user problem and reuses existing components. It must not inherit this architecture merely for compatibility. See [PROJECT_SCOPE.md](../PROJECT_SCOPE.md).