#![forbid(unsafe_code)]

/// Workspace scope for MCP client/server/session management.
pub const SCOPE: &str = "mcp";

#[cfg(test)]
mod tests {
    #[test]
    fn scope_names_design_component() {
        assert_eq!(super::SCOPE, "mcp");
    }
}
