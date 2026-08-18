use std::{fmt, net::SocketAddr, path::PathBuf};

/// Gmail Pub/Sub push host configuration.
pub struct GmailConfig {
    pub socket: PathBuf,
    pub bind: SocketAddr,
    pub path: String,
    pub access_token: String,
    pub api_base: String,
    pub token: Option<String>,
}

impl fmt::Debug for GmailConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GmailConfig")
            .field("socket", &self.socket)
            .field("bind", &self.bind)
            .field("path", &self.path)
            .field("access_token", &"[redacted]")
            .field("api_base", &self.api_base)
            .field("token", &self.token.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}
