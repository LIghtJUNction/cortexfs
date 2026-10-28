# CortexFS

This repository is now the documentation and ABI reference for CortexFS. The
product implementation, tests, examples, build scripts, and documentation-site
implementation were removed from the current source tree. The preserved guides
describe the `v0.1.21` source snapshot and are reference material; they are not
executable instructions for this checkout.

## Rebuild prompt

The self-contained reconstruction package is maintained in the
[`cortexfs-prompts` submodule](docs/cortexfs-prompts/README.md). Start with
[`BUILD_PROMPT.md`](docs/cortexfs-prompts/BUILD_PROMPT.md). The package records
its source baseline, evidence, known documentation conflicts, and file hashes.
It has not been validated by an independent rebuild.

## Canonical documentation

- [Normative ABI index](docs/spec/README.md)
- [Architecture and product boundaries](docs/architecture.md)
- [Documentation status and provenance](docs/README.md)
- [Visual design tokens](docs/DESIGN.md)
- [Chinese translations](docs/translations/zh-Hans/)

The detailed root ABI includes eight stable entries, including
`/ctx/channel/`. Historical marketing copy may still say seven; see the prompt
package's evidence record.

## Historical artifacts

Any retained files under `dist/` are old release artifacts, not builds of this
documentation-only tree. This repository no longer provides a supported
installer or source build.
