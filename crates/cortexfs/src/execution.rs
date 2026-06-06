use crate::{
    PROVIDER_SPECS, in_memory_execution_provider_spec, provider_chat_response,
    provider_format_response, provider_response_for_format,
};
use cortex_core::{ApiFormat, ModelId, ProviderId};
use cortex_providers::{InMemoryProvider, ProviderModel, ProviderResponse};
use cortex_store::InMemoryStore;
use cortexd::ExecutionPlane;
use std::str::FromStr;

pub fn default_execution_plane() -> Option<ExecutionPlane<InMemoryStore, InMemoryProvider>> {
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
    Some(ExecutionPlane::new(InMemoryStore::new(), provider))
}
