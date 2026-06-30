fn provider_config(provider: &str) -> Option<RunnerProviderConfig> {
    provider_config_from_dir(Path::new(RUNNER_PROVIDER_CONFIG_DIR), provider)
}

fn provider_config_from_dir(config_dir: &Path, provider: &str) -> Option<RunnerProviderConfig> {
    let directory = open_runner_provider_config_dir(config_dir).ok()?;
    let entries = fs::read_dir(runner_provider_proc_fd_path(&directory)).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().into_string().ok()?;
        if Path::new(&name).extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let content = read_runner_provider_config_file(&directory, &name).ok()?;
        let config = serde_json::from_str::<RunnerProviderConfig>(&content).ok()?;
        if provider_name_from_config(&config.base_url, config.name.as_deref()).as_deref()
            != Ok(provider)
        {
            continue;
        }
        return Some(config);
    }
    None
}

fn read_runner_provider_config_file(directory: &fs::File, name: &str) -> io::Result<String> {
    let fd = nix::fcntl::openat(
        directory,
        name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(io::Error::from)?;
    let mut file = fs::File::from(fd);
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_RUNNER_PROVIDER_CONFIG_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "provider config file is invalid",
        ));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    read_utf8_exact_len(&mut file, len)
}

fn runner_provider_proc_fd_path(directory: &fs::File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()))
}

fn open_runner_provider_config_dir(config_dir: &Path) -> io::Result<fs::File> {
    let mut directory = if config_dir.is_absolute() {
        open_runner_provider_config_dir_leaf(Path::new("/"))?
    } else {
        open_runner_provider_config_dir_leaf(Path::new("."))?
    };
    for component in config_dir.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid provider config dir")
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
                .map_err(io::Error::from)?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "provider config dir contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_runner_provider_config_dir_leaf(path: &Path) -> io::Result<fs::File> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "provider config dir is not a directory",
        ));
    }
    Ok(directory)
}
