use crate::*;

pub(crate) fn resolve_tool_hit(root: &Path, name: &str) -> Result<cortexfs::ToolHit, TshError> {
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find(name).map_err(tool_path_error)? else {
        return command_not_found(name);
    };
    Ok(hit)
}

pub(crate) fn load_tool_context(
    root: &Path,
    name: &str,
    pinned: bool,
) -> Result<LoadedTool, TshError> {
    let hit = resolve_tool_hit(root, name)?;
    Ok(LoadedTool {
        name: name.to_owned(),
        path: hit.path().to_path_buf(),
        description: tool_description(&hit),
        schema: tool_schema(&hit),
        dynamic_resident: false,
        pinned,
        last_used: 0,
    })
}

pub(crate) fn report_context_evictions(evicted: Vec<LoadedTool>) -> Result<(), TshError> {
    for tool in evicted {
        write_stdout(&format!("auto-unloaded {}\tcontext-limit\n", tool.name))?;
    }
    Ok(())
}

pub(crate) fn tool_description(hit: &cortexfs::ToolHit) -> String {
    read_control_text(hit, "description")
        .map(|description| terminal_safe_text(&description))
        .unwrap_or_default()
}

pub(crate) fn tool_schema(hit: &cortexfs::ToolHit) -> Option<String> {
    read_control_text(hit, "schema")
}

pub(crate) fn read_control_text(hit: &cortexfs::ToolHit, file: &str) -> Option<String> {
    read_small_plain_text_file(&hit.control_dir().join(file), MAX_TSH_CONTROL_BYTES, "tsh")
        .ok()
        .map(|content| content.trim().to_owned())
        .filter(|content| !content.is_empty())
}

pub(crate) fn terminal_safe_text(text: &str) -> String {
    text.chars().flat_map(char::escape_default).collect()
}

pub(crate) fn append_schema_help(text: &mut String, schema: &str) {
    let Ok(value) = serde_json::from_str::<Value>(schema) else {
        return;
    };
    if let Some(title) = value.get("title").and_then(Value::as_str) {
        let title = terminal_safe_text(title);
        let _ignored = writeln!(text, "  schema: {title}");
    }
    if let Some(description) = value.get("description").and_then(Value::as_str) {
        let description = terminal_safe_text(description);
        let _ignored = writeln!(text, "  schema-description: {description}");
    }
    if let Some(required) = value.get("required").and_then(Value::as_array) {
        let fields = required
            .iter()
            .filter_map(Value::as_str)
            .map(terminal_safe_text)
            .collect::<Vec<_>>()
            .join(" ");
        if !fields.is_empty() {
            let _ignored = writeln!(text, "  required: {fields}");
        }
    }
}

pub(crate) fn command_not_found<T>(name: &str) -> Result<T, TshError> {
    Err(TshError::unavailable(format!(
        "{name}: command not found\ntry: tools"
    )))
}

pub(crate) fn ctx_tool_path(root: &Path) -> Result<ToolPath, TshError> {
    let home = ctx_home(root)?;
    ctx_tool_path_with_home(
        root,
        &home,
        env::var("CTX_PATH"),
        env::var_os("CTX_AGENT").is_none(),
    )
}

pub(crate) fn ctx_tool_path_with_home(
    root: &Path,
    home: &Path,
    env_ctx_path: Result<String, env::VarError>,
    prefer_tshrc: bool,
) -> Result<ToolPath, TshError> {
    if prefer_tshrc && let Some(value) = tshrc_ctx_path(root, home)? {
        return Ok(tshrc_tool_path(root, home, &value));
    }

    match env_ctx_path {
        Ok(value) => Ok(ToolPath::parse(&value)),
        Err(env::VarError::NotPresent) => tshrc_ctx_path(root, home)?.map_or_else(
            || Ok(ToolPath::default(root, home)),
            |value| Ok(tshrc_tool_path(root, home, &value)),
        ),
        Err(env::VarError::NotUnicode(_value)) => Err(TshError::usage("CTX_PATH must be UTF-8")),
    }
}

pub(crate) fn tshrc_tool_path(root: &Path, home: &Path, value: &str) -> ToolPath {
    ToolPath::new(value.split(':').map(|component| {
        let path = Path::new(component);
        if path == Path::new("/ctx/tool") {
            return root.join("tool");
        }
        if let Some(uid) = home.file_name()
            && path == Path::new("/ctx/home").join(uid).join("tool")
        {
            return home.join("tool");
        }
        path.to_path_buf()
    }))
}

pub(crate) fn tshrc_ctx_path(root: &Path, home: &Path) -> Result<Option<String>, TshError> {
    let path = home.join(".tshrc");
    let content = match read_small_plain_text_file(&path, MAX_TSH_CONTROL_BYTES, "tsh") {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(TshError::unavailable(format!(
                "cannot read {}: {error}",
                path.display()
            )));
        }
    };
    let value = parse_tshrc_ctx_path(&content)
        .map_err(|message| TshError::usage(format!("invalid {}: {message}", path.display())))?;
    if let Some(ref value) = value {
        validate_tshrc_ctx_path(value, root, home)
            .map_err(|message| TshError::usage(format!("invalid {}: {message}", path.display())))?;
    }
    Ok(value)
}

pub(crate) fn parse_tshrc_ctx_path(content: &str) -> Result<Option<String>, String> {
    let mut value = None;
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(path) = line.strip_prefix("CTX_PATH=") else {
            return Err(format!(
                "line {} must be CTX_PATH=...",
                index.saturating_add(1)
            ));
        };
        if path.is_empty() {
            return Err(format!(
                "line {} has empty CTX_PATH",
                index.saturating_add(1)
            ));
        }
        if value.replace(path.to_owned()).is_some() {
            return Err(format!("line {} repeats CTX_PATH", index.saturating_add(1)));
        }
    }
    Ok(value)
}

pub(crate) fn validate_tshrc_ctx_path(value: &str, root: &Path, home: &Path) -> Result<(), String> {
    for component in value.split(':') {
        if component.is_empty() {
            return Err("CTX_PATH contains an empty component".to_owned());
        }
        let path = Path::new(component);
        if !path.is_absolute() {
            return Err(format!("CTX_PATH component is not absolute: {component}"));
        }
        if is_allowed_tshrc_tool_dir(path, root, home) {
            continue;
        }
        return Err(format!(
            "CTX_PATH component must be /ctx/tool, /ctx/home/<uid>/tool, or the matching --root/CTX_HOME tool directory: {component}"
        ));
    }
    Ok(())
}

pub(crate) fn is_allowed_tshrc_tool_dir(path: &Path, root: &Path, home: &Path) -> bool {
    path == Path::new("/ctx/tool")
        || path == root.join("tool")
        || path == home.join("tool")
        || home
            .file_name()
            .is_some_and(|uid| path == Path::new("/ctx/home").join(uid).join("tool"))
}

pub(crate) fn ctx_home(root: &Path) -> Result<PathBuf, TshError> {
    if let Some(home) = env::var_os("CTX_HOME") {
        return Ok(PathBuf::from(home));
    }
    Ok(root
        .join("home")
        .join(current_uid_text().map_err(TshError::unavailable)?))
}

pub(crate) fn tool_path_error(error: cortexfs::ToolPathError) -> TshError {
    match error {
        cortexfs::ToolPathError::InvalidName => TshError::usage("invalid tool name"),
        cortexfs::ToolPathError::CannotReadDirectory => {
            TshError::unavailable("cannot read CTX_PATH directory")
        }
    }
}
