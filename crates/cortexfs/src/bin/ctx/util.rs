const MAX_CTX_FILE_CHECK_BYTES: u64 = 1024 * 1024;

fn read_file_to_string(path: &Path) -> Result<String, CliError> {
    let mut file = open_plain_read_file(path)?;
    let metadata = file.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    if !metadata.is_file() || metadata.len() > MAX_CTX_FILE_CHECK_BYTES {
        return Err(CliError::unavailable(format!(
            "cannot read {}: not a small regular file",
            path.display()
        )));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|error| CliError::unavailable(format!("cannot read {}: {error}", path.display())))?;
    let mut content = vec![0; len];
    file.read_exact(&mut content).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", path.display()))
    })?;
    String::from_utf8(content)
        .map_err(|error| CliError::unavailable(format!("cannot read {}: {error}", path.display())))
}

fn open_plain_read_file(path: &Path) -> Result<fs::File, CliError> {
    let Some(parent) = path.parent() else {
        return Err(CliError::usage("file path must have a parent directory"));
    };
    let parent_dir = open_plain_file_parent_dir(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::usage("file path must end with a valid UTF-8 file name"))?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|error| CliError::unavailable(format!("cannot read {}: {error}", path.display())))?;
    Ok(fs::File::from(file_fd))
}

fn open_executable_no_follow(path: &Path) -> Result<fs::File, CliError> {
    let Some(parent) = path.parent() else {
        return Err(CliError::usage("executable path must have a parent directory"));
    };
    let parent_dir = open_plain_file_parent_dir(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::usage("executable path must end with a valid UTF-8 file name"))?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|error| CliError::unavailable(format!("cannot open {}: {error}", path.display())))?;
    let file = fs::File::from(file_fd);
    let metadata = file.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(CliError::unavailable(format!(
            "object is not executable: {}",
            path.display()
        )));
    }
    Ok(file)
}

fn classify_input_path(root: &Path, path: &str) -> Result<String, CliError> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        let relative = candidate.strip_prefix(root).map_err(|error| {
            CliError::usage(format!(
                "absolute file path must be under CTX_ROOT: {error}"
            ))
        })?;
        validate_relative_abi_path(relative)?;
        return Ok(relative.display().to_string());
    }
    validate_relative_abi_path_text(path)?;
    validate_relative_abi_path(candidate)?;
    Ok(path.to_owned())
}

fn resolve_abi_path(root: &Path, path: &str) -> Result<PathBuf, CliError> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        let relative = candidate.strip_prefix(root).map_err(|error| {
            CliError::usage(format!(
                "absolute file path must be under CTX_ROOT: {error}"
            ))
        })?;
        validate_relative_abi_path(relative)?;
        return Ok(root.join(relative));
    }

    validate_relative_abi_path_text(path)?;

    Ok(root.join(path))
}

fn validate_relative_abi_path_text(path: &str) -> Result<(), CliError> {
    if path
        .split('/')
        .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || path.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(CliError::usage("file path must be a relative ABI path"));
    }
    Ok(())
}

fn validate_relative_abi_path(path: &Path) -> Result<(), CliError> {
    for component in path.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(CliError::usage("file path must be a relative ABI path"));
        };
        let Some(part) = part.to_str() else {
            return Err(CliError::usage("file path must be a relative ABI path"));
        };
        if part.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(CliError::usage("file path must be a relative ABI path"));
        }
    }

    Ok(())
}

fn temp_file_name(attempt: u8) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(".ctx.tmp.{}.{}.{}", std::process::id(), nanos, attempt)
}

fn validate_name(name: &str) -> Result<(), CliError> {
    if is_object_name(name) {
        print_line("ok")
    } else {
        Err(CliError::usage(format!("invalid name: {name}")))
    }
}

fn ctx_tool_path(root: &Path) -> Result<ToolPath, CliError> {
    match env::var("CTX_PATH") {
        Ok(value) => Ok(ToolPath::parse(&value)),
        Err(env::VarError::NotPresent) => Ok(ToolPath::default(root, &ctx_home(root)?)),
        Err(env::VarError::NotUnicode(_value)) => {
            Err(CliError::usage("CTX_PATH must be valid UTF-8"))
        }
    }
}

fn tool_path_error(error: cortexfs::ToolPathError) -> CliError {
    match error {
        cortexfs::ToolPathError::InvalidName => CliError::usage("invalid tool name"),
        cortexfs::ToolPathError::CannotReadDirectory => {
            CliError::unavailable("cannot read CTX_PATH directory")
        }
    }
}

fn is_mount_point(root: &Path) -> io::Result<bool> {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
    let root = absolute_existing_path(root)?.display().to_string();
    Ok(mountinfo
        .lines()
        .any(|line| mount_point(line).is_some_and(|point| point == root)))
}

fn absolute_existing_path(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path)
}

fn mount_point(line: &str) -> Option<String> {
    let mut fields = line.split(' ');
    let _id = fields.next()?;
    let _parent = fields.next()?;
    let _major_minor = fields.next()?;
    let _root = fields.next()?;
    fields.next().map(unescape_mountinfo)
}

fn unescape_mountinfo(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn shell_quote(value: &str) -> String {
    let value = terminal_safe_text(value);
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b':')
    }) {
        value
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD_CYAN: &str = "\x1b[1;36m";
const ANSI_BOLD_BLUE: &str = "\x1b[1;34m";
const ANSI_BOLD_YELLOW: &str = "\x1b[1;33m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_BLUE: &str = "\x1b[34m";
const ANSI_DIM: &str = "\x1b[2m";

fn color_enabled() -> bool {
    if env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if env::var_os("CLICOLOR_FORCE").is_some_and(|value| value != "0") {
        return true;
    }
    io::stdout().is_terminal()
}

fn styled(enabled: bool, style: &str, text: &str) -> String {
    if enabled {
        format!("{style}{text}{ANSI_RESET}")
    } else {
        text.to_owned()
    }
}

fn print_help_lines(lines: &[&str]) -> Result<(), CliError> {
    let color = color_enabled();
    for line in lines {
        print_line(&help_line(color, line))?;
    }
    Ok(())
}

fn help_line(color: bool, line: &str) -> String {
    if line.is_empty() {
        return String::new();
    }
    if line == "ctx - CortexFS filesystem management CLI" {
        return styled(color, ANSI_BOLD_CYAN, line);
    }
    if line.ends_with(':') && !line.starts_with(' ') {
        return styled(color, ANSI_BOLD_YELLOW, line);
    }
    if line.trim_start().starts_with("ctx ") {
        return styled(color, ANSI_GREEN, line);
    }
    styled(color, ANSI_DIM, line)
}

fn print_status_field(color: bool, label: &str, value: &str) -> Result<(), CliError> {
    print_line(&format!(
        "{} {value}",
        styled(color, ANSI_BOLD_BLUE, label)
    ))
}

fn status_state_value(color: bool, value: &str) -> String {
    let style = match value {
        "running" | "ready" => ANSI_GREEN,
        "available" | "unknown" => ANSI_YELLOW,
        "invalid" | "missing" | "failed" | "error" => ANSI_RED,
        _ => ANSI_CYAN,
    };
    styled(color, style, value)
}

fn status_bool_value(color: bool, value: &str, ok: bool) -> String {
    styled(color, if ok { ANSI_GREEN } else { ANSI_RED }, value)
}

fn status_tree_line(color: bool, line: &str) -> String {
    let line = line
        .replace("[idle]", &styled(color, ANSI_DIM, "[idle]"))
        .replace("[running]", &styled(color, ANSI_GREEN, "[running]"))
        .replace("[stopped]", &styled(color, ANSI_RED, "[stopped]"));
    styled(color, ANSI_CYAN, &line)
}

fn print_line(line: &str) -> Result<(), CliError> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(line.as_bytes())
        .and_then(|()| stdout.write_all(b"\n"))
        .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))
}
