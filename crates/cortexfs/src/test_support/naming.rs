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
        fs.lookup_path(["home", "1000", "mcp", "server", "count"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["home", "1000", "mcp", "server", "list"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["home", "1000", "mcp", "servers", "list"])
            .is_some(),
        "home/<uid>/mcp/servers remains a compatibility namespace"
    );
    assert!(
        fs.lookup_path(["home", "1000", "mcp", "tool", "count"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["home", "1000", "mcp", "tool", "list"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["home", "1000", "mcp", "tools", "list"])
            .is_some(),
        "home/<uid>/mcp/tools remains a compatibility namespace"
    );
    assert!(
        fs.lookup_path(["home", "1000", "mcp", "servers_count"])
            .is_none()
    );
    assert!(
        fs.lookup_path(["home", "1000", "mcp", "tools_count"])
            .is_none()
    );
}

#[test]
fn home_route_uses_singular_primary_directory() {
    let fs = CortexFs::new();

    assert!(
        fs.lookup_path(["home", "1000", "route"]).is_some(),
        "home/<uid>/route must be the primary routing namespace"
    );
    assert!(
        fs.lookup_path(["home", "1000", "routes"]).is_some(),
        "home/<uid>/routes remains a compatibility namespace"
    );
}

#[test]
fn home_thread_and_export_use_singular_primary_directories() {
    let fs = CortexFs::new();

    for (primary, compat) in [
        ("agent", "agents"),
        ("skill", "skills"),
        ("tool", "tools"),
        ("thread", "threads"),
        ("export", "exports"),
    ] {
        assert!(
            fs.lookup_path(["home", "1000", primary]).is_some(),
            "home/<uid>/{primary} must be the primary namespace"
        );
        assert!(
            fs.lookup_path(["home", "1000", compat]).is_some(),
            "home/<uid>/{compat} remains a compatibility namespace"
        );
    }
}

#[test]
fn agent_helper_capability_views_use_singular_primary_directories() {
    let fs = CortexFs::new();

    assert!(
        fs.lookup_path(["agent", "helper", "skill", "list"])
            .is_some(),
        "agent/<id>/skill must be the primary skill namespace"
    );
    assert!(
        fs.lookup_path(["agent", "helper", "skills", "list"])
            .is_some(),
        "agent/<id>/skills remains a compatibility namespace"
    );
    assert!(
        fs.lookup_path(["agent", "helper", "tool", "list"])
            .is_some(),
        "agent/<id>/tool must be the primary tool namespace"
    );
    assert!(
        fs.lookup_path(["agent", "helper", "tools", "list"])
            .is_some(),
        "agent/<id>/tools remains a compatibility namespace"
    );
    assert!(
        fs.lookup_path(["agent", "helper", "thread"]).is_some(),
        "agent/<id>/thread must be the primary thread namespace"
    );
    assert!(
        fs.lookup_path(["agent", "helper", "threads"]).is_some(),
        "agent/<id>/threads remains a compatibility namespace"
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
        fs.lookup_path(["agent", "helper", "mcp", "server", "count"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["agent", "helper", "mcp", "server", "list"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["agent", "helper", "mcp", "server", "enabled"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["agent", "helper", "mcp", "servers", "enabled"])
            .is_some(),
        "agent/<id>/mcp/servers remains a compatibility namespace"
    );
    assert!(
        fs.lookup_path(["agent", "helper", "mcp", "servers_count"])
            .is_none()
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

#[test]
fn cluster_local_uses_singular_primary_directories() {
    let fs = CortexFs::new();

    for (primary, compat) in [
        ("agent", "agents"),
        ("worker", "workers"),
        ("queue", "queues"),
        ("task", "tasks"),
    ] {
        assert!(
            fs.lookup_path(["cluster", "local", primary]).is_some(),
            "cluster/local/{primary} must be the primary namespace"
        );
        assert!(
            fs.lookup_path(["cluster", "local", compat]).is_some(),
            "cluster/local/{compat} remains a compatibility namespace"
        );
    }

    assert!(
        fs.lookup_path(["cluster", "local", "worker", "local-worker"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["cluster", "local", "queue", "default"])
            .is_some()
    );
}

#[test]
fn shared_collab_uses_singular_primary_directories() {
    let fs = CortexFs::new();

    for (primary, compat) in [
        ("task", "tasks"),
        ("handoff", "handoffs"),
        ("lock", "locks"),
        ("decision", "decisions"),
    ] {
        assert!(
            fs.lookup_path(["shared", "project-a", "collab", primary])
                .is_some(),
            "shared/<name>/collab/{primary} must be the primary namespace"
        );
        assert!(
            fs.lookup_path(["shared", "project-a", "collab", compat])
                .is_some(),
            "shared/<name>/collab/{compat} remains a compatibility namespace"
        );
    }

    assert!(
        fs.lookup_path(["shared", "project-a", "collab", "task", "demo", "claim"])
            .is_some()
    );
    assert!(
        fs.lookup_path(["shared", "project-a", "collab", "lock", "lease"])
            .is_some()
    );
}

#[test]
fn local_capability_files_use_cap_as_primary_name() {
    let fs = CortexFs::new();

    assert!(
        fs.lookup_path(["cluster", "local", "worker", "local-worker", "cap"])
            .is_some(),
        "cluster/local/worker/<id>/cap must be the primary capability file"
    );
    assert!(
        fs.lookup_path(["mcp", "server", "local-fs", "cap"])
            .is_some(),
        "mcp/server/<id>/cap must be the primary capability file"
    );
    assert!(
        fs.lookup_path(["cluster", "local", "worker", "local-worker", "capabilities"])
            .is_some(),
        "cluster/local/worker/<id>/capabilities remains a compatibility capability file"
    );
    assert!(
        fs.lookup_path(["mcp", "server", "local-fs", "capabilities"])
            .is_some(),
        "mcp/server/<id>/capabilities remains a compatibility capability file"
    );
}
