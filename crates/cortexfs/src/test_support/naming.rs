use crate::{CortexFs, ROOT_INODE};
use fuse3::FileType;
use std::ffi::OsStr;

const ROOT_ABI_NAMES: &[&str] = &[
    "status", "cap", "format", "provider", "chan", "model", "home", "group", "shared", "ext",
    "space", "agent", "cluster", "mcp", "skill", "tool", "memory", "vector", "db", "audit",
    "control",
];

const FORBIDDEN_ROOT_NAMES: &[&str] = &[
    "api",
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
    "ctx_home",
    "current_user",
    "user_home",
    "my",
    "me",
    "default",
];

const FORBIDDEN_STALE_DIR_NAMES: &[&str] = &[
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
    "routes",
    "threads",
    "exports",
    "filters",
    "limits",
    "artifacts",
    "claims",
    "leases",
    "handoffs",
    "locks",
    "decisions",
    "stores",
    "migrations",
    "pools",
    "servers",
    "resources",
    "prompts",
    "sessions",
    "workers",
    "queues",
    "tasks",
    "indexes",
    "default_model",
];

const FORBIDDEN_STALE_FILE_NAMES: &[&str] =
    &["capabilities", "formats", "layers", "sources", "reload"];

#[test]
fn root_names_are_single_canonical_abi_entries() -> fuse3::Result<()> {
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
    for name in FORBIDDEN_ROOT_NAMES {
        assert!(
            fs.lookup_child(ROOT_INODE, OsStr::new(name)).is_err(),
            "root ABI must not expose stale entry {name}"
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
fn nested_names_expose_only_single_canonical_entries() {
    let fs = CortexFs::new();

    for path in [
        &["home", "1000", "route"][..],
        &["home", "1000", "agent"][..],
        &["home", "1000", "skill"][..],
        &["home", "1000", "tool"][..],
        &["home", "1000", "thread"][..],
        &["home", "1000", "export"][..],
        &["home", "1000", "model"][..],
        &["home", "1000", "mcp", "server"][..],
        &["home", "1000", "mcp", "tool"][..],
        &["home", "1000", "export", "filter"][..],
        &["home", "1000", "thread", "demo", "tool-loop", "limit"][..],
        &["shared", "project-a", "collab", "blackboard", "artifact"][..],
        &["shared", "project-a", "collab", "task"][..],
        &["shared", "project-a", "collab", "handoff"][..],
        &["shared", "project-a", "collab", "lock"][..],
        &["shared", "project-a", "collab", "decision"][..],
        &["vector", "store"][..],
        &["vector", "index"][..],
        &["db", "postgres", "migration"][..],
        &["db", "postgres", "pool"][..],
        &["mcp", "server"][..],
        &["mcp", "tool"][..],
        &["mcp", "resource"][..],
        &["mcp", "prompt"][..],
        &["mcp", "session"][..],
        &["cluster", "local", "agent"][..],
        &["cluster", "local", "worker"][..],
        &["cluster", "local", "queue"][..],
        &["cluster", "local", "task"][..],
        &["skill", "index"][..],
        &["memory", "index"][..],
    ] {
        assert!(
            fs.tree.path_inode(path).is_some(),
            "{} must be the canonical namespace",
            path.join("/")
        );
    }

    for path in [
        &["home", "1000", "routes"][..],
        &["home", "1000", "agents"][..],
        &["home", "1000", "skills"][..],
        &["home", "1000", "tools"][..],
        &["home", "1000", "threads"][..],
        &["home", "1000", "exports"][..],
        &["home", "1000", "models"][..],
        &["home", "1000", "mcp", "servers"][..],
        &["home", "1000", "mcp", "tools"][..],
        &["home", "1000", "export", "filters"][..],
        &["home", "1000", "thread", "demo", "tool-loop", "limits"][..],
        &["shared", "project-a", "collab", "blackboard", "artifacts"][..],
        &["shared", "project-a", "collab", "tasks"][..],
        &["shared", "project-a", "collab", "handoffs"][..],
        &["shared", "project-a", "collab", "locks"][..],
        &["shared", "project-a", "collab", "decisions"][..],
        &["vector", "stores"][..],
        &["vector", "indexes"][..],
        &["db", "postgres", "migrations"][..],
        &["db", "postgres", "pools"][..],
        &["mcp", "servers"][..],
        &["mcp", "tools"][..],
        &["mcp", "resources"][..],
        &["mcp", "prompts"][..],
        &["mcp", "sessions"][..],
        &["cluster", "local", "agents"][..],
        &["cluster", "local", "workers"][..],
        &["cluster", "local", "queues"][..],
        &["cluster", "local", "tasks"][..],
        &["skill", "indexes"][..],
        &["memory", "indexes"][..],
    ] {
        assert!(
            fs.tree.path_inode(path).is_none(),
            "{} must not exist during development",
            path.join("/")
        );
    }
}

#[test]
fn static_tree_contains_no_stale_directories() {
    let fs = CortexFs::new();

    for (path, inode) in &fs.tree.paths {
        let Some(node) = fs.tree.nodes.get(inode) else {
            continue;
        };
        if node.kind() != FileType::Directory {
            continue;
        }
        assert!(
            !FORBIDDEN_STALE_DIR_NAMES.contains(&node.name()),
            "{} must not expose stale directory {}",
            path.join("/"),
            node.name()
        );
    }
}

#[test]
fn static_tree_contains_no_stale_files() {
    let fs = CortexFs::new();

    for (path, inode) in &fs.tree.paths {
        let Some(node) = fs.tree.nodes.get(inode) else {
            continue;
        };
        if node.kind() == FileType::Directory {
            continue;
        }
        assert!(
            !FORBIDDEN_STALE_FILE_NAMES.contains(&node.name()),
            "{} must not expose stale file {}",
            path.join("/"),
            node.name()
        );
    }
}

#[test]
fn metadata_uses_cap_not_capabilities() -> fuse3::Result<()> {
    let fs = CortexFs::new();
    let provider = crate::default_provider_spec()?;
    let model_id = crate::provider_model_id(&provider);

    for path in [
        &["model", model_id.as_str(), "cap"][..],
        &[
            "provider",
            provider.id,
            "model",
            provider.default_model,
            "cap",
        ][..],
        &["home", "1000", "model", model_id.as_str(), "cap"][..],
        &["cluster", "local", "worker", "local-worker", "cap"][..],
        &["mcp", "server", "local-fs", "cap"][..],
    ] {
        assert!(fs.tree.path_inode(path).is_some());
    }
    for path in [
        &["model", model_id.as_str(), "capabilities"][..],
        &[
            "provider",
            provider.id,
            "model",
            provider.default_model,
            "capabilities",
        ][..],
        &["home", "1000", "model", model_id.as_str(), "capabilities"][..],
        &["cluster", "local", "worker", "local-worker", "capabilities"][..],
        &["mcp", "server", "local-fs", "capabilities"][..],
    ] {
        assert!(fs.tree.path_inode(path).is_none());
    }
    Ok(())
}
