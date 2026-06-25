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

The first proof target is `proofs/verus/abi_name.rs`. It mirrors the v1 object
name rule from `docs/spec/object-abi.md`:

```text
[a-zA-Z0-9][a-zA-Z0-9._+-]{0,63}
```

It proves these ABI safety facts for accepted names:

```text
the name is non-empty
the name is at most 64 bytes
the first byte is ASCII alphanumeric
all bytes are ASCII path-component bytes
NUL, newline, and slash cannot appear
.sock and .d control suffixes are rejected
```

The Rust implementation lives in `crates/cortexfs/src/lib.rs` as
`is_object_name`. Keep the Verus predicate and the Rust predicate in sync when
the object-name ABI changes.
