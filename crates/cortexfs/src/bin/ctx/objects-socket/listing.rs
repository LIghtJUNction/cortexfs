use crate::*;

pub(crate) fn list_objects(root: &Path, target: &LsTarget) -> Result<(), CliError> {
    for entry in list_names(root, target)? {
        print_line(&entry)?;
    }
    Ok(())
}

pub(crate) fn list_names(root: &Path, target: &LsTarget) -> Result<Vec<String>, CliError> {
    let LsPath { path, object_class } = resolve_ls_path(root, target)?;

    if let Some(kind) = object_class {
        return list_kind_names(root, kind);
    }

    read_dir_names(&path)
}

pub(crate) fn list_kind_names(root: &Path, kind: ObjectClass) -> Result<Vec<String>, CliError> {
    Ok(read_dir_names(&root.join(kind.as_str()))?
        .into_iter()
        .filter(|name| is_object_name(name))
        .collect())
}

pub(crate) struct LsPath {
    pub(crate) path: PathBuf,
    pub(crate) object_class: Option<ObjectClass>,
}

pub(crate) fn resolve_ls_path(root: &Path, target: &LsTarget) -> Result<LsPath, CliError> {
    let path = match *target {
        LsTarget::Root => return Ok(root_ls_path(root)),
        LsTarget::Path(ref path) => normalized_ls_path(path),
    };

    if path.is_empty() {
        return Ok(root_ls_path(root));
    }

    let resolved = resolve_abi_path(root, &path)?;
    let abi_path = classify_input_path(root, &path)?;
    let object_class = match abi_path.as_str() {
        "model" => Some(ObjectClass::Model),
        "agent" => Some(ObjectClass::Agent),
        "tool" => Some(ObjectClass::Tool),
        _ => None,
    };

    Ok(LsPath {
        path: resolved,
        object_class,
    })
}

pub(crate) fn root_ls_path(root: &Path) -> LsPath {
    LsPath {
        path: root.to_path_buf(),
        object_class: None,
    }
}

pub(crate) fn normalized_ls_path(path: &str) -> String {
    if path == "/" {
        return String::new();
    }

    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }

    trimmed.to_owned()
}

pub(crate) fn read_dir_names(dir: &Path) -> Result<Vec<String>, CliError> {
    let directory = open_plain_directory(dir).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", dir.display()))
    })?;
    let fd_path = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    let entries = fs::read_dir(fd_path).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", dir.display()))
    })?;
    let mut names = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| {
            CliError::unavailable(format!("cannot read {} entry: {error}", dir.display()))
        })?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }

    names.sort();
    Ok(names)
}

pub(crate) fn which_object(root: &Path, class: ObjectClass, name: &str) -> Result<(), CliError> {
    match class {
        ObjectClass::Model if !is_model_name(name) => {
            return Err(CliError::usage(format!("invalid model name: {name}")));
        }
        ObjectClass::Agent => require_cli_name("object name", name)?,
        ObjectClass::Tool => return which_tool(root, name),
        ObjectClass::Model => {}
    }

    let candidate = root.join(class.as_str()).join(name);
    if is_executable_file(&candidate) {
        return print_line(&candidate.display().to_string());
    }

    Err(CliError::unavailable(format!(
        "{} not found: {name}",
        class.as_str()
    )))
}

pub(crate) fn which_tool(root: &Path, name: &str) -> Result<(), CliError> {
    require_cli_name("object name", name)?;

    if let Some(hit) = ctx_tool_path(root)?.find(name).map_err(tool_path_error)? {
        return print_line(&hit.path().display().to_string());
    }

    Err(CliError::unavailable(format!("tool not found: {name}")))
}

pub(crate) fn run_visible_tool(
    root: &Path,
    name: &str,
    args: &[String],
) -> Result<ExitCode, CliError> {
    run_visible_tool_with_writer(root, name, args, &mut io::stdout())
}

pub(crate) fn is_safe_direct_core_tool_cli(name: &str) -> bool {
    matches!(name, "tsh.config")
}

pub(crate) fn run_visible_tool_with_writer(
    root: &Path,
    name: &str,
    args: &[String],
    writer: &mut dyn Write,
) -> Result<ExitCode, CliError> {
    require_cli_name("tool name", name)?;
    let Some(_hit) = ctx_tool_path(root)?.find(name).map_err(tool_path_error)? else {
        return Err(CliError::unavailable(format!(
            "tool not found in CTX_PATH: {name}"
        )));
    };
    let cli_args = args.iter().map(OsString::from).collect::<Vec<_>>();
    if is_safe_direct_core_tool_cli(name)
        && let Some(code) = run_core_tool_cli_with_root(root, name, &cli_args, writer)
            .map_err(|error| CliError::unavailable(format!("tool {name} failed: {error}")))?
    {
        return Ok(code);
    }

    Err(CliError::unavailable(format!(
        "ctx tool {name} is disabled because direct CTX_PATH execution bypasses CortexFS tool authorization"
    )))
}

pub(crate) fn path_shared(root: &Path, name: &str) -> Result<(), CliError> {
    require_cli_name("shared name", name)?;
    print_line(&root.join("shared").join(name).display().to_string())
}

pub(crate) fn history(root: &Path, agent: &str, session: Option<&str>) -> Result<(), CliError> {
    let session_dir = agent_session_dir(root, agent, session)?;
    cat_path(&session_dir.join("messages.jsonl"))
}

pub(crate) fn latest(root: &Path, agent: &str, session: Option<&str>) -> Result<(), CliError> {
    let session_dir = agent_session_dir(root, agent, session)?;
    cat_path(&session_dir.join("latest.md"))
}

pub(crate) fn ping(root: &Path, path: &str) -> Result<ExitCode, CliError> {
    stream_socket_request(&object_socket_path(root, path)?, "{\"op\":\"ping\"}\n")
}

pub(crate) fn cancel(root: &Path, path: &str, run: &str) -> Result<ExitCode, CliError> {
    require_cli_name("run id", run)?;
    let request = format!("{{\"op\":\"cancel\",\"id\":{}}}\n", json_string(run));
    stream_socket_request(&object_socket_path(root, path)?, &request)
}
