use std::{fmt, time::Duration};

/// Reddit OAuth inbox configuration; secrets stay outside filesystem state.
pub struct RedditConfig {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub username: String,
    pub subreddits: Vec<String>,
    pub api_base: String,
    pub token_url: String,
    pub poll_seconds: u64,
}

impl RedditConfig {
    pub(super) fn poll_delay(&self) -> Duration {
        Duration::from_secs(self.poll_seconds.clamp(1, 300))
    }

    pub(super) fn accepts(&self, subreddit: Option<&String>) -> bool {
        self.subreddits.is_empty()
            || subreddit.is_some_and(|value| {
                self.subreddits
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(value))
            })
    }
}

impl fmt::Debug for RedditConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedditConfig")
            .field("client_id", &"[redacted]")
            .field("client_secret", &"[redacted]")
            .field("refresh_token", &"[redacted]")
            .field("username", &self.username)
            .field("subreddits", &self.subreddits)
            .field("api_base", &self.api_base)
            .field("token_url", &self.token_url)
            .field("poll_seconds", &self.poll_seconds)
            .finish()
    }
}
