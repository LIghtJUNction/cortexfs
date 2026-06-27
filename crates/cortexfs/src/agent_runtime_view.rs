/// Derives an agent runtime view from the frozen v1 control files under
/// `ctx_root/agent/<agent_name>.d/`.
///
/// The returned environment always contains the runtime-owned `CTX_ROOT`,
/// `CTX_HOME`, `HOME`, and `CTX_PATH` values derived from the ABI controls.
/// The `env` control is validated as data, but v1 does not let it add process
/// variables. Runtime environment authority is fixed here and by the sandbox
/// launcher so text config cannot expand the authority established by `path`,
/// `mount`, and `policy`.
const MAX_AGENT_RUNTIME_CONTROL_BYTES: u64 = 64 * 1024;

pub fn derive_agent_runtime_view(
    ctx_root: &Path,
    agent_name: &str,
) -> Result<AgentRuntimeView, AgentRuntimeViewError> {
    if !is_object_name(agent_name) {
        return Err(AgentRuntimeViewError::InvalidAgentName);
    }

    let control_dir = ctx_root.join("agent").join(format!("{agent_name}.d"));
    if open_agent_runtime_plain_directory(&control_dir).is_err() {
        return Err(AgentRuntimeViewError::MissingControlDirectory);
    }

    let owner = parse_agent_number_control(&control_dir, AgentControlKind::Owner, "owner")?;
    let uid = parse_agent_number_control(&control_dir, AgentControlKind::Uid, "uid")?;
    let gid = parse_agent_number_control(&control_dir, AgentControlKind::Gid, "gid")?;
    let groups = parse_agent_groups_control(&control_dir)?;
    let identity = AgentUnixIdentity::new(uid, gid, groups);

    let label = read_required_agent_control_value(&control_dir, "label")?;
    let policy_subject = policy_subject_from_label(&label)
        .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile("label".to_owned()))?
        .to_owned();

    let iso = parse_agent_vocab_value(&control_dir, AgentControlKind::Iso, "iso")?;
    let parent = parse_agent_parent_control(&control_dir)?;
    let lifecycle = ChildLifecycle::parse(&parse_agent_vocab_value(
        &control_dir,
        AgentControlKind::Life,
        "life",
    )?)
    .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("life".to_owned()))?;

    let root = parse_agent_absolute_path_control(&control_dir, "root")?;
    let cwd = parse_agent_absolute_path_control(&control_dir, "cwd")?;

    let raw_path = read_required_agent_control_value(&control_dir, "path")?;
    validate_agent_ctx_path(&raw_path)?;
    let tool_path = ToolPath::parse(&raw_path);

    let mount_content = read_required_agent_control(&control_dir, "mount")?;
    let mount_table = MountTable::parse(&mount_content)
        .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("mount".to_owned()))?;

    let model = read_required_agent_control_value(&control_dir, "model")?;
    if !abi_path::is_model_reference(&model) {
        return Err(AgentRuntimeViewError::InvalidControlFile(
            "model".to_owned(),
        ));
    }

    let policy_content = read_required_agent_control(&control_dir, "policy")?;
    let policy = PolicyV0::parse(&policy_content)
        .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("policy".to_owned()))?;

    let ctx_home = ctx_root.join("home").join(owner.to_string());
    let home = ctx_home.join("agent").join(agent_name);
    let env = derive_agent_runtime_env(ctx_root, &ctx_home, &home, &raw_path, &control_dir)?;

    Ok(AgentRuntimeView {
        agent_name: agent_name.to_owned(),
        control_dir,
        ctx_root: ctx_root.to_path_buf(),
        ctx_home,
        home,
        owner,
        identity,
        label,
        policy_subject,
        iso,
        parent,
        lifecycle,
        root,
        cwd,
        env,
        tool_path,
        mount_table,
        model,
        policy,
    })
}

fn parse_agent_number_control(
    control_dir: &Path,
    kind: AgentControlKind,
    file: &str,
) -> Result<u32, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, file)?;
    if !inspect_agent_control(kind, &content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    required_single_agent_control_value(file, &content)?
        .parse::<u32>()
        .map_err(|_error| AgentRuntimeViewError::InvalidControlFile(file.to_owned()))
}

fn parse_agent_groups_control(control_dir: &Path) -> Result<Vec<u32>, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, "groups")?;
    if !inspect_agent_control(AgentControlKind::Groups, &content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(
            "groups".to_owned(),
        ));
    }
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.parse::<u32>()
                .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("groups".to_owned()))
        })
        .collect()
}

fn parse_agent_vocab_value(
    control_dir: &Path,
    kind: AgentControlKind,
    file: &str,
) -> Result<String, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, file)?;
    if !inspect_agent_control(kind, &content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    required_single_agent_control_value(file, &content)
}

fn parse_agent_parent_control(control_dir: &Path) -> Result<Option<String>, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, "parent")?;
    if !inspect_agent_control(AgentControlKind::Parent, &content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(
            "parent".to_owned(),
        ));
    }
    let value = optional_single_agent_control_value("parent", &content)?;
    Ok(value.filter(|parent| !parent.is_empty()))
}

fn parse_agent_absolute_path_control(
    control_dir: &Path,
    file: &str,
) -> Result<PathBuf, AgentRuntimeViewError> {
    let value = read_required_agent_control_value(control_dir, file)?;
    if !is_stable_chroot_absolute_path(&value) {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    Ok(PathBuf::from(value))
}

fn derive_agent_runtime_env(
    ctx_root: &Path,
    ctx_home: &Path,
    home: &Path,
    ctx_path: &str,
    control_dir: &Path,
) -> Result<Vec<(String, String)>, AgentRuntimeViewError> {
    let env_content = read_required_agent_control(control_dir, "env")?;
    let env = vec![
        ("CTX_ROOT".to_owned(), ctx_root.display().to_string()),
        ("CTX_HOME".to_owned(), ctx_home.display().to_string()),
        ("HOME".to_owned(), home.display().to_string()),
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        ("CTX_PATH".to_owned(), ctx_path.to_owned()),
    ];
    let _validated_env = parse_agent_env_control(&env_content)?;
    Ok(env)
}

fn parse_agent_env_control(content: &str) -> Result<Vec<(String, String)>, AgentRuntimeViewError> {
    let mut env = Vec::new();
    for raw_line in content.lines().filter(|line| !line.trim().is_empty()) {
        let (key, value) = raw_line
            .split_once('=')
            .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile("env".to_owned()))?;
        if !is_valid_env_key(key) || value.contains('\0') {
            return Err(AgentRuntimeViewError::InvalidControlFile("env".to_owned()));
        }
        env.push((key.to_owned(), value.to_owned()));
    }
    Ok(env)
}

/// Resolves an API key with the stable priority: environment, system keychain,
/// then unconfigured.
pub fn resolve_api_key(
    env_name: &str,
    service: &str,
    account: &str,
) -> Result<Option<String>, ApiKeyResolutionError> {
    resolve_api_key_from_env_names(&[env_name.to_owned()], service, account)
}

/// Resolves an API key from candidate environment variables with the stable
/// priority: environment, system keychain, then unconfigured.
pub fn resolve_api_key_from_env_names(
    env_names: &[String],
    service: &str,
    account: &str,
) -> Result<Option<String>, ApiKeyResolutionError> {
    resolve_api_key_from_env_names_with(
        env_names,
        service,
        account,
        |name| env::var(name),
        system_keychain_secret,
    )
}

/// Testable core for API key resolution from multiple environment candidates.
pub fn resolve_api_key_from_env_names_with<E, K>(
    env_names: &[String],
    service: &str,
    account: &str,
    env_lookup: E,
    keychain_lookup: K,
) -> Result<Option<String>, ApiKeyResolutionError>
where
    E: Fn(&str) -> Result<String, env::VarError>,
    K: FnOnce(&str, &str) -> Result<Option<String>, ApiKeyResolutionError>,
{
    if env_names.iter().any(|name| !is_valid_env_key(name))
        || !is_valid_secret_lookup_part(service)
        || !is_valid_secret_lookup_part(account)
    {
        return Err(ApiKeyResolutionError::InvalidName);
    }
    for env_name in env_names {
        match env_lookup(env_name) {
            Ok(value) if !value.trim().is_empty() => return Ok(Some(value)),
            Ok(_value) => {}
            Err(env::VarError::NotPresent) => {}
            Err(env::VarError::NotUnicode(_value)) => {
                return Err(ApiKeyResolutionError::InvalidName);
            }
        }
    }
    keychain_lookup(service, account)
}

/// Testable core for API key resolution.
pub fn resolve_api_key_with<E, K>(
    env_name: &str,
    service: &str,
    account: &str,
    env_lookup: E,
    keychain_lookup: K,
) -> Result<Option<String>, ApiKeyResolutionError>
where
    E: FnOnce(&str) -> Result<String, env::VarError>,
    K: FnOnce(&str, &str) -> Result<Option<String>, ApiKeyResolutionError>,
{
    if !is_valid_env_key(env_name)
        || !is_valid_secret_lookup_part(service)
        || !is_valid_secret_lookup_part(account)
    {
        return Err(ApiKeyResolutionError::InvalidName);
    }
    match env_lookup(env_name) {
        Ok(value) if !value.trim().is_empty() => return Ok(Some(value)),
        Ok(_value) => {}
        Err(env::VarError::NotPresent) => {}
        Err(env::VarError::NotUnicode(_value)) => {
            return Err(ApiKeyResolutionError::InvalidName);
        }
    }
    keychain_lookup(service, account)
}

fn system_keychain_secret(
    service: &str,
    account: &str,
) -> Result<Option<String>, ApiKeyResolutionError> {
    let entry = match keyring::Entry::new(service, account) {
        Ok(entry) => entry,
        Err(keyring::Error::NoDefaultStore) => return secret_tool_lookup(service, account),
        Err(_error) => return secret_tool_lookup(service, account),
    };
    let secret = match entry.get_password() {
        Ok(secret) => secret,
        Err(keyring::Error::NoEntry) => return secret_tool_lookup(service, account),
        Err(_error) => return secret_tool_lookup(service, account),
    };
    if secret.is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret))
    }
}

const SECRET_TOOL_PROGRAM: &str = "/usr/bin/secret-tool";
const MAX_SECRET_TOOL_OUTPUT_BYTES: usize = 8 * 1024;
const SECRET_TOOL_TIMEOUT_SECONDS: u64 = 5;

fn get_secret_tool_program() -> &'static str {
    SECRET_TOOL_PROGRAM
}

fn secret_tool_lookup(
    service: &str,
    account: &str,
) -> Result<Option<String>, ApiKeyResolutionError> {
    let mut command = Command::new(get_secret_tool_program());
    command
        .env_clear()
        .args(["lookup", "service", service, "account", account])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command.env(
        "DBUS_SESSION_BUS_ADDRESS",
        secret_tool_dbus_address(|name| env::var_os(name), nix::unistd::geteuid().as_raw()),
    );
    let output = match run_secret_tool_command_with_timeout(
        command,
        Duration::from_secs(SECRET_TOOL_TIMEOUT_SECONDS),
    ) {
        Ok(output) => output,
        Err(_error) => return Ok(None),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let secret =
        String::from_utf8(output.stdout).map_err(|_error| ApiKeyResolutionError::InvalidName)?;
    let secret = secret.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret.to_owned()))
    }
}

fn run_secret_tool_command_with_timeout(
    mut command: Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot run secret-tool: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cannot read secret-tool stdout".to_owned())?;
    let stdout_reader = thread::spawn(move || {
        read_agent_runtime_limited_bytes(stdout, MAX_SECRET_TOOL_OUTPUT_BYTES.saturating_add(1))
    });
    let mut stdout_reader = Some(stdout_reader);
    let mut stdout = None;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if stdout.is_none()
            && stdout_reader
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
        {
            let output = stdout_reader
                .take()
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default();
            if output.len() > MAX_SECRET_TOOL_OUTPUT_BYTES {
                terminate_agent_runtime_process_group(&mut child);
                let _ignored = child.wait();
                return Err("secret-tool output exceeds limit".to_owned());
            }
            stdout = Some(output);
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            terminate_agent_runtime_process_group(&mut child);
            let _ignored = child.wait();
            if let Some(reader) = stdout_reader.take() {
                let _ignored = reader.join();
            }
            return Err(format!("secret-tool timed out after {}s", timeout.as_secs()));
        }
        thread::sleep(Duration::from_millis(50));
    };
    let stdout = stdout.unwrap_or_else(|| {
        stdout_reader
            .take()
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default()
    });
    if stdout.len() > MAX_SECRET_TOOL_OUTPUT_BYTES {
        return Err("secret-tool output exceeds limit".to_owned());
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr: Vec::new(),
    })
}

fn read_agent_runtime_limited_bytes(mut reader: impl Read, limit: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(limit.min(8 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let remaining = limit.saturating_sub(output.len());
        let kept = read.min(remaining);
        if let Some(chunk) = buffer.get(..kept) {
            output.extend_from_slice(chunk);
        }
        if output.len() >= limit {
            break;
        }
    }
    output
}

fn terminate_agent_runtime_process_group(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        signal_agent_runtime_process_group(pid, nix::sys::signal::Signal::SIGTERM);
        for _attempt in 0..5 {
            let _ignored = child.try_wait();
            thread::sleep(Duration::from_millis(50));
        }
        signal_agent_runtime_process_group(pid, nix::sys::signal::Signal::SIGKILL);
    }
    let _ignored = child.kill();
}

fn signal_agent_runtime_process_group(pid: i32, signal: nix::sys::signal::Signal) {
    let _ignored = nix::sys::signal::kill(nix::unistd::Pid::from_raw(-pid), signal);
}

fn secret_tool_dbus_address(
    get_env: impl FnOnce(&str) -> Option<std::ffi::OsString>,
    uid: u32,
) -> std::ffi::OsString {
    get_env("DBUS_SESSION_BUS_ADDRESS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("unix:path=/run/user/{uid}/bus").into())
}

fn is_valid_secret_lookup_part(value: &str) -> bool {
    !value.is_empty() && !value.contains('\0') && !value.contains('\n')
}

fn validate_agent_ctx_path(value: &str) -> Result<(), AgentRuntimeViewError> {
    if value
        .split(':')
        .filter(|component| !component.is_empty())
        .all(is_stable_chroot_absolute_path)
    {
        Ok(())
    } else {
        Err(AgentRuntimeViewError::InvalidControlFile("path".to_owned()))
    }
}

fn read_required_agent_control_value(
    control_dir: &Path,
    file: &str,
) -> Result<String, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, file)?;
    required_single_agent_control_value(file, &content)
}

fn read_required_agent_control(
    control_dir: &Path,
    file: &str,
) -> Result<String, AgentRuntimeViewError> {
    let path = control_dir.join(file);
    read_agent_runtime_control_file(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AgentRuntimeViewError::MissingControlFile(file.to_owned())
        } else {
            AgentRuntimeViewError::CannotReadControl(file.to_owned())
        }
    })
}

fn read_agent_runtime_control_file(path: &Path) -> std::io::Result<String> {
    let mut file = open_agent_runtime_plain_file(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    if metadata.len() > MAX_AGENT_RUNTIME_CONTROL_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "agent runtime control file exceeds read limit",
        ));
    }
    let len = usize::try_from(metadata.len()).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("file is too large to read: {error}"),
        )
    })?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error()))
}

fn open_agent_runtime_plain_file(path: &Path) -> std::io::Result<fs::File> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_agent_runtime_plain_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid file name")
        })?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    Ok(fs::File::from(file_fd))
}

fn open_agent_runtime_plain_directory(path: &Path) -> std::io::Result<fs::File> {
    let mut directory = if path.is_absolute() {
        open_agent_runtime_single_plain_directory(Path::new("/"))?
    } else {
        open_agent_runtime_single_plain_directory(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid directory name")
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )
                .map_err(std::io::Error::from)?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_agent_runtime_single_plain_directory(path: &Path) -> std::io::Result<fs::File> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}

fn required_single_agent_control_value(
    file: &str,
    content: &str,
) -> Result<String, AgentRuntimeViewError> {
    optional_single_agent_control_value(file, content)?
        .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile(file.to_owned()))
}

fn optional_single_agent_control_value(
    file: &str,
    content: &str,
) -> Result<Option<String>, AgentRuntimeViewError> {
    let lines = content.lines().collect::<Vec<_>>();
    if lines.len() > 1 {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    let Some(raw_value) = lines.first() else {
        return Ok(None);
    };
    let value = raw_value.trim();
    if *raw_value != value || value.contains('\0') {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value.to_owned()))
    }
}
