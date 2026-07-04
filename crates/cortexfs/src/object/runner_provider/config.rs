fn provider_config(provider: &str) -> Option<RunnerProviderConfig> {
    let config_dir = env::var_os("CTX_PROVIDER_CONFIG_DIR")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(RUNNER_PROVIDER_CONFIG_DIR), PathBuf::from);
    provider_config_from_dir(&config_dir, provider)
}

fn provider_config_from_model_control(
    ctx_root: &Path,
    provider: &str,
    model: &str,
) -> Option<RunnerProviderConfig> {
    let control = ctx_root.join("model").join(provider).join(format!("{model}.d"));
    let default = read_small_plain_text_file(&control.join("default")).ok()?;
    let base_url = model_default_base_url(&default)?;
    let driver = read_small_plain_text_file(&control.join("driver")).unwrap_or_default();
    Some(RunnerProviderConfig {
        name: Some(provider.to_owned()),
        base_url,
        oauth: None,
        formats: model_driver_formats(&driver),
    })
}

fn model_default_base_url(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let line = line.split_once('#').map_or(line, |(value, _comment)| value).trim();
        let value = line.strip_prefix("base_url=")?.trim();
        (!value.is_empty()).then_some(value.to_owned())
    })
}

fn model_driver_formats(content: &str) -> Vec<String> {
    let mut formats = Vec::new();
    if content.contains("openai.chat") || content.contains("openai-chat") {
        formats.push("openai.chat".to_owned());
    }
    if content.contains("openai.responses") || content.contains("openai-responses") {
        formats.push("openai.responses".to_owned());
    }
    if formats.is_empty() {
        formats.push("openai.chat".to_owned());
    }
    formats
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

#[cfg(test)]
mod runner_provider_config_tests {
    use super::*;

    #[test]
    fn provider_config_can_fall_back_to_model_control_files(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let control = root.path().join("model/api.test/gpt-5.4-mini.d");
        fs::create_dir_all(&control)?;
        fs::write(control.join("default"), "base_url=https://api.test/v1\n")?;
        fs::write(
            control.join("driver"),
            "default=openai-chat\nagent=openai-responses,openai-chat\n",
        )?;

        let config = provider_config_from_model_control(root.path(), "api.test", "gpt-5.4-mini")
            .ok_or_else(|| io::Error::other("missing fallback provider config"))?;

        assert_eq!(config.name.as_deref(), Some("api.test"));
        assert_eq!(config.base_url, "https://api.test/v1");
        assert_eq!(config.formats, ["openai.chat", "openai.responses"]);
        Ok(())
    }
}
