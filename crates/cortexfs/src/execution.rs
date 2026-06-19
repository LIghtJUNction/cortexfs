#[cfg(not(test))]
use crate::provider_registry::{ProviderRegistry, SecretStore};
#[cfg(test)]
use crate::{
    PROVIDER_SPECS, in_memory_execution_provider_spec, provider_chat_response,
    provider_format_response, provider_response_for_format,
};
#[cfg(test)]
use cortex_core::{ApiFormat, ModelId, ProviderId};
#[cfg(not(test))]
use cortex_core::{ApiFormat, ModelId, ProviderId};
use cortex_providers::Provider;
#[cfg(test)]
use cortex_providers::{InMemoryProvider, ProviderModel, ProviderResponse};
#[cfg(not(test))]
use cortex_providers::{OpenAiCompatibleProvider, ProviderResponse};
#[cfg(not(test))]
use cortex_providers::{ProviderError, ProviderHealth, ProviderModel, ProviderRequest};
use cortex_store::InMemoryStore;
use cortexd::ExecutionPlane;
#[cfg(test)]
use std::str::FromStr;
#[cfg(not(test))]
use std::{collections::BTreeMap, str::FromStr as _};

pub type FsExecutionPlane = ExecutionPlane<InMemoryStore, Box<dyn Provider + Send + Sync>>;

#[cfg(not(test))]
pub fn default_execution_plane() -> Option<FsExecutionPlane> {
    if let Some(provider) = RegistryProviderRouter::from_registry() {
        return Some(ExecutionPlane::new(
            InMemoryStore::new(),
            Box::new(provider),
        ));
    }
    let provider = OpenAiCompatibleProvider::from_env().ok().flatten()?;
    Some(ExecutionPlane::new(
        InMemoryStore::new(),
        Box::new(provider),
    ))
}

#[cfg(test)]
pub fn default_execution_plane() -> Option<FsExecutionPlane> {
    let provider_spec = in_memory_execution_provider_spec()?;
    let provider_id = ProviderId::new(provider_spec.id).ok()?;
    let mut models = Vec::new();
    for provider in PROVIDER_SPECS {
        for format in provider.formats {
            let format = ApiFormat::from_str(format).ok()?;
            if let Ok(model) = ModelId::new(provider.default_model) {
                models.push(ProviderModel::new(model, format));
            }
        }
    }
    let mut provider = InMemoryProvider::new(
        provider_id,
        vec![
            ApiFormat::OpenAiChat,
            ApiFormat::OpenAiResponses,
            ApiFormat::AnthropicMessages,
            ApiFormat::GoogleGenerateContent,
        ],
    )
    .with_models(models)
    .with_response(
        ApiFormat::OpenAiChat,
        ProviderResponse::new(
            ApiFormat::OpenAiChat,
            provider_chat_response(provider_spec.id, provider_spec.default_model),
        ),
    )
    .with_response(
        ApiFormat::OpenAiResponses,
        ProviderResponse::new(
            ApiFormat::OpenAiResponses,
            provider_format_response(provider_spec.id, "openai.responses"),
        ),
    )
    .with_response(
        ApiFormat::AnthropicMessages,
        ProviderResponse::new(
            ApiFormat::AnthropicMessages,
            provider_format_response(provider_spec.id, "anthropic.messages"),
        ),
    )
    .with_response(
        ApiFormat::GoogleGenerateContent,
        ProviderResponse::new(
            ApiFormat::GoogleGenerateContent,
            provider_format_response(provider_spec.id, "google.generate_content"),
        ),
    );
    for routed_provider in PROVIDER_SPECS {
        let provider_id = ProviderId::new(routed_provider.id).ok()?;
        for format in routed_provider.formats {
            let api_format = ApiFormat::from_str(format).ok()?;
            let body = provider_response_for_format(routed_provider, format);
            provider = provider.with_provider_response(
                provider_id.clone(),
                api_format,
                ProviderResponse::new(api_format, body),
            );
        }
    }
    Some(ExecutionPlane::new(
        InMemoryStore::new(),
        Box::new(provider),
    ))
}

#[cfg(not(test))]
#[derive(Debug)]
struct RegistryProviderRouter {
    id: ProviderId,
    formats: Vec<ApiFormat>,
    providers: BTreeMap<ProviderId, OpenAiCompatibleProvider>,
}

#[cfg(not(test))]
impl RegistryProviderRouter {
    fn from_registry() -> Option<Self> {
        let registry = ProviderRegistry::from_env()?;
        let mut providers = BTreeMap::new();
        let mut formats = Vec::new();
        for provider in registry.load() {
            if !provider.enabled || provider.family != "openai-compatible" {
                continue;
            }
            let Ok(api_key) = SecretStore::lookup_provider_key(&provider.id) else {
                continue;
            };
            let provider_id = ProviderId::new(provider.id).ok()?;
            let model = ModelId::new(provider.default_model).ok()?;
            let provider_formats = provider
                .formats
                .iter()
                .filter_map(|format| ApiFormat::from_str(format).ok())
                .collect::<Vec<_>>();
            for format in &provider_formats {
                if !formats.contains(format) {
                    formats.push(*format);
                }
            }
            let adapter = OpenAiCompatibleProvider::new(
                provider_id.clone(),
                provider.base_url,
                api_key,
                model,
            )
            .with_formats(provider_formats);
            providers.insert(provider_id, adapter);
        }
        let id = providers.keys().next().cloned()?;
        Some(Self {
            id,
            formats,
            providers,
        })
    }
}

#[cfg(not(test))]
impl Provider for RegistryProviderRouter {
    fn id(&self) -> &ProviderId {
        &self.id
    }

    fn formats(&self) -> &[ApiFormat] {
        &self.formats
    }

    fn health(&self) -> ProviderHealth {
        self.providers
            .get(&self.id)
            .map_or_else(ProviderHealth::healthy, Provider::health)
    }

    fn models(&self) -> Vec<ProviderModel> {
        self.providers
            .values()
            .flat_map(Provider::models)
            .collect::<Vec<_>>()
    }

    fn call(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        let provider = request.provider().unwrap_or(&self.id);
        let Some(adapter) = self.providers.get(provider) else {
            return Err(ProviderError::Transport(format!(
                "unknown registry provider: {provider}"
            )));
        };
        adapter.call(request)
    }
}
