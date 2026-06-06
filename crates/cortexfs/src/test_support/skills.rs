use crate::CortexFs;

#[test]
fn projection_exposes_installed_skill_and_indexes() {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["skills", "registry", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["skills", "registry", "list"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert_eq!(
        fs.lookup_path(["skills", "installed", "cortexfs-test", "status"])
            .and_then(crate::Node::content),
        Some("installed\n")
    );
    assert_eq!(
        fs.lookup_path(["skills", "installed", "cortexfs-test", "context"])
            .and_then(crate::Node::content),
        Some("local:skill_r:skill_t:s0\n")
    );
    assert!(
        fs.lookup_path(["skills", "installed", "cortexfs-test", "SKILL.md"])
            .and_then(crate::Node::content)
            .is_some_and(|skill| skill.contains("provider-neutral")),
        "installed skill body must be readable"
    );
    assert_eq!(
        fs.lookup_path(["skills", "indexes", "by-trigger", "fuse"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert_eq!(
        fs.lookup_path(["skills", "indexes", "by-domain", "cortexfs"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert!(
        fs.lookup_path(["skills", "installed", "cortexfs-test", "references"])
            .is_some(),
        "skill references directory must exist"
    );
    for directory in ["scripts", "assets", "examples"] {
        assert!(
            fs.lookup_path(["skills", "installed", "cortexfs-test", directory])
                .is_some(),
            "skill progressive disclosure directory must exist: {directory}"
        );
    }
    assert_eq!(
        fs.lookup_path(["skills", "installed", "cortexfs-test", "permissions"])
            .and_then(crate::Node::content),
        Some("provider.test\nhost.fuse.mount\n")
    );
    assert_eq!(
        fs.lookup_path(["agents", "helper", "policy", "allowed_skills"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert_eq!(
        fs.lookup_path(["spaces", "users", "1000", "skills", "enabled"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
}
