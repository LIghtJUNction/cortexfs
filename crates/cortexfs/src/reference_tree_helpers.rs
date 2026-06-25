const REFERENCE_HOME_UID: u32 = 1000;
const REFERENCE_HOME_GID: u32 = 1000;

fn ensure_reference_home(root: &Path) -> Result<(), ReferenceTreeError> {
    for agent in ["base", "coder", "reviewer"] {
        ensure_reference_home_agent(root, agent)?;
    }
    create_reference_dir(&root.join("home").join("1000").join("tool"))?;
    create_reference_dir(&root.join("home").join("1000").join("model"))?;
    write_reference_text(
        &root.join("home").join("1000").join(".tshrc"),
        "CTX_PATH=/ctx/tool:/ctx/home/1000/tool\n",
    )?;

    ensure_reference_model_alias(
        &root.join("home").join("1000").join("model").join("coder"),
        Path::new("/ctx/model/main"),
    )?;
    ensure_reference_home_ownership(&root.join("home").join("1000"))
}

fn ensure_reference_home_agent(root: &Path, agent: &str) -> Result<(), ReferenceTreeError> {
    let agent_root = root.join("home").join("1000").join("agent").join(agent);
    create_reference_dir(&agent_root.join("root"))?;
    create_reference_dir(&agent_root.join("session").join("index").join("by-cwd"))?;
    create_reference_dir(&agent_root.join("data"))?;
    create_reference_dir(&agent_root.join("cache"))?;
    create_reference_dir(&agent_root.join("log"))
}

fn remove_deprecated_reference_home_tool_aliases(root: &Path) -> Result<(), ReferenceTreeError> {
    let alias = root.join("home").join("1000").join("tool").join("fs.read");
    match fs::read_link(&alias) {
        Ok(target) if target == Path::new("/ctx/tool/fs.read") => {
            fs::remove_file(alias).map_err(|_error| ReferenceTreeError::CannotUnlink)
        }
        Ok(_) | Err(_) => Ok(()),
    }
}

fn migrate_reference_legacy_session_meta_models(root: &Path) -> Result<(), ReferenceTreeError> {
    let mut meta_paths = Vec::new();
    collect_reference_agent_session_meta_paths(&root.join("home"), &mut meta_paths)?;
    collect_reference_shared_agent_session_meta_paths(&root.join("shared"), &mut meta_paths)?;
    for meta_path in meta_paths {
        migrate_reference_session_meta_model(&meta_path)?;
    }
    Ok(())
}

fn collect_reference_agent_session_meta_paths(
    home_root: &Path,
    meta_paths: &mut Vec<PathBuf>,
) -> Result<(), ReferenceTreeError> {
    let Ok(users) = fs::read_dir(home_root) else {
        return Ok(());
    };
    for user in users {
        let user = user.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        if user
            .file_type()
            .map_err(|_error| ReferenceTreeError::CannotCreate)?
            .is_dir()
        {
            collect_reference_session_meta_paths(&user.path().join("agent"), meta_paths)?;
        }
    }
    Ok(())
}

fn collect_reference_shared_agent_session_meta_paths(
    shared_root: &Path,
    meta_paths: &mut Vec<PathBuf>,
) -> Result<(), ReferenceTreeError> {
    let Ok(spaces) = fs::read_dir(shared_root) else {
        return Ok(());
    };
    for space in spaces {
        let space = space.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        if space
            .file_type()
            .map_err(|_error| ReferenceTreeError::CannotCreate)?
            .is_dir()
        {
            collect_reference_session_meta_paths(&space.path().join("agent"), meta_paths)?;
        }
    }
    Ok(())
}

fn collect_reference_session_meta_paths(
    agent_root: &Path,
    meta_paths: &mut Vec<PathBuf>,
) -> Result<(), ReferenceTreeError> {
    let Ok(agents) = fs::read_dir(agent_root) else {
        return Ok(());
    };
    for agent in agents {
        let agent = agent.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        if !agent
            .file_type()
            .map_err(|_error| ReferenceTreeError::CannotCreate)?
            .is_dir()
        {
            continue;
        }
        let session_root = agent.path().join("session");
        let Ok(sessions) = fs::read_dir(session_root) else {
            continue;
        };
        for session in sessions {
            let session = session.map_err(|_error| ReferenceTreeError::CannotCreate)?;
            if session
                .file_type()
                .map_err(|_error| ReferenceTreeError::CannotCreate)?
                .is_dir()
            {
                let meta_path = session.path().join("meta.json");
                if meta_path.is_file() {
                    meta_paths.push(meta_path);
                }
            }
        }
    }
    Ok(())
}

fn migrate_reference_session_meta_model(meta_path: &Path) -> Result<(), ReferenceTreeError> {
    let content =
        fs::read_to_string(meta_path).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let Ok(mut value) = serde_json::from_str::<Value>(&content) else {
        return Ok(());
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    let Some(model) = object.get("model").and_then(Value::as_str) else {
        return Ok(());
    };
    if abi_path::is_model_reference(model) || !is_object_name(model) {
        return Ok(());
    }

    object.insert("model".to_owned(), serde_json::json!("debug/echo"));
    let content =
        serde_json::to_string(&value).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    atomic_replace_text(meta_path, &format!("{content}\n"))
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn create_reference_dir(path: &Path) -> Result<(), ReferenceTreeError> {
    fs::create_dir_all(path).map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn ensure_reference_home_ownership(path: &Path) -> Result<(), ReferenceTreeError> {
    if !nix::unistd::Uid::effective().is_root() {
        return Ok(());
    }
    chown_reference_home_entry(path)
}

fn chown_reference_home_entry(path: &Path) -> Result<(), ReferenceTreeError> {
    nix::unistd::chown(
        path,
        Some(nix::unistd::Uid::from_raw(REFERENCE_HOME_UID)),
        Some(nix::unistd::Gid::from_raw(REFERENCE_HOME_GID)),
    )
    .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn write_reference_text(path: &Path, content: &str) -> Result<(), ReferenceTreeError> {
    if let Some(parent) = path.parent() {
        create_reference_dir(parent)?;
    }
    atomic_replace_text(path, content).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn set_reference_executable(path: &Path) -> Result<(), ReferenceTreeError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn ensure_reference_socket(path: &Path) -> Result<(), ReferenceTreeError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        let file_type = metadata.file_type();
        if file_type.is_socket() {
            return set_reference_socket_permissions(path);
        }
        if file_type.is_symlink() {
            match fs::metadata(path) {
                Ok(target) if target.file_type().is_socket() => {
                    return set_reference_socket_permissions(path);
                }
                Ok(_target) => return Err(ReferenceTreeError::CannotSocket),
                Err(_error) => {
                    fs::remove_file(path).map_err(|_error| ReferenceTreeError::CannotSocket)?;
                }
            }
        } else {
            return Err(ReferenceTreeError::CannotSocket);
        }
    }
    if let Some(parent) = path.parent() {
        create_reference_dir(parent)?;
    }
    UnixListener::bind(path).map_err(|_error| ReferenceTreeError::CannotSocket)?;
    set_reference_socket_permissions(path)
}

fn set_reference_socket_permissions(path: &Path) -> Result<(), ReferenceTreeError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o777))
        .map_err(|_error| ReferenceTreeError::CannotSocket)
}

fn ensure_reference_model_alias(path: &Path, target: &Path) -> Result<(), ReferenceTreeError> {
    if let Ok(existing) = fs::read_link(path) {
        if existing == target || is_valid_ctx_model_symlink(&existing) {
            return Ok(());
        }
        if is_legacy_ctx_model_symlink(&existing) {
            fs::remove_file(path).map_err(|_error| ReferenceTreeError::CannotLink)?;
        } else {
            return Err(ReferenceTreeError::CannotLink);
        }
    } else if path.exists() {
        return Err(ReferenceTreeError::CannotLink);
    }
    if let Some(parent) = path.parent() {
        create_reference_dir(parent)?;
    }
    symlink(target, path).map_err(|_error| ReferenceTreeError::CannotLink)
}

fn is_valid_ctx_model_symlink(target: &Path) -> bool {
    let Some(target) = target.to_str() else {
        return false;
    };
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return false;
    };
    abi_path::is_model_reference(model)
}

fn is_legacy_ctx_model_symlink(target: &Path) -> bool {
    let Some(target) = target.to_str() else {
        return false;
    };
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return false;
    };
    is_object_name(model)
}
