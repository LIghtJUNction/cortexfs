use std::collections::HashMap;

use serde::Deserialize;

use crate::ModelContextLimit;
use crate::provider::oauth::OAuthProviderConfig;

#[derive(Clone, Debug, Deserialize)]
pub struct ProviderConfig {
    pub(crate) name: Option<String>,
    pub(crate) base_url: String,
    pub(crate) default_model: Option<String>,
    #[serde(default)]
    pub(crate) models: Vec<String>,
    #[serde(default)]
    pub(crate) model_limits: HashMap<String, u32>,
    #[serde(default)]
    pub(crate) model_capabilities: HashMap<String, Vec<String>>,
    #[serde(default = "default_enabled")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) formats: Vec<String>,
    pub(crate) oauth: Option<OAuthProviderConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProviderModelCache {
    #[serde(default)]
    pub(crate) models: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectedProviderModel {
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) base_url: String,
    pub(crate) driver: String,
    pub(crate) cap: String,
    pub(crate) effort: String,
    pub(crate) fallback: String,
    pub(crate) limit: ModelContextLimit,
}

const fn default_enabled() -> bool {
    true
}
