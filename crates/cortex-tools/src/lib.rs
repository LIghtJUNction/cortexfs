#![forbid(unsafe_code)]

/// Workspace scope for tool registry, invocation, and tool-loop primitives.
pub const SCOPE: &str = "tools";

#[cfg(test)]
mod tests {
    #[test]
    fn scope_names_design_component() {
        assert_eq!(super::SCOPE, "tools");
    }
}
