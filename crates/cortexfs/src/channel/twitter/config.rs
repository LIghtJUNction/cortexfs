use std::{fmt, time::Duration};

/// Twitter API v2 bearer token and mention routing configuration.
pub struct TwitterConfig {
    pub bearer_token: String,
    pub api_base: String,
    pub allowed_users: Vec<String>,
    pub poll_seconds: u64,
}

impl TwitterConfig {
    pub fn new(
        bearer_token: impl Into<String>,
        api_base: impl Into<String>,
    ) -> Result<Self, super::TwitterError> {
        let config = Self {
            bearer_token: bearer_token.into(),
            api_base: api_base.into().trim_end_matches('/').to_owned(),
            allowed_users: Vec::new(),
            poll_seconds: 15,
        };
        if [config.bearer_token.as_str(), config.api_base.as_str()]
            .iter()
            .any(|value| value.is_empty() || value.contains('\0'))
        {
            return Err(super::TwitterError::Config(
                "bearer token and API base are required".to_owned(),
            ));
        }
        url::Url::parse(&config.api_base)
            .map_err(|error| super::TwitterError::Config(error.to_string()))?;
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

    pub(super) fn accepts(&self, id: &str, handle: Option<&str>) -> bool {
        self.allowed_users.iter().any(|allowed| {
            allowed == "*"
                || allowed == id
                || handle.is_some_and(|handle| allowed.eq_ignore_ascii_case(handle))
        })
    }
}

impl fmt::Debug for TwitterConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TwitterConfig")
            .field("bearer_token", &"[redacted]")
            .field("api_base", &self.api_base)
            .field("allowed_users", &self.allowed_users)
            .field("poll_seconds", &self.poll_seconds)
            .finish()
    }
}
