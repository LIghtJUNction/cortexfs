# cortexfs-metadatas

`cortexfs-metadatas` is a provider-neutral Rust model metadata catalog for
AI/LLM applications. It answers capability questions before an adapter sends a
request: context window, maximum output, text/image/audio/video/PDF input,
tool/function calling, structured output, streaming, reasoning levels, model
status, aliases, and source provenance.

Search keywords: AI model metadata, LLM model catalog, context window,
function calling, tool use, vision model, multimodal model, reasoning effort,
OpenAI models, Gemini models, Anthropic Claude, DeepSeek, Qwen, Mistral, GLM,
Grok, CortexFS Rust.

```toml
cortexfs-metadatas = "0.1.7"
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

## Capabilities and aliases

```rust
use cortexfs_metadatas::{MetadataCatalog, Modality, Support};

let catalog = MetadataCatalog::builtins();
let model = catalog.resolve("gpt-5.6").ok_or("unknown model")?;
assert!(model.supports_input(Modality::Image));
assert_eq!(model.tools, Support::Supported);
assert_eq!(catalog.canonical_key("gpt-5.6"), Some("openai/gpt-5.6-sol"));
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

## API shape

- `ModelMetadata`: canonical identity, limits, modalities, capabilities,
  reasoning controls, aliases, lifecycle status, and sources.
- `MetadataCatalog`: deterministic registry with `resolve`, `resolve_for`,
  `canonical_key`, iteration, and custom mapping APIs.
- `Support`: `supported`, `unsupported`, or `unknown`; unknown is intentional
  and safer than claiming a provider feature.
- `ReasoningMetadata`: normalized levels plus provider parameter name, default,
  and optional token budget.
- `MetadataSource` / `SourceConfidence`: provenance suitable for audits and
  downstream UI explanations.

The crate contains no HTTP client, credentials, background refresh, provider
special case in the public lookup API, or unsafe code.

## Tests and publishing

The current integration suite reports **3 passed, 0 failed** and covers the
official catalog, aliases, custom registration, provider-scoped resolution,
unknown-model errors, and transactional conflict handling.

```text
TMPDIR=/tmp cargo test --locked -p cortexfs-metadatas
TMPDIR=/tmp cargo clippy --locked -p cortexfs-metadatas --all-targets -- -D warnings
cargo package --locked -p cortexfs-metadatas --allow-dirty
```

The crate version follows the CortexFS workspace version (`0.1.7`).
