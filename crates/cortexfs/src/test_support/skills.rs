use crate::CortexFs;

#[test]
fn projection_exposes_installed_skill_and_index() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    assert_eq!(
        fs.lookup_path(["skill", "registry", "count"])
            .and_then(crate::Node::content),
        Some("1\n")
    );
    assert_eq!(
        fs.lookup_path(["skill", "registry", "list"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert!(
        fs.tree
            .path_inode(&["skill", "installed", "cortexfs-test", "status"])
            .is_none(),
        "installed skill status must be runtime-owned, not a static placeholder"
    );
    assert_eq!(
        fs.node_content(fs.resolve_path_inode([
            "skill",
            "installed",
            "cortexfs-test",
            "status"
        ])?)?,
        "installed\n"
    );
    assert_eq!(
        fs.lookup_path(["skill", "installed", "cortexfs-test", "context"])
            .and_then(crate::Node::content),
        Some("local:skill_r:skill_t:s0\n")
    );
    assert!(
        fs.lookup_path(["skill", "installed", "cortexfs-test", "SKILL.md"])
            .and_then(crate::Node::content)
            .is_some_and(|skill| skill.contains("provider-neutral")),
        "installed skill body must be readable"
    );
    assert_eq!(
        fs.lookup_path(["skill", "index", "by-trigger", "fuse"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert_eq!(
        fs.lookup_path(["skill", "index", "by-domain", "cortexfs"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert!(
        fs.lookup_path(["skill", "installed", "cortexfs-test", "references"])
            .is_some(),
        "skill references directory must exist"
    );
    for directory in ["references", "scripts", "assets", "examples"] {
        assert!(
            fs.lookup_path(["skill", "installed", "cortexfs-test", directory])
                .is_some(),
            "skill progressive disclosure directory must exist: {directory}"
        );
        assert!(
            fs.lookup_path(["skill", "installed", "cortexfs-test", directory, "list"])
                .and_then(crate::Node::content)
                .is_some_and(|list| !list.trim().is_empty()),
            "skill progressive disclosure directory must expose a list file: {directory}"
        );
    }
    assert_eq!(
        fs.lookup_path(["skill", "installed", "cortexfs-test", "permissions"])
            .and_then(crate::Node::content),
        Some("provider.test\nhost.fuse.mount\n")
    );
    assert_eq!(
        fs.lookup_path(["skill", "installed", "cortexfs-test", "tool"])
            .and_then(crate::Node::content),
        Some("filesystem.read\nmcp.local-fs.read_file\n")
    );
    assert_eq!(
        fs.lookup_path(["skill", "installed", "cortexfs-test", "mcp_server"])
            .and_then(crate::Node::content),
        Some("local-fs\n")
    );
    assert_eq!(
        fs.lookup_path(["skill", "installed", "cortexfs-test", "provider"])
            .and_then(crate::Node::content),
        Some("local-runtime\n")
    );
    assert_eq!(
        fs.lookup_path(["agent", "helper", "policy", "allowed_skills"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    assert_eq!(
        fs.lookup_path(["home", "1000", "skill", "enabled"])
            .and_then(crate::Node::content),
        Some("cortexfs-test\n")
    );
    Ok(())
}

#[test]
fn installed_skill_progressive_disclosure_files_are_readable() {
    let fs = CortexFs::new();

    assert!(
        fs.lookup_path([
            "skill",
            "installed",
            "cortexfs-test",
            "references",
            "mount.md"
        ])
        .and_then(crate::Node::content)
        .is_some_and(|content| content.contains("tests/mounts/cortexfs")),
        "skill reference content must be readable on demand"
    );
    assert_eq!(
        fs.lookup_path(["skill", "installed", "cortexfs-test", "scripts", "smoke.sh"])
            .and_then(crate::Node::content),
        Some("cargo test -p cortexfs --locked clusters -- --nocapture\n")
    );
    assert!(
        fs.lookup_path([
            "skill",
            "installed",
            "cortexfs-test",
            "assets",
            "mountpoint"
        ])
        .and_then(crate::Node::content)
        .is_some_and(|content| content.contains("tests/mounts/cortexfs")),
        "skill asset content must expose concrete local mountpoint data"
    );
    assert!(
        fs.lookup_path([
            "skill",
            "installed",
            "cortexfs-test",
            "examples",
            "ollama-smollm2.req.json"
        ])
        .and_then(crate::Node::content)
        .is_some_and(|content| content.contains("smollm2:135m") && content.contains("cortexfs-ok")),
        "skill example request must be readable without loading SKILL.md"
    );
}

#[test]
fn skill_dependencies_remain_separate_from_authorization_policy() {
    let fs = CortexFs::new();
    let skill_tools = fs
        .lookup_path(["skill", "installed", "cortexfs-test", "tool"])
        .and_then(crate::Node::content)
        .unwrap_or_default();
    let user_tools = fs
        .lookup_path(["home", "1000", "tool", "enabled"])
        .and_then(crate::Node::content)
        .unwrap_or_default();
    let agent_tools = fs
        .lookup_path(["agent", "helper", "policy", "allowed_tools"])
        .and_then(crate::Node::content)
        .unwrap_or_default();

    for tool in skill_tools.lines() {
        assert!(
            user_tools.lines().any(|allowed| allowed == tool),
            "space policy must explicitly authorize skill tool dependency: {tool}"
        );
        assert!(
            agent_tools.lines().any(|allowed| allowed == tool),
            "agent policy must explicitly authorize skill tool dependency: {tool}"
        );
    }
    assert!(
        !skill_tools.lines().any(|tool| tool == "shell.exec"),
        "skill dependency declaration must not grant shell.exec implicitly"
    );
    assert!(
        !user_tools.lines().any(|tool| tool == "shell.exec"),
        "space policy must keep shell.exec disabled by default"
    );
    assert!(
        !agent_tools.lines().any(|tool| tool == "shell.exec"),
        "agent policy must keep shell.exec disabled by default"
    );
}
