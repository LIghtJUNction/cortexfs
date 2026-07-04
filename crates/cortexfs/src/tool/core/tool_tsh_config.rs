impl Tool for TshConfigTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "tsh.config",
            description: "Read or update persistent tsh runtime configuration.",
            input_schema: TSH_CONFIG_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let input = invocation.input().trim();
        let request = if input.is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_str::<Value>(input)
                .map_err(|_error| ToolError::invalid("invalid json input"))?
        };
        let Some(object) = request.as_object() else {
            return Err(ToolError::invalid("input must be a json object"));
        };
        let path = requested_tsh_config_path(&ctx_root_from_env(), object)?;
        let mut config = read_tsh_runtime_config(&path)?;
        let changed = object.contains_key("max_loaded_tools")
            || object.contains_key("cache_capacity")
            || object.contains_key("window_percent");
        if let Some(value) = object.get("max_loaded_tools") {
            config.max_loaded_tools = tsh_tool_count(value, "max_loaded_tools")?;
        }
        if let Some(value) = object.get("cache_capacity") {
            config.cache_capacity = tsh_tool_count(value, "cache_capacity")?;
        }
        if let Some(value) = object.get("window_percent") {
            let window_percent = positive_usize(value, "window_percent")?;
            if !(1..=100).contains(&window_percent) {
                return Err(ToolError::invalid("window_percent must be 1..100"));
            }
            config.window_percent = window_percent;
        }
        if changed {
            write_tsh_runtime_config(&path, config)?;
        }
        output
            .message(&format!(
                "{}\n{}",
                path.display(),
                format_tsh_runtime_config(config)
            ))
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TshRuntimeConfig {
    max_loaded_tools: usize,
    cache_capacity: usize,
    window_percent: usize,
}

impl Default for TshRuntimeConfig {
    fn default() -> Self {
        Self {
            max_loaded_tools: 64,
            cache_capacity: 32,
            window_percent: 1,
        }
    }
}

fn default_tsh_config_path(root: &Path) -> PathBuf {
    root.join("tool/tsh.d/config")
}

fn requested_tsh_config_path(root: &Path, object: &Map<String, Value>) -> ToolResult<PathBuf> {
    let default_path = default_tsh_config_path(root);
    let Some(value) = object.get("path") else {
        return Ok(default_path);
    };
    let Some(path) = value.as_str() else {
        return Err(ToolError::invalid("path must be a string"));
    };
    let requested_path = PathBuf::from(path);
    if requested_path == default_path {
        Ok(default_path)
    } else {
        Err(ToolError::denied(
            "tsh.config path is restricted to CTX_ROOT/tool/tsh.d/config",
        ))
    }
}

fn read_tsh_runtime_config(path: &Path) -> ToolResult<TshRuntimeConfig> {
    let content = match read_regular_utf8_file(path, MAX_TSH_CONFIG_BYTES) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(TshRuntimeConfig::default());
        }
        Err(error) => return Err(ToolError::denied(format!("cannot read config: {error}"))),
    };
    parse_tsh_runtime_config(&content)
}

fn parse_tsh_runtime_config(content: &str) -> ToolResult<TshRuntimeConfig> {
    let mut config = TshRuntimeConfig::default();
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(ToolError::invalid(format!(
                "line {} must be key=value",
                index.saturating_add(1)
            )));
        };
        let value = value.parse::<usize>().map_err(|_error| {
            ToolError::invalid(format!(
                "line {} value must be a positive integer",
                index.saturating_add(1)
            ))
        })?;
        match key {
            "max_loaded_tools" | "cache_capacity" if (1..=MAX_TSH_TOOL_COUNT).contains(&value) => {
                if key == "max_loaded_tools" {
                    config.max_loaded_tools = value;
                } else {
                    config.cache_capacity = value;
                }
            }
            "window_percent" if (1..=100).contains(&value) => config.window_percent = value,
            "max_loaded_tools" | "cache_capacity" => {
                return Err(ToolError::invalid(format!(
                    "line {} value must be 1..{MAX_TSH_TOOL_COUNT}",
                    index.saturating_add(1),
                )));
            }
            "window_percent" => {
                return Err(ToolError::invalid(format!(
                    "line {} window_percent must be 1..100",
                    index.saturating_add(1)
                )));
            }
            _ => {
                return Err(ToolError::invalid(format!(
                    "line {} has unknown key {key}",
                    index.saturating_add(1)
                )));
            }
        }
    }
    Ok(config)
}

fn write_tsh_runtime_config(path: &Path, config: TshRuntimeConfig) -> ToolResult<()> {
    let Some(parent) = path.parent() else {
        return Err(ToolError::invalid(
            "config path must have a parent directory",
        ));
    };
    create_tsh_config_dir(parent)?;
    let content = format_tsh_runtime_config(config);
    write_text_file_atomic(path, &content)
        .map_err(|error| ToolError::denied(format!("cannot write config: {error}")))
}

fn create_tsh_config_dir(path: &Path) -> ToolResult<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            sync_tsh_config_dir(path)
        } else {
            Err(ToolError::denied(
                "config directory is not a plain directory",
            ))
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(ToolError::denied(
                    "config path contains a non-directory entry",
                ));
            }
            Ok(_metadata) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(error) => {
                return Err(ToolError::denied(format!(
                    "cannot inspect config directory: {error}"
                )));
            }
        }
    }

    let mut parent_dir =
        if let Some(existing_parent) = missing.last().and_then(|path| path.parent()) {
            open_plain_directory(existing_parent)
                .map_err(|error| ToolError::denied(format!("cannot open config parent: {error}")))?
        } else {
            return Ok(());
        };

    for directory in missing.iter().rev() {
        let name = directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| ToolError::denied("invalid config directory name"))?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o755),
        )
        .map_err(|error| ToolError::denied(format!("cannot create config directory: {error}")))?;
        parent_dir
            .sync_all()
            .map_err(|error| ToolError::denied(format!("cannot sync config parent: {error}")))?;
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(|error| ToolError::denied(format!("cannot open config directory: {error}")))?;
        parent_dir = fs::File::from(child);
        parent_dir
            .sync_all()
            .map_err(|error| ToolError::denied(format!("cannot sync config directory: {error}")))?;
    }
    Ok(())
}

fn sync_tsh_config_dir(path: &Path) -> ToolResult<()> {
    let directory = open_plain_directory(path)
        .map_err(|error| ToolError::denied(format!("cannot open config directory: {error}")))?;
    directory
        .sync_all()
        .map_err(|error| ToolError::denied(format!("cannot sync config directory: {error}")))
}

fn format_tsh_runtime_config(config: TshRuntimeConfig) -> String {
    format!(
        "max_loaded_tools={}\ncache_capacity={}\nwindow_percent={}\n",
        config.max_loaded_tools, config.cache_capacity, config.window_percent
    )
}

fn positive_usize(value: &Value, field: &str) -> ToolResult<usize> {
    let Some(value) = value.as_u64() else {
        return Err(ToolError::invalid(format!(
            "{field} must be a positive integer"
        )));
    };
    usize::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| ToolError::invalid(format!("{field} must be a positive integer")))
}

fn tsh_tool_count(value: &Value, field: &str) -> ToolResult<usize> {
    let value = positive_usize(value, field)?;
    if value <= MAX_TSH_TOOL_COUNT {
        Ok(value)
    } else {
        Err(ToolError::invalid(format!(
            "{field} must be 1..{MAX_TSH_TOOL_COUNT}"
        )))
    }
}

fn run_tsh_config_cli(
    root: &Path,
    args: &[OsString],
    writer: &mut dyn Write,
) -> io::Result<ExitCode> {
    let input = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let request = if input.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str::<Value>(&input).map_err(io::Error::other)?
    };
    let object = request.as_object().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "input must be a json object")
    })?;
    let path = requested_tsh_config_path(root, object).map_err(|error| tool_error_to_io(&error))?;
    let mut config = read_tsh_runtime_config(&path).map_err(|error| tool_error_to_io(&error))?;
    let changed = object.contains_key("max_loaded_tools")
        || object.contains_key("cache_capacity")
        || object.contains_key("window_percent");
    if let Some(value) = object.get("max_loaded_tools") {
        config.max_loaded_tools =
            tsh_tool_count(value, "max_loaded_tools").map_err(|error| tool_error_to_io(&error))?;
    }
    if let Some(value) = object.get("cache_capacity") {
        config.cache_capacity =
            tsh_tool_count(value, "cache_capacity").map_err(|error| tool_error_to_io(&error))?;
    }
    if let Some(value) = object.get("window_percent") {
        let window_percent =
            positive_usize(value, "window_percent").map_err(|error| tool_error_to_io(&error))?;
        if !(1..=100).contains(&window_percent) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "window_percent must be 1..100",
            ));
        }
        config.window_percent = window_percent;
    }
    if changed {
        write_tsh_runtime_config(&path, config).map_err(|error| tool_error_to_io(&error))?;
    }
    writeln!(writer, "{}", path.display())?;
    writer.write_all(format_tsh_runtime_config(config).as_bytes())?;
    Ok(ExitCode::SUCCESS)
}
