use super::*;
use crate::support::atomic::atomic_replace_text;

pub(crate) const REFERENCE_HOME_UID: u32 = 1000;
pub(crate) const REFERENCE_HOME_GID: u32 = 1000;

pub(crate) const MAX_REFERENCE_SESSION_META_BYTES: u64 = 64 * 1024;

pub(crate) fn ensure_reference_home(root: &Path) -> Result<(), ReferenceTreeError> {
    for agent in REFERENCE_AGENTS {
        ensure_reference_home_agent(root, agent.name)?;
    }
    create_reference_dir(&cortexfs_paths::home_tool_path(root, "1000"))?;
    create_reference_dir(&cortexfs_paths::home_model_path(root, "1000"))?;
    let ctx = cortexfs_paths::ctx_root();
    let tool_path = cortexfs_paths::tool_root_path(&ctx);
    let home_tool_path = cortexfs_paths::home_tool_path(&ctx, "1000");
    write_reference_text(
        &cortexfs_paths::ctx_home_path(root, "1000").join(".tshrc"),
        &format!(
            "CTX_PATH={}:{}\n",
            tool_path.display(),
            home_tool_path.display()
        ),
    )?;

    ensure_reference_home_scaffold_ownership(root)
}

pub(crate) fn ensure_reference_home_agent(
    root: &Path,
    agent: &str,
) -> Result<(), ReferenceTreeError> {
    let agent_root = cortexfs_paths::agent_home_path(root, "1000", agent);
    create_reference_dir(&agent_root.join("root"))?;
    create_reference_dir(&agent_root.join("session").join("index").join("by-cwd"))?;
    create_reference_dir(&agent_root.join("session").join("index").join("by-hash"))?;
    create_reference_dir(&agent_root.join("session").join("index").join("by-uuid"))?;
    create_reference_dir(&cortexfs_paths::session_channel_index_path(
        &agent_root.join("session"),
    ))?;
    create_reference_dir(&agent_root.join("data"))?;
    create_reference_dir(&agent_root.join("cache"))?;
    create_reference_dir(&agent_root.join("log"))
}

pub(crate) fn ensure_reference_home_scaffold_ownership(
    root: &Path,
) -> Result<(), ReferenceTreeError> {
    let home = cortexfs_paths::ctx_home_path(root, "1000");
    for path in [
        home.clone(),
        cortexfs_paths::home_agent_root_path(root, "1000"),
        cortexfs_paths::home_tool_path(root, "1000"),
        cortexfs_paths::home_model_path(root, "1000"),
        home.join(".tshrc"),
    ] {
        ensure_reference_home_entry_ownership(&path)?;
    }
    for agent in REFERENCE_AGENTS {
        let agent_root = cortexfs_paths::agent_home_path(root, "1000", agent.name);
        for path in [
            agent_root.clone(),
            agent_root.join("root"),
            agent_root.join("session"),
            agent_root.join("session").join("index"),
            agent_root.join("session").join("index").join("by-cwd"),
            agent_root.join("session").join("index").join("by-hash"),
            agent_root.join("session").join("index").join("by-uuid"),
            cortexfs_paths::session_channel_index_path(&agent_root.join("session")),
            agent_root.join("data"),
            agent_root.join("cache"),
            agent_root.join("log"),
        ] {
            ensure_reference_home_entry_ownership(&path)?;
        }
    }
    Ok(())
}

pub(crate) fn migrate_reference_legacy_session_meta_models(
    root: &Path,
) -> Result<(), ReferenceTreeError> {
    let mut meta_paths = Vec::new();
    collect_reference_agent_session_meta_paths(
        &cortexfs_paths::home_root_path(root),
        &mut meta_paths,
    )?;
    collect_reference_shared_agent_session_meta_paths(
        &cortexfs_paths::shared_root_path(root),
        &mut meta_paths,
    )?;
    for meta_path in meta_paths {
        migrate_reference_session_meta_model(&meta_path)?;
    }
    Ok(())
}

pub(crate) fn collect_reference_agent_session_meta_paths(
    home_root: &Path,
    meta_paths: &mut Vec<PathBuf>,
) -> Result<(), ReferenceTreeError> {
    let Ok(home_dir) = open_reference_dir(home_root) else {
        return Ok(());
    };
    let users = reference_tree_read_dir(&home_dir)?;
    for user in users {
        let user = user.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        let user_name = reference_tree_entry_name(&user)?;
        if reference_tree_entry_is_directory(&home_dir, user_name.as_str())? {
            collect_reference_session_meta_paths(
                &cortexfs_paths::home_agent_root_from_home_path(&home_root.join(&user_name)),
                meta_paths,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn collect_reference_shared_agent_session_meta_paths(
    shared_root: &Path,
    meta_paths: &mut Vec<PathBuf>,
) -> Result<(), ReferenceTreeError> {
    let Ok(shared_dir) = open_reference_dir(shared_root) else {
        return Ok(());
    };
    let spaces = reference_tree_read_dir(&shared_dir)?;
    for space in spaces {
        let space = space.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        let space_name = reference_tree_entry_name(&space)?;
        if reference_tree_entry_is_directory(&shared_dir, space_name.as_str())? {
            collect_reference_session_meta_paths(
                &cortexfs_paths::shared_agent_root_from_space_path(&shared_root.join(&space_name)),
                meta_paths,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn collect_reference_session_meta_paths(
    agent_root: &Path,
    meta_paths: &mut Vec<PathBuf>,
) -> Result<(), ReferenceTreeError> {
    let Ok(agent_root_dir) = open_reference_dir(agent_root) else {
        return Ok(());
    };
    let agents = reference_tree_read_dir(&agent_root_dir)?;
    for agent in agents {
        let agent = agent.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        let agent_name = reference_tree_entry_name(&agent)?;
        if !reference_tree_entry_is_directory(&agent_root_dir, agent_name.as_str())? {
            continue;
        }
        let session_root = agent_root.join(&agent_name).join("session");
        let Ok(session_root_dir) = open_reference_dir(&session_root) else {
            continue;
        };
        let sessions = reference_tree_read_dir(&session_root_dir)?;
        for session in sessions {
            let session = session.map_err(|_error| ReferenceTreeError::CannotCreate)?;
            let session_name = reference_tree_entry_name(&session)?;
            if !reference_tree_entry_is_directory(&session_root_dir, session_name.as_str())? {
                continue;
            }
            let session_path = session_root.join(&session_name);
            let Ok(session_dir) = open_reference_dir(&session_path) else {
                continue;
            };
            if reference_tree_entry_is_file(&session_dir, "meta.json")? {
                meta_paths.push(session_path.join("meta.json"));
            }
        }
    }
    Ok(())
}

pub(crate) fn reference_tree_read_dir(
    directory: &fs::File,
) -> Result<fs::ReadDir, ReferenceTreeError> {
    fs::read_dir(support::plain::proc_fd_path(directory))
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

pub(crate) fn reference_tree_entry_name(
    entry: &fs::DirEntry,
) -> Result<String, ReferenceTreeError> {
    entry
        .file_name()
        .to_str()
        .map(str::to_owned)
        .ok_or(ReferenceTreeError::CannotCreate)
}

pub(crate) fn reference_tree_entry_is_directory(
    directory: &fs::File,
    name: &str,
) -> Result<bool, ReferenceTreeError> {
    Ok(reference_tree_entry_file_type(directory, name)? == libc::S_IFDIR)
}

pub(crate) fn reference_tree_entry_is_file(
    directory: &fs::File,
    name: &str,
) -> Result<bool, ReferenceTreeError> {
    let stat =
        match nix::sys::stat::fstatat(directory, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(nix::errno::Errno::ENOENT) => return Ok(false),
            Err(_error) => return Err(ReferenceTreeError::CannotCreate),
        };
    Ok(stat.st_mode & libc::S_IFMT == libc::S_IFREG)
}

pub(crate) fn reference_tree_entry_file_type(
    directory: &fs::File,
    name: &str,
) -> Result<libc::mode_t, ReferenceTreeError> {
    let stat = nix::sys::stat::fstatat(directory, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    Ok(stat.st_mode & libc::S_IFMT)
}

pub(crate) fn read_reference_symlink(path: &Path) -> Result<PathBuf, ReferenceTreeError> {
    let Some(parent) = path.parent() else {
        return Err(ReferenceTreeError::CannotUnlink);
    };
    let directory =
        open_reference_dir(parent).map_err(|_error| ReferenceTreeError::CannotUnlink)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ReferenceTreeError::CannotUnlink)?;
    nix::fcntl::readlinkat(&directory, name)
        .map(PathBuf::from)
        .map_err(|_error| ReferenceTreeError::CannotUnlink)
}

pub(crate) fn migrate_reference_session_meta_model(
    meta_path: &Path,
) -> Result<(), ReferenceTreeError> {
    let content = support::plain::read_small_text_file(meta_path, MAX_REFERENCE_SESSION_META_BYTES)
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let Ok(mut value) = serde_json::from_str::<Value>(&content) else {
        return Ok(());
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    let Some(model) = object.get("model").and_then(Value::as_str) else {
        return Ok(());
    };
    if abi::path::is_model_reference(model) || !is_object_name(model) {
        return Ok(());
    }

    object.insert("model".to_owned(), serde_json::json!("debug/echo"));
    let content =
        serde_json::to_string(&value).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    atomic_replace_text(meta_path, &format!("{content}\n"))
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}
