#![expect(
    unreachable_pub,
    reason = "private provider submodules expose items only through crate-visible reexports"
)]

mod alias;
pub mod auth;
pub mod catalog;
mod config;
pub mod discovery;
mod link;
pub mod model;
pub mod name;
pub mod oauth;
mod project;
mod snapshot;

use serde_json::Value;

pub(crate) use alias::{current_model_alias_target, is_current_model_alias_target};
pub(crate) use config::{ProjectedProviderModel, ProviderConfig, ProviderModelCache};
pub(crate) use link::{remove_alias, replace_alias};
pub(crate) use project::projected_control_content;
#[cfg(test)]
pub(crate) use snapshot::set_load_hook;
pub(crate) use snapshot::{ProviderError, ProviderSnapshot};

pub(crate) fn openai_response_item_requires_continuation(value: &Value) -> bool {
    matches!(
        value.get("type").and_then(Value::as_str),
        Some("program" | "program_output")
    ) || value.get("type").and_then(Value::as_str) == Some("function_call")
        && value.get("caller").is_some_and(|caller| !caller.is_null())
}

pub(crate) fn effective_base_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.rsplit('/').next() == Some("v1") {
        base.to_owned()
    } else {
        format!("{base}/v1")
    }
}
