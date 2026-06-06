#![forbid(unsafe_code)]

/// Workspace scope for memory, vector, and database integration.
pub const SCOPE: &str = "memory";

#[cfg(test)]
mod tests {
    #[test]
    fn scope_names_design_component() {
        assert_eq!(super::SCOPE, "memory");
    }
}
