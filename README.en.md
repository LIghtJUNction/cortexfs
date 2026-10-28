# CortexFS

[简体中文](README.zh-CN.md)

This repository is now CortexFS's documentation and ABI reference. The product
implementation, tests, examples, build and installation scripts, and
documentation-site implementation have been removed from the current source
tree. Preserved operational guides describe the `v0.1.21` source snapshot and
serve as reference material; they are not executable instructions for this
checkout.

## Rebuild prompt

The self-contained reconstruction package lives in the
[`cortexfs-prompts` submodule](docs/cortexfs-prompts/README.md). Start with
[`BUILD_PROMPT.md`](docs/cortexfs-prompts/BUILD_PROMPT.md). It records the source
baseline, evidence, known documentation conflicts, and file hashes. It has not
been validated by an independent rebuild.

## Canonical documents

- [Normative ABI index](docs/spec/README.md)
- [Architecture and product boundaries](docs/architecture.md)
- [Documentation status and provenance](docs/README.md)
- [Visual design tokens](docs/DESIGN.md)
- [Simplified Chinese translations](docs/translations/zh-Hans/)

The detailed root ABI defines eight stable entries, including
`/ctx/channel/`. Historical marketing copy may still say seven; the prompt
package records that discrepancy.

## Historical artifacts

Any retained files under `dist/` are old release artifacts, not builds of this
documentation-only tree. This repository no longer provides a supported source
build or installer.
