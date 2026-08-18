use std::{fmt, time::Duration};

/// Mochat HTTP API and sender allowlist configuration.
pub struct MochatConfig {
    pub api_base: String,
    pub api_token: String,
    pub allowed_users: Vec<String>,
    pub poll_seconds: u64,
}

impl MochatConfig {
    pub fn new(
        api_base: impl Into<String>,
        api_token: impl Into<String>,
    ) -> Result<Self, super::MochatError> {
        let config = Self {
            api_base: api_base.into().trim_end_matches('/').to_owned(),
            api_token: api_token.into(),
            allowed_users: Vec::new(),
            poll_seconds: 5,
        };
        if [config.api_base.as_str(), config.api_token.as_str()]
            .iter()
            .any(|value| value.is_empty() || value.contains('\0'))
        {
            return Err(super::MochatError::Config(
                "API base and token are required".to_owned(),
            ));
        }
        url::Url::parse(&config.api_base)
            .map_err(|error| super::MochatError::Config(error.to_string()))?;
        Ok(config)
    }

    #[must_use]
    pub fn with_allowed_users(mut self, users: Vec<String>) -> Self {
        self.allowed_users = users;
        self
    }

    #[must_use]
    pub fn with_poll_seconds(mut self, seconds: u64) -> Self {
        self.poll_seconds = seconds.clamp(1, 300);
        self
    }

    pub(super) fn poll_delay(&self) -> Duration {
        Duration::from_secs(self.poll_seconds)
    }

    pub(super) fn accepts(&self, sender: &str) -> bool {
        self.allowed_users
            .iter()
            .any(|allowed| allowed == "*" || allowed == sender)
    }
}

impl fmt::Debug for MochatConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MochatConfig")
            .field("api_base", &self.api_base)
            .field("api_token", &"[redacted]")
            .field("allowed_users", &self.allowed_users)
            .field("poll_seconds", &self.poll_seconds)
            .finish()
    }
}
