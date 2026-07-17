# Verus Proofs

CortexFS keeps Verus proofs outside the runtime Cargo workspace. Verus is a
static verifier distributed as a separate `verus` binary, so normal `cargo
test` and `cargo build` do not depend on it.

Run the proof suite with:

```sh
scripts/verify-verus.sh
```

This harness was checked with upstream Verus release
`release/0.2026.06.20.911e4e7`.

## Current coverage

The current proof target is `proofs/verus/abi_name.rs`. It defines the
standalone `is_valid_object_name` specification predicate for the stable 64-byte
object-name rule from `docs/spec/object-abi.md`:

```text
[a-zA-Z0-9][a-zA-Z0-9._+-]{0,63}
```

In addition to the grammar, the predicate rejects the reserved `.sock` and
`.d` suffixes. Its proof functions establish these safety facts for names that
the standalone predicate accepts:

```text
the name is non-empty
the name is at most 64 bytes
the first byte is ASCII alphanumeric
all bytes are ASCII path-component bytes
NUL, newline, and slash cannot appear
.sock and .d control suffixes are rejected
```

The executable `is_object_name` implementation lives in
`crates/cortexfs/src/abi/path.rs`; its 64-byte limit is
`MAX_OBJECT_NAME_LEN` in `crates/cortexfs/src/abi/constants.rs`. The Verus
predicate currently mirrors that logic manually. No proof connects the
standalone specification to the executable Rust implementation.

The proof is checked only when `scripts/verify-verus.sh` runs successfully
with a compatible `verus` binary on `PATH`. Normal Cargo commands do not check
it, and repository CI does not currently invoke the script.

## Upgrade boundary

The following remain future verification work, not current proof coverage:

- equivalence between `is_valid_object_name` and executable Rust
  `is_object_name`
- provider/model composition in `is_model_name`
- the canonical aliases accepted by `is_model_reference`
- class-dependent validation in `is_object_name_for_class`
- the SDK-local predicate in `crates/cortexfs-agent-sdk/src/lib.rs`, which
  currently uses a 255-byte limit rather than the core ABI's 64-byte limit
- CI enforcement of the standalone Verus harness

Keep the predicate, implementation, and normative ABI under explicit review
when the object-name rule changes. The SDK limit mismatch is unresolved until
the executable predicates are aligned or their difference is specified.
