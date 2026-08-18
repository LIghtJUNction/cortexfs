use std::path::Path;

use serde::Deserialize;

use crate::is_object_name;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Locator {
    transport: String,
    config: String,
    sha256: String,
    server: String,
    tool: String,
}

pub(super) fn validate_locator(content: &str) -> bool {
    serde_json::from_str::<Locator>(content).is_ok_and(|locator| {
        locator.transport == "stdio"
            && Path::new(&locator.config).is_absolute()
            && locator.sha256.len() == 64
            && locator
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            && is_object_name(&locator.server)
            && is_object_name(&locator.tool)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locator_is_strict_and_absolute() {
        assert!(validate_locator(
            r#"{"transport":"stdio","config":"/visible/mcp.json","sha256":"0000000000000000000000000000000000000000000000000000000000000000","server":"github","tool":"search"}"#
        ));
        assert!(!validate_locator(
            r#"{"transport":"http","config":"/x","server":"s","tool":"t"}"#
        ));
        assert!(!validate_locator(
            r#"{"transport":"stdio","config":"relative","server":"s","tool":"t"}"#
        ));
        assert!(!validate_locator(
            r#"{"transport":"stdio","config":"/x","server":"s","tool":"t","extra":1}"#
        ));
    }
}
