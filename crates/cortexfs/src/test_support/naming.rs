use crate::{CortexFs, ROOT_INODE};
use std::ffi::OsStr;

const ROOT_ABI_NAMES: &[&str] = &[
    "status",
    "capabilities",
    "api",
    "formats",
    "providers",
    "models",
    "home",
    "spaces",
    "agents",
    "clusters",
    "mcp",
    "skills",
    "tools",
    "memory",
    "vector",
    "databases",
    "audit",
    "control",
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
    let url = fs.path_inode(["providers", provider, "url"])?;

    assert!(fs.lookup_path(["providers", provider, "url"]).is_some());
    assert!(
        fs.lookup_path(["providers", provider, "url", "default"])
            .is_some()
    );
    let runtime = fs.runtime.lock().map_err(|_error| libc::EIO)?;
    assert!(
        runtime.lookup_child(url, "current").is_some(),
        "runtime provider config must attach to providers/<id>/url"
    );
    drop(runtime);
    Ok(())
}

#[test]
fn agent_profile_uses_model_directory_for_default_selection() {
    let fs = CortexFs::new();

    assert!(
        fs.lookup_path(["agents", "helper", "profile", "model", "provider"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["agents", "helper", "profile", "model", "model"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["agents", "helper", "profile", "model", "format"])
            .is_some()
    );
}

#[test]
fn agent_mcp_indexes_use_directories_not_flat_underscore_names() {
    let fs = CortexFs::new();

    assert!(
        fs.lookup_path(["agents", "helper", "mcp", "servers", "count"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["agents", "helper", "mcp", "servers", "list"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["agents", "helper", "mcp", "servers", "enabled"])
            .is_some()
    );
}
