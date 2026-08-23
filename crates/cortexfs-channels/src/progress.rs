use serde::{Deserialize, Serialize};

/// Default presentation for a channel's in-progress response.
///
/// Hosts may override these values or use `None` to disable an effect.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelProgressPolicy {
    pub reaction: Option<String>,
    pub error_reaction: Option<String>,
    pub placeholder: Option<String>,
    pub error_prefix: Option<String>,
    pub typing: bool,
    pub edit_interval_ms: Option<u64>,
    pub edit_chunk_bytes: Option<usize>,
}

impl Default for ChannelProgressPolicy {
    fn default() -> Self {
        Self {
            reaction: Some("👀".to_owned()),
            error_reaction: Some("❌".to_owned()),
            placeholder: Some("⏳ 思考中…".to_owned()),
            error_prefix: Some("⚠️ ".to_owned()),
            typing: true,
            edit_interval_ms: Some(700),
            edit_chunk_bytes: Some(512),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ChannelProgressPolicy;

    #[test]
    fn progress_policy_defaults_to_enabled_presentation() -> Result<(), serde_json::Error> {
        let policy = serde_json::from_str::<ChannelProgressPolicy>("{}")?;
        assert_eq!(policy, ChannelProgressPolicy::default());
        Ok(())
    }
}
