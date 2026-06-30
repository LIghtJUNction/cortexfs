fn resolve_tool_hit(root: &Path, name: &str) -> Result<cortexfs::ToolHit, TshError> {
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find(name).map_err(tool_path_error)? else {
        return command_not_found(name);
    };
    Ok(hit)
}

fn load_tool_context(root: &Path, name: &str, pinned: bool) -> Result<LoadedTool, TshError> {
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

fn report_context_evictions(evicted: Vec<LoadedTool>) -> Result<(), TshError> {
    for tool in evicted {
        write_stdout(&format!("auto-unloaded {}\tcontext-limit\n", tool.name))?;
    }
    Ok(())
}

fn tool_description(hit: &cortexfs::ToolHit) -> String {
    read_control_text(hit, "description")
        .map(|description| terminal_safe_text(&description))
        .unwrap_or_default()
}

fn tool_schema(hit: &cortexfs::ToolHit) -> Option<String> {
    read_control_text(hit, "schema")
}

fn read_control_text(hit: &cortexfs::ToolHit, file: &str) -> Option<String> {
    read_small_plain_text_file(&hit.control_dir().join(file))
        .ok()
        .map(|content| content.trim().to_owned())
        .filter(|content| !content.is_empty())
}

fn read_small_plain_text_file(path: &Path) -> io::Result<String> {
    let mut file = open_plain_read_file(path)?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_TSH_CONTROL_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds tsh control read limit",
        ));
    }
    let len = usize::try_from(metadata.len()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file is too large to read: {error}"),
        )
    })?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.utf8_error()))
}

fn open_plain_read_file(path: &Path) -> io::Result<fs::File> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let parent_dir = open_plain_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )?;
    let file = fs::File::from(file_fd);
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    Ok(file)
}

fn open_executable_no_follow(path: &Path) -> io::Result<fs::File> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let parent_dir = open_plain_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW,
        nix::sys::stat::Mode::empty(),
    )?;
    let file = fs::File::from(file_fd);
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    Ok(file)
}

fn proc_fd_path(file: &fs::File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

fn open_plain_directory(path: &Path) -> io::Result<fs::File> {
    let mut directory = if path.is_absolute() {
        open_single_plain_directory(Path::new("/"))?
    } else {
        open_single_plain_directory(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid directory name")
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_single_plain_directory(path: &Path) -> io::Result<fs::File> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}

fn terminal_safe_text(text: &str) -> String {
    text.chars().flat_map(char::escape_default).collect()
}

fn append_schema_help(text: &mut String, schema: &str) {
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

fn command_not_found<T>(name: &str) -> Result<T, TshError> {
    Err(TshError::unavailable(format!(
        "{name}: command not found\ntry: tools"
    )))
}

fn ctx_tool_path(root: &Path) -> Result<ToolPath, TshError> {
    let home = ctx_home(root)?;
    ctx_tool_path_with_home(
        root,
        &home,
        env::var("CTX_PATH"),
        env::var_os("CTX_AGENT").is_none(),
    )
}

fn ctx_tool_path_with_home(
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

fn tshrc_tool_path(root: &Path, home: &Path, value: &str) -> ToolPath {
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

fn tshrc_ctx_path(root: &Path, home: &Path) -> Result<Option<String>, TshError> {
    let path = home.join(".tshrc");
    let content = match read_small_plain_text_file(&path) {
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

fn parse_tshrc_ctx_path(content: &str) -> Result<Option<String>, String> {
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

fn validate_tshrc_ctx_path(value: &str, root: &Path, home: &Path) -> Result<(), String> {
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

fn is_allowed_tshrc_tool_dir(path: &Path, root: &Path, home: &Path) -> bool {
    path == Path::new("/ctx/tool")
        || path == root.join("tool")
        || path == home.join("tool")
        || home
            .file_name()
            .is_some_and(|uid| path == Path::new("/ctx/home").join(uid).join("tool"))
}

fn ctx_home(root: &Path) -> Result<PathBuf, TshError> {
    if let Some(home) = env::var_os("CTX_HOME") {
        return Ok(PathBuf::from(home));
    }
    Ok(root.join("home").join(current_uid()?))
}

const ID_PROGRAM: &str = "/usr/bin/id";

fn get_id_program() -> &'static str {
    ID_PROGRAM
}

fn id_command() -> ProcessCommand {
    let mut command = ProcessCommand::new(get_id_program());
    command.arg("-u").env_clear().env("PATH", "/usr/bin:/bin");
    command
}

fn current_uid() -> Result<String, TshError> {
    let output = id_command()
        .output()
        .map_err(|error| TshError::unavailable(format!("cannot run id -u: {error}")))?;
    if !output.status.success() {
        return Err(TshError::unavailable("id -u failed"));
    }
    let uid = String::from_utf8(output.stdout)
        .map_err(|_error| TshError::unavailable("id -u returned non-UTF-8 output"))?;
    parse_current_uid(&uid)
}

fn parse_current_uid(output: &str) -> Result<String, TshError> {
    let uid = output.trim();
    if uid.is_empty() {
        return Err(TshError::unavailable("id -u returned empty output"));
    }
    if !uid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(TshError::unavailable("id -u returned invalid uid"));
    }
    Ok(uid.to_owned())
}

fn tool_path_error(error: cortexfs::ToolPathError) -> TshError {
    match error {
        cortexfs::ToolPathError::InvalidName => TshError::usage("invalid tool name"),
        cortexfs::ToolPathError::CannotReadDirectory => {
            TshError::unavailable("cannot read CTX_PATH directory")
        }
    }
}
