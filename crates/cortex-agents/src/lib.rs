#![forbid(unsafe_code)]

/// Workspace scope for agent runtime, collaboration, and cluster primitives.
pub const SCOPE: &str = "agents";

#[cfg(test)]
mod tests {
    #[test]
    fn scope_names_design_component() {
        assert_eq!(super::SCOPE, "agents");
    }
}
