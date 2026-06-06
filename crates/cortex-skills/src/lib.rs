#![forbid(unsafe_code)]

/// Workspace scope for skill registry, indexing, and loading.
pub const SCOPE: &str = "skills";

#[cfg(test)]
mod tests {
    #[test]
    fn scope_names_design_component() {
        assert_eq!(super::SCOPE, "skills");
    }
}
