/// `DingTalk` Stream Mode configuration.
#[derive(Clone)]
pub struct DingTalkConfig {
    pub(super) client_id: String,
    pub(super) client_secret: String,
    pub(super) gateway_url: String,
}

impl DingTalkConfig {
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        gateway_url: impl Into<String>,
    ) -> Result<Self, DingTalkError> {
        let config = Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            gateway_url: gateway_url.into(),
        };
        if [
            config.client_id.as_str(),
            config.client_secret.as_str(),
            config.gateway_url.as_str(),
        ]
        .iter()
        .any(|value| value.is_empty() || value.contains('\0'))
        {
            return Err(DingTalkError::Config(
                "client credentials and gateway URL are required".to_owned(),
            ));
        }
        Ok(config)
    }
}

impl std::fmt::Debug for DingTalkConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DingTalkConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &"[redacted]")
            .field("gateway_url", &self.gateway_url)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DingTalkError {
    #[error("invalid DingTalk configuration: {0}")]
    Config(String),
    #[error("DingTalk HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error("DingTalk JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("DingTalk WebSocket failed: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error(transparent)]
    Channel(#[from] cortexfs_channels::ChannelError),
    #[error(transparent)]
    Bridge(#[from] super::super::bridge::ChannelBridgeError),
    #[error("DingTalk gateway protocol error: {0}")]
    Protocol(String),
}
