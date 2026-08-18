use serde::{Deserialize, Serialize};

/// Token accounting reported by a provider adapter.
#[expect(
    clippy::struct_field_names,
    reason = "wire usage fields retain explicit token units"
)]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}
