---
title: Design Spec
---

# Design Spec

The canonical design specification is maintained in Chinese at
`docs/DESIGN.md` and is also published in the default Simplified Chinese docs.

This English page summarizes the most important contract:

- CortexFS is a FUSE/VFS projection. `cortexd` owns execution.
- FUSE callbacks must not call remote providers or perform slow work.
- The mount tree is an ABI, not a UI.
- API format, provider instance, model, route, secret state, audit, and export
  are separate objects.
- Provider/model design must stay neutral; no provider is a core special case.
- Secrets never enter the mounted tree as raw values.
- File submission, HTTP, and Unix socket fast paths must share the same route,
  policy, store, audit, and export pipeline.

For complete details, switch to Simplified Chinese and open the design spec.
