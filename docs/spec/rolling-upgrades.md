# Rolling Reference-Tree Updates

CortexFS maintains one stable reference tree. Implementations MUST NOT create
parallel `v1`, `v2`, or phase-specific roots. Wire protocols and manifest
schemas may remain independently versioned; their versions do not select a
reference-tree layout.

## Version and state

`REFERENCE_TREE_VERSION` is the target tree version installed by the running
binary. The durable state file is:

```text
bin/cortexfs.bootstrap.json
```

The state records its own schema number, the applied `tree_version`, the managed
reference agents, and migration identifiers as audit evidence. The state-file
schema is independent from the reference-tree version. Migration selection is
authoritative from `tree_version`, never from the audit list.

## Migration rules

The implementation keeps an ordered registry of migrations, each with a target
tree version and a stable audit identifier. An update MUST:

1. Read the existing state and reject a `tree_version` newer than the running
   binary before mutating the tree.
2. Select every registered migration where the current version is lower than
   its target version and its target is not newer than `REFERENCE_TREE_VERSION`.
3. Process selected migrations in ascending target-version order.
4. Reconcile the complete current reference-tree shape idempotently.
5. Write the new bootstrap state last, only after reconciliation succeeds.

The state writer rebuilds the audit list deterministically from the registered
migrations through the target version; unknown or duplicate historical audit
entries do not grant authority and are not carried forward. Implementations
MUST NOT reuse an identifier for different work or report a target version
before its migrations and reconciliation have completed.

## Command modes

`ctx bootstrap` has three modes over the same plan:

```text
--check     inspect and report without writing
--dry-run   print the ordered actions without writing
default     apply the actions and record state last
```

Check and dry-run MUST NOT mutate the source tree.
For a newer on-disk version they report an explicit downgrade rejection rather
than a state write.

## Storage generations

`ctx storage update` stages a new generation by cloning the selected current
generation and applying the same rolling update. After bootstrap has written
state last, storage validates the staged generation, then atomically switches
`storage/current` to it. Direct `ctx bootstrap` reconciles but does not perform
this storage-generation validation phase. A failed stage or validation MUST
leave `storage/current` unchanged.

Generation directories are deployment snapshots named from the current tree
version. They are not separate ABI generations and do not create versioned
roots.

## Independently versioned formats

Rolling reference-tree updates do not remove versioning from wire or data
formats. Identifiers such as `cortexfs.object/v1`, agent invocation envelopes,
receipts, provider HTTP paths, and model-limit schemas keep their own explicit
compatibility rules.
