use std::collections::HashMap;

use serde::Deserialize;

use crate::ModelContextLimit;
use crate::provider::auth::{ProviderAuthConfig, effective_auth_methods};
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
    #[serde(default)]
    pub(crate) auth: Vec<ProviderAuthConfig>,
    pub(crate) oauth: Option<OAuthProviderConfig>,
}

impl ProviderConfig {
    pub(crate) fn auth_methods(&self) -> Vec<ProviderAuthConfig> {
        effective_auth_methods(&self.auth, self.oauth.is_some())
    }
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
    pub(crate) log: String,
    pub(crate) effort: String,
    pub(crate) limit: ModelContextLimit,
    pub(crate) recommended: ModelContextLimit,
    pub(crate) compact: ModelContextLimit,
    pub(crate) metadata: String,
}

const fn default_enabled() -> bool {
    true
}
