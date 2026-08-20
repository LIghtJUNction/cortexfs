# cortexfs-metadatas

`cortexfs-metadatas` is the provider-neutral model metadata layer for
CortexFS. Its only upstream model catalog is
[models.dev](https://models.dev): no OpenAI, Anthropic, Google, or other
provider model table is compiled into this crate.

## Data flow

`MetadataCatalog::from_models_dev()` fetches `catalog.json`, validates the
complete response, atomically writes a bounded cache, and builds the normalized
Rust catalog. `MetadataCatalog::from_cache()` rebuilds that catalog offline from
the last valid cache; `from_cache_or_empty()` deliberately returns no model
facts when no valid cache exists.

The cache stores the raw `catalog.json` response and its HTTP observation time,
not a hand-maintained snapshot. A failed fetch, invalid response, or failed
write leaves the prior valid cache intact. Refresh is an explicit host action;
the crate has no background polling or provider-specific refresh loop.

Each serving record retains:

- `models_dev`: the exact provider-serving model object;
- `models_dev_base`: the matching provider-independent model object, including
  fields such as benchmarks and weights;
- `MetadataCatalog::provider()` and `base_model()`: provider descriptors and
  model-only records that are not part of a selected endpoint.

Provider-serving facts determine runtime normalization. Provider descriptors
remain metadata only: API keys, request headers, route policy, and live model
discovery stay in CortexFS's provider registry and secret store.

## Usage

```rust,no_run
use cortexfs_metadatas::MetadataCatalog;
use std::path::Path;

let cache = Path::new("/var/lib/cortexfs/provider-models");
let cached = MetadataCatalog::from_cache_or_empty(cache);

let refreshed = {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(MetadataCatalog::from_models_dev(cache))?
};

# Ok::<(), Box<dyn std::error::Error>>(())
```

`ModelMetadata` exposes conservative normalized limits, modalities, tool use,
reasoning, and output capabilities for `/ctx/model`. Unknown upstream facts
stay unknown. Applications may register local records and aliases explicitly,
but a provider name is never guessed from an aggregator endpoint.
