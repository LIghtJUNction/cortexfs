use std::{fmt, time::Duration};

/// Notion database task channel configuration.
#[derive(Clone)]
pub struct NotionConfig {
    pub api_base: String,
    pub api_token: String,
    pub database_id: String,
    pub poll_seconds: u64,
    pub status_property: String,
    pub input_property: String,
    pub result_property: String,
    pub status_type: String,
    pub max_concurrent: usize,
    pub recover_stale: bool,
}

impl NotionConfig {
    pub fn new(
        api_base: impl Into<String>,
        api_token: impl Into<String>,
        database_id: impl Into<String>,
    ) -> Result<Self, super::NotionError> {
        let config = Self {
            api_base: api_base.into().trim_end_matches('/').to_owned(),
            api_token: api_token.into(),
            database_id: database_id.into(),
            poll_seconds: 5,
            status_property: "Status".to_owned(),
            input_property: "Input".to_owned(),
            result_property: "Result".to_owned(),
            status_type: "auto".to_owned(),
            max_concurrent: 1,
            recover_stale: true,
        };
        if [
            config.api_base.as_str(),
            config.api_token.as_str(),
            config.database_id.as_str(),
        ]
        .iter()
        .any(|value| value.is_empty() || value.contains('\0'))
        {
            return Err(super::NotionError::Config(
                "API base, token, and database id are required".to_owned(),
            ));
        }
        url::Url::parse(&config.api_base)
            .map_err(|error| super::NotionError::Config(error.to_string()))?;
        Ok(config)
    }

    #[must_use]
    pub fn with_properties(
        mut self,
        status: impl Into<String>,
        input: impl Into<String>,
        result: impl Into<String>,
    ) -> Self {
        self.status_property = status.into();
        self.input_property = input.into();
        self.result_property = result.into();
        self
    }

    #[must_use]
    pub fn with_status_type(mut self, value: impl Into<String>) -> Self {
        self.status_type = value.into();
        self
    }

    #[must_use]
    pub fn with_poll_seconds(mut self, value: u64) -> Self {
        self.poll_seconds = value.clamp(1, 300);
        self
    }

    #[must_use]
    pub fn with_max_concurrent(mut self, value: usize) -> Self {
        self.max_concurrent = value.clamp(1, 32);
        self
    }

    #[must_use]
    pub fn with_recover_stale(mut self, value: bool) -> Self {
        self.recover_stale = value;
        self
    }

    pub(super) fn poll_delay(&self) -> Duration {
        Duration::from_secs(self.poll_seconds)
    }
}

impl fmt::Debug for NotionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NotionConfig")
            .field("api_base", &self.api_base)
            .field("api_token", &"[redacted]")
            .field("database_id", &self.database_id)
            .field("poll_seconds", &self.poll_seconds)
            .field("status_property", &self.status_property)
            .field("input_property", &self.input_property)
            .field("result_property", &self.result_property)
            .field("status_type", &self.status_type)
            .field("max_concurrent", &self.max_concurrent)
            .field("recover_stale", &self.recover_stale)
            .finish()
    }
}
