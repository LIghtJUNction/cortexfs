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

    let model = read_agent_model_control_value(&control_dir, agent_name)?;
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
        if !is_valid_env_key(key) || value.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(AgentRuntimeViewError::InvalidControlFile("env".to_owned()));
        }
        env.push((key.to_owned(), value.to_owned()));
    }
    Ok(env)
}

include!("agent_secret_resolution.rs");

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

fn read_agent_model_control_value(
    control_dir: &Path,
    agent_name: &str,
) -> Result<String, AgentRuntimeViewError> {
    match read_required_agent_control_value(control_dir, "model") {
        Ok(model) => Ok(model),
        Err(AgentRuntimeViewError::MissingControlFile(_))
            if matches!(agent_name, "executor" | "worker")
                || agent_name.starts_with("executor-")
                || agent_name.starts_with("worker-") =>
        {
            Ok(DEFAULT_WORKER_MODEL.to_owned())
        }
        Err(error) => Err(error),
    }
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
