#[test]
fn context_revision_is_stable_without_exposing_context() {
    let first = crate::runtime::observation::context_revision("history", "tools", "none");
    assert_eq!(
        first,
        crate::runtime::observation::context_revision("history", "tools", "none")
    );
    assert!(first.starts_with("sha256:"));
    assert_eq!(first.len(), 71);
    assert_ne!(
        first,
        crate::runtime::observation::context_revision("changed", "tools", "none")
    );
    assert!(!first.contains("history"));
    assert!(!first.contains("tools"));
}
