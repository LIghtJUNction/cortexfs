# cortexfs-metadatas

`cortexfs-metadatas` is a provider-neutral Rust model metadata catalog for
AI/LLM applications. It answers capability questions before an adapter sends a
request: hard context limit, recommended working context, compaction threshold,
maximum output, text/image/audio/video/PDF modalities, tool/function calling,
structured output, streaming, reasoning levels, model status, aliases, and
source provenance.

Search keywords: AI model metadata, LLM model catalog, context window,
function calling, tool use, vision model, multimodal model, reasoning effort,
OpenAI models, Gemini models, Anthropic Claude, DeepSeek, Qwen, Mistral, GLM,
Grok, CortexFS Rust.

```toml
cortexfs-metadatas = "0.1.16"
```

## Included official snapshot

The checked-in snapshot is dated `2026-08-16` (`CATALOG_DATE`) and is kept as
ordinary Rust objects, so it has no runtime network dependency. It includes
officially sourced records for OpenAI, Anthropic Claude, Google Gemini,
DeepSeek, Mistral, xAI Grok, Z.AI GLM, and Alibaba Qwen. Each record carries
one or more official documentation URLs and an explicit confidence value.

Provider facts change. Treat the snapshot as a safe default and register a
new verified record when the configured endpoint exposes newer limits or
aliases.

The refresh path validates the current `models.dev/api.json` object directly
and retains the exact official model object in `ModelMetadata::models_dev`.
The upstream schema is allowed to add optional fields or omit facts such as
`temperature`; missing facts become `unknown`, never an invented claim. The
raw object is deliberately kept so newly added upstream fields are not
silently lost while normalized fields remain stable for Rust callers.

## Capabilities and aliases

```rust
use cortexfs_metadatas::{MetadataCatalog, Modality, Support};

let catalog = MetadataCatalog::builtins();
let model = catalog.resolve("gpt-5.6").ok_or("unknown model")?;
assert!(model.supports_input(Modality::Image));
assert_eq!(model.tools, Support::Supported);
assert_eq!(catalog.canonical_key("gpt-5.6"), Some("openai/gpt-5.6-sol"));
let policy = model.context_policy();
assert!(policy.max_tokens >= policy.recommended_tokens);
assert!(policy.compaction_threshold_tokens <= policy.recommended_tokens);
```

Aliases are many-to-one. Canonical references use `provider/model`; provider
scoped aliases remain unambiguous when different providers share a short name.
One model can have multiple snapshot aliases and applications can add more.

## Custom registration and mapping

```rust
use cortexfs_metadatas::{MetadataCatalog, ModelMetadata};

let mut catalog = MetadataCatalog::new();
catalog.register(
    ModelMetadata::new("my-gateway", "reasoner-v2", "Reasoner V2")
        .with_aliases(["latest", "production"])
        .with_context(131_072),
)?;
catalog.register_alias("stable", "my-gateway/reasoner-v2")?;
assert_eq!(catalog.resolve("production").map(|m| m.id.as_str()), Some("reasoner-v2"));
```

`register`, `register_alias`, and `register_provider_alias` validate empty
names, duplicate canonical models, unknown targets, and alias conflicts. A
failed model registration is transactional: it cannot leave half of its
aliases in the catalog.

For models whose official maximum is much larger than the useful default,
`with_context` derives a conservative policy (131072 recommended tokens and an
80% compaction trigger by default). A verified record can override those
values:

```rust
let model = ModelMetadata::new("gateway", "long", "Long Context")
    .with_context(1_000_000)
    .with_context_policy(262_144, 209_715);
assert_eq!(model.context_policy().recommended_tokens, Some(262_144));
```

## API shape

- `ModelMetadata`: canonical identity, limits, modalities, capabilities,
  reasoning controls, descriptive model facts, aliases, lifecycle status,
  sources, context policy, and the complete optional `models_dev` payload.
- `MetadataCatalog`: deterministic registry with `resolve`, `resolve_for`,
  `canonical_key`, iteration, and custom mapping APIs.
- `Support`: `supported`, `unsupported`, or `unknown`; unknown is intentional
  and safer than claiming a provider feature.
- `ReasoningMetadata`: normalized levels plus provider parameter name, default,
  and optional token budget.
- `MetadataSource` / `SourceConfidence`: provenance suitable for audits and
  downstream UI explanations.

The crate exposes cache-backed loading for downstream crates.

- `MetadataSourceError`: fetch and cache load failures while resolving metadata.
- Use `from_cache_or_builtins` for deterministic bootstrap with offline fallback.
- Use `from_models_dev` for a remote refresh path that publishes `model-metadata.json` cache.

```rust
use cortexfs_metadatas::MetadataCatalog;
use std::path::Path;

let cache = Path::new("/var/lib/cortexfs/provider-models");

let local = MetadataCatalog::from_cache_or_builtins(cache);

let refreshed = {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime init failed");
    rt.block_on(MetadataCatalog::from_models_dev(cache)).expect("metadata refresh failed")
};
```

The crate keeps a checked-in Rust snapshot for offline and invalid-cache
fallback paths. A verified `models.dev` record is authoritative for the
normalized facts it supplies; an omitted upstream fact remains `unknown`
instead of inheriting a stale built-in claim. Non-authoritative custom
overlays remain conservative, and aliases can map many provider model IDs to
one canonical record.

## Tests and publishing

The current integration suite reports **9 passed, 0 failed** and covers the
official catalog, aliases, custom registration, provider-scoped resolution,
unknown-model errors, optional upstream facts, zero limits, and transactional
conflict handling.

```text
TMPDIR=/var/tmp cargo test --locked -p cortexfs-metadatas
TMPDIR=/var/tmp cargo clippy --locked -p cortexfs-metadatas --all-targets -- -D warnings
cargo package --locked -p cortexfs-metadatas --allow-dirty
```

The crate version follows the CortexFS workspace version (`0.1.16`).
