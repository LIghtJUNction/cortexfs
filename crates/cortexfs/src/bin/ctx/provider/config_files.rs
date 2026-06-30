const PROVIDER_CONFIG_DIR: &str = "/etc/cortexfs/providers.d";
const MAX_CTX_PROVIDER_CONFIG_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Deserialize)]
struct CtxProviderConfig {
    name: Option<String>,
    base_url: String,
    oauth: Option<cortexfs::OAuthProviderConfig>,
}

fn atomic_write_provider_config(path: &Path, content: &str) -> Result<(), CliError> {
    let parent = path
        .parent()
        .ok_or_else(|| CliError::unavailable("provider config path has no parent"))?;
    let parent_dir = open_provider_config_dir(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::unavailable("provider config path has no file name"))?;
    for attempt in 0..16 {
        let temp_name = temp_file_name(attempt);
        let file_fd = match nix::fcntl::openat(
            &parent_dir,
            temp_name.as_str(),
            nix::fcntl::OFlag::O_CREAT
                | nix::fcntl::OFlag::O_EXCL
                | nix::fcntl::OFlag::O_WRONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::from_bits_truncate(0o600),
        ) {
            Ok(file_fd) => file_fd,
            Err(nix::errno::Errno::EEXIST) => continue,
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot write provider config: {error}"
                )));
            }
        };
        let mut temp = fs::File::from(file_fd);
        temp.write_all(content.as_bytes())
            .and_then(|()| temp.flush())
            .and_then(|()| temp.sync_all())
            .map_err(|error| {
                let _ignored = nix::unistd::unlinkat(
                    &parent_dir,
                    temp_name.as_str(),
                    nix::unistd::UnlinkatFlags::NoRemoveDir,
                );
                CliError::unavailable(format!("cannot write provider config: {error}"))
            })?;
        drop(temp);
        nix::fcntl::renameat(&parent_dir, temp_name.as_str(), &parent_dir, file_name).map_err(
            |error| {
                let _ignored = nix::unistd::unlinkat(
                    &parent_dir,
                    temp_name.as_str(),
                    nix::unistd::UnlinkatFlags::NoRemoveDir,
                );
                CliError::unavailable(format!("cannot install provider config: {error}"))
            },
        )?;
        return parent_dir.sync_all().map_err(|error| {
            CliError::unavailable(format!("cannot sync provider config dir: {error}"))
        });
    }
    Err(CliError::unavailable(
        "cannot create unique provider config temp file",
    ))
}

fn create_provider_config_dir(path: &Path) -> Result<(), CliError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            sync_provider_config_dir(path)
        } else {
            Err(CliError::unavailable(
                "provider config directory is not a plain directory",
            ))
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(CliError::unavailable(
                    "provider config path contains a non-directory entry",
                ));
            }
            Ok(_metadata) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot inspect provider config dir: {error}"
                )));
            }
        }
    }

    let existing_parent = missing
        .last()
        .and_then(|path| path.parent())
        .ok_or_else(|| CliError::unavailable("invalid provider config dir"))?;
    let mut parent_dir = open_provider_config_dir(existing_parent)?;
    for directory in missing.iter().rev() {
        let name = provider_config_file_name(directory)?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o755),
        )
        .map_err(|error| {
            CliError::unavailable(format!("cannot create provider config dir: {error}"))
        })?;
        parent_dir.sync_all().map_err(|error| {
            CliError::unavailable(format!("cannot sync provider config dir: {error}"))
        })?;
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(|error| {
            CliError::unavailable(format!("cannot open provider config dir: {error}"))
        })?;
        parent_dir = fs::File::from(child);
        parent_dir.sync_all().map_err(|error| {
            CliError::unavailable(format!("cannot sync provider config dir: {error}"))
        })?;
    }
    Ok(())
}

fn sync_provider_config_dir(path: &Path) -> Result<(), CliError> {
    let directory = open_provider_config_dir(path)?;
    directory
        .sync_all()
        .map_err(|error| CliError::unavailable(format!("cannot sync provider config dir: {error}")))
}

fn open_provider_config_dir(path: &Path) -> Result<fs::File, CliError> {
    let mut directory = if path.is_absolute() {
        open_single_provider_config_dir(Path::new("/"))?
    } else {
        open_single_provider_config_dir(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    CliError::unavailable(format!(
                        "cannot open provider config dir {}: invalid directory name",
                        path.display()
                    ))
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
                .map_err(|error| {
                    CliError::unavailable(format!(
                        "cannot open provider config dir {}: {error}",
                        path.display()
                    ))
                })?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(CliError::unavailable(format!(
                    "cannot open provider config dir {}: unsupported path component",
                    path.display()
                )));
            }
        }
    }
    Ok(directory)
}

fn open_single_provider_config_dir(path: &Path) -> Result<fs::File, CliError> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| {
            CliError::unavailable(format!("cannot open provider config dir: {error}"))
        })?;
    if !directory
        .metadata()
        .map_err(|error| {
            CliError::unavailable(format!("cannot inspect provider config dir: {error}"))
        })?
        .is_dir()
    {
        return Err(CliError::unavailable(
            "provider config path is not a directory",
        ));
    }
    Ok(directory)
}

fn provider_config_file_name(path: &Path) -> Result<&str, CliError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::unavailable("invalid provider config path"))
}
fn read_provider_config(provider: &str) -> Result<CtxProviderConfig, CliError> {
    read_provider_config_from_dir(provider, Path::new(PROVIDER_CONFIG_DIR))
}

fn read_provider_config_from_dir(
    provider: &str,
    dir: &Path,
) -> Result<CtxProviderConfig, CliError> {
    let directory = open_provider_config_dir(dir)?;
    let entries = fs::read_dir(provider_config_proc_fd_path(&directory)).map_err(|error| {
        CliError::unavailable(format!("cannot read provider config dir: {error}"))
    })?;
    for entry in entries {
        let entry = entry
            .map_err(|error| CliError::unavailable(format!("cannot read provider config: {error}")))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if Path::new(file_name)
            .extension()
            .and_then(|value| value.to_str())
            != Some("json")
        {
            continue;
        }
        let content = read_provider_config_file_at(&directory, file_name)?;
        let config = serde_json::from_str::<CtxProviderConfig>(&content)
            .map_err(|error| CliError::usage(format!("invalid provider config: {error}")))?;
        if cortexfs::provider_name_from_config(&config.base_url, config.name.as_deref())
            .as_deref()
            == Ok(provider)
        {
            return Ok(config);
        }
    }
    Err(CliError::usage(format!("missing provider: {provider}")))
}

fn provider_config_proc_fd_path(directory: &fs::File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()))
}

fn read_provider_config_file_at(parent_dir: &fs::File, file_name: &str) -> Result<String, CliError> {
    let file_fd = nix::fcntl::openat(
        parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|error| CliError::unavailable(format!("cannot read provider config: {error}")))?;
    read_provider_config_open_file(fs::File::from(file_fd), "provider config")
}

#[cfg(test)]
fn read_provider_config_file(path: &Path) -> Result<String, CliError> {
    let Some(parent) = path.parent() else {
        return Err(CliError::unavailable("provider config path has no parent"));
    };
    let parent_dir = open_provider_config_dir(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::unavailable("provider config path has no file name"))?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|error| {
        CliError::unavailable(format!("cannot read provider config {}: {error}", path.display()))
    })?;
    read_provider_config_open_file(fs::File::from(file_fd), &path.display().to_string())
}

fn read_provider_config_open_file(mut file: fs::File, label: &str) -> Result<String, CliError> {
    let metadata = file.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot inspect provider config {label}: {error}"))
    })?;
    if !metadata.is_file() {
        return Err(CliError::unavailable(format!(
            "provider config is not a regular file: {label}",
        )));
    }
    if metadata.len() > MAX_CTX_PROVIDER_CONFIG_BYTES {
        return Err(CliError::unavailable(format!(
            "provider config is too large: {label}",
        )));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|_error| CliError::unavailable("provider config is too large"))?;
    let mut content = vec![0; len];
    file.read_exact(&mut content).map_err(|error| {
        CliError::unavailable(format!("cannot read provider config {label}: {error}"))
    })?;
    String::from_utf8(content)
        .map_err(|_error| CliError::usage(format!("provider config is not utf-8: {label}")))
}
