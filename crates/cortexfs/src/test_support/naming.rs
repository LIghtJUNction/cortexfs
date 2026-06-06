use crate::{CortexFs, ROOT_INODE};
use std::ffi::OsStr;

const ROOT_ABI_NAMES: &[&str] = &[
    "status",
    "cap",
    "api",
    "format",
    "provider",
    "model",
    "home",
    "group",
    "shared",
    "ext",
    "space",
    "agent",
    "cluster",
    "mcp",
    "skill",
    "tool",
    "memory",
    "vector",
    "db",
    "audit",
    "control",
    "capabilities",
    "formats",
    "providers",
    "models",
    "spaces",
    "agents",
    "clusters",
    "skills",
    "tools",
    "databases",
];

const FORBIDDEN_ROOT_ALIASES: &[&str] = &[
    "ctx_home",
    "current_user",
    "user_home",
    "my",
    "me",
    "default",
];

#[test]
fn root_names_are_plain_abi_entries_without_helper_aliases() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let root = fs
        .tree
        .nodes
        .get(&ROOT_INODE)
        .ok_or_else(fuse3::Errno::new_not_exist)?;

    let names = root
        .children()
        .iter()
        .filter_map(|inode| fs.tree.nodes.get(inode))
        .map(crate::Node::name)
        .collect::<Vec<_>>();

    assert_eq!(names, ROOT_ABI_NAMES);
    for alias in FORBIDDEN_ROOT_ALIASES {
        assert!(
            fs.lookup_child(ROOT_INODE, OsStr::new(alias)).is_err(),
            "root ABI must not expose convenience alias {alias}"
        );
    }
    Ok(())
}

#[test]
fn short_root_names_are_primary_and_plural_names_are_compat() -> fuse3::Result<()> {
    let fs = CortexFs::new();

    for (primary, compat) in [
        ("cap", "capabilities"),
        ("format", "formats"),
        ("provider", "providers"),
        ("model", "models"),
        ("space", "spaces"),
        ("agent", "agents"),
        ("cluster", "clusters"),
        ("skill", "skills"),
        ("tool", "tools"),
        ("db", "databases"),
    ] {
        assert!(
            fs.lookup_child(ROOT_INODE, OsStr::new(primary)).is_ok(),
            "missing primary root ABI entry {primary}"
        );
        assert!(
            fs.lookup_child(ROOT_INODE, OsStr::new(compat)).is_ok(),
            "missing compatibility root ABI entry {compat}"
        );
    }

    let provider = crate::default_provider_id();
    let primary = fs.path_inode(["provider", provider])?;
    let compat = fs.path_inode(["providers", provider])?;
    assert_eq!(
        primary, compat,
        "provider compat path must point at the same inode"
    );
    Ok(())
}

#[test]
fn home_directory_uses_uid_entries_without_index_files() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let home = fs.path_inode(["home"])?;
    let home_node = fs
        .tree
        .nodes
        .get(&home)
        .ok_or_else(fuse3::Errno::new_not_exist)?;
    let names = home_node
        .children()
        .iter()
        .filter_map(|inode| fs.tree.nodes.get(inode))
        .map(crate::Node::name)
        .collect::<Vec<_>>();

    assert_eq!(names, [crate::LOCAL_USER_ID]);
    for helper in ["count", "list", "current", "path", "default"] {
        assert!(
            fs.lookup_path(["home", helper]).is_none(),
            "home must look like a uid directory, not a control surface"
        );
    }
    Ok(())
}

#[test]
fn home_mcp_indexes_use_directories_not_flat_underscore_names() {
    let fs = CortexFs::new();

    assert!(
        fs.lookup_path(["home", "1000", "mcp", "servers", "count"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["home", "1000", "mcp", "servers", "list"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["home", "1000", "mcp", "tools", "count"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["home", "1000", "mcp", "tools", "list"])
            .is_some()
    );
}

#[test]
fn provider_config_uses_short_url_directory() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::default_provider_id();
    let url = fs.path_inode(["provider", provider, "url"])?;

    assert!(fs.lookup_path(["provider", provider, "url"]).is_some());
    assert!(fs.lookup_path(["providers", provider, "url"]).is_some());
    assert!(
        fs.lookup_path(["provider", provider, "url", "default"])
            .is_some()
    );
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime.lookup_child(url, "current").is_some(),
        "runtime provider config must attach to provider/<id>/url"
    );
    drop(runtime);
    Ok(())
}

#[test]
fn agent_profile_uses_model_directory_for_default_selection() {
    let fs = CortexFs::new();

    assert!(
        fs.lookup_path(["agent", "helper", "profile", "model", "provider"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["agent", "helper", "profile", "model", "model"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["agent", "helper", "profile", "model", "format"])
            .is_some()
    );
}

#[test]
fn agent_mcp_indexes_use_directories_not_flat_underscore_names() {
    let fs = CortexFs::new();

    assert!(
        fs.lookup_path(["agent", "helper", "mcp", "servers", "count"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["agent", "helper", "mcp", "servers", "list"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["agent", "helper", "mcp", "servers", "enabled"])
            .is_some()
    );
}

#[test]
fn mcp_indexes_use_singular_primary_directories() {
    let fs = CortexFs::new();

    for (primary, compat) in [
        ("server", "servers"),
        ("tool", "tools"),
        ("resource", "resources"),
        ("prompt", "prompts"),
        ("session", "sessions"),
    ] {
        assert!(
            fs.lookup_path(["mcp", primary, "list"]).is_some(),
            "mcp/{primary} must be the primary registry"
        );
        assert!(
            fs.lookup_path(["mcp", compat, "list"]).is_some(),
            "mcp/{compat} remains a compatibility registry"
        );
    }
}
