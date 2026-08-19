use serde::{Deserialize, Serialize};

/// User-selected presentation for a channel's in-progress response.
///
/// `None` values disable that effect. The ABI intentionally does not choose
/// emoji, text, or timing defaults; each host may load this policy from its
/// own configuration format.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::ChannelProgressPolicy;

    #[test]
    fn progress_policy_does_not_invent_presentation_defaults() -> Result<(), serde_json::Error> {
        let policy = serde_json::from_str::<ChannelProgressPolicy>("{}")?;
        assert_eq!(policy, ChannelProgressPolicy::default());
        Ok(())
    }
}
