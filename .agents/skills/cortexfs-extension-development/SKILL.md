---
name: cortexfs-extension-development
description: This skill should be used when the user asks to "build a CortexFS tool", "create a CortexFS agent", "package a CortexFS extension", "write a cortexfs.object/v1 manifest", "install an external tool", or "install an external agent".
version: 0.1.0
---

# CortexFS Extension Development

Build executable Tool SDK and Agent SDK extensions without adding a root ABI,
provider special case, watcher, queue, or alternate orchestration path.

## Workflow

1. Read `docs/architecture.md`, `docs/naming-guide.md`, and the relevant files
   under `docs/spec/` before changing an ABI surface.
2. Use CodeGraph before text search when `.codegraph/` exists.
3. Reuse the Rust SDK matching the object class:
   `cortexfs-tool-sdk` for tools and `cortexfs-agent-sdk` for agents.
4. Implement one executable per object. Accept argv or stdin and emit canonical
   JSONL. The Agent SDK entrypoint performs the runtime startup capability ping;
   do not bypass it. Keep model-provider formats outside the extension contract.
   Treat `CTX_SOURCE` only as a candidate path: runtime receipt dev/inode/type
   is authoritative for durable writes.
   Agent SDK extensions that consume the hosted typed invocation envelope must
   set the agent control `abi` to `sdk-envelope-v1`; omitting `abi` selects the
   exact legacy `argv-v1` launch contract. Follow the normative
   [Agent Runtime envelope](../../../docs/spec/agent-runtime.md) rather than
   duplicating its schema in extension documentation.
5. Create a strict `cortexfs.object/v1` manifest. Read
   `references/manifest.md` before choosing controls or an install tier.
6. Build the executable, calculate its SHA-256, and place the exact digest in
   the manifest. Do not put commands, arguments, wrappers, secrets, or policy
   grants for unrelated objects in the manifest.
7. Validate source and manifest changes without installing first. Run
   `ctx object check MANIFEST` for each rendered manifest; it requires no
   source tree and performs no backing-tree writes. Follow
   `references/testing.md` for the staged test ladder.
8. Perform installation only after an explicit mutation request. Invoke
   `ctx object install --source PATH MANIFEST --tier user|system`, where PATH
   is the durable backing tree. Never use `/ctx`, `CTX_ROOT`, or `--root` as
   the writable target, and never copy directly into a live object directory.
9. Verify discovery, metadata, policy denial, authorized execution, JSONL
   ordering, and durable agent behavior through the installed path.

## Boundaries

- Keep `/ctx/status`, `/ctx/bin`, `/ctx/model`, `/ctx/agent`, `/ctx/tool`,
  `/ctx/home`, and `/ctx/shared` as the only root classes.
- Treat manifest installation as new-object-only. Use a new name for an
  upgrade until a separately specified replacement contract exists.
- Treat the executable publication as the object commit boundary. Do not
  fabricate runtime-owned `status`, `pid`, `log`, or socket state.
- Keep tool installation separate from authorization. Installing a tool must
  not edit an agent policy.
- Install user tools under `/ctx/home/<effective-uid>/tool` and system tools
  under `/ctx/tool`. Install agents through v1 only under `/ctx/agent`; although
  the root ABI defines `/ctx/home/<effective-uid>/agent`, the v1 installer
  rejects user-tier agents because it cannot carry tier identity to the root
  socket runtime.
- Use Git commits or process restarts as development refresh boundaries. Do not
  add polling, watchers, or hot reload.
- Preserve executable plugins as the supported extension path. Do not claim
  the Tool SDK `DynamicTool` loader or native resident cache is wired into the
  core runtime until a core consumer and end-to-end proof exist.

## Canonical Example

Use `examples/extensions/echo/` as the canonical paired extension. It contains
one Tool SDK executable, one Agent SDK executable, strict manifest templates,
and an explicitly opted-in installer that installs the tool before the agent.
Link to this example instead of copying it into skill resources.

## References

- `references/manifest.md` — manifest schema, controls, tiers, and security.
- `references/testing.md` — read-only validation and opt-in live acceptance.
