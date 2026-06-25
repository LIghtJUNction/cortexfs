fn append_jsonl_event(path: &Path, event: &str) -> std::io::Result<()> {
    append_jsonl_line(path, event)
}

fn append_jsonl_line(path: &Path, line: &str) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .append(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()
}

pub(crate) fn atomic_replace_text(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(content.as_bytes())?;
    temp.flush()?;
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    temp.persist(path)
        .map(|_file| ())
        .map_err(std::io::Error::from)
}

fn unix_timestamp_text() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    format!("{seconds}\n")
}

fn tool_path_denial(error: ToolPathError) -> ToolExecutionDenial {
    match error {
        ToolPathError::InvalidName => ToolExecutionDenial::InvalidToolName,
        ToolPathError::CannotReadDirectory => ToolExecutionDenial::CannotReadToolPath,
    }
}

fn symlink_safe_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "symlink authority target refused",
        ));
    }
    Ok(metadata)
}

fn linux_identity_can_execute(metadata: &fs::Metadata, identity: &AgentUnixIdentity) -> bool {
    let mode = metadata.permissions().mode();
    if identity.uid() == 0 {
        return mode & 0o111 != 0;
    }
    if metadata.uid() == identity.uid() {
        return mode & 0o100 != 0;
    }
    if identity.is_in_group(metadata.gid()) {
        return mode & 0o010 != 0;
    }
    mode & 0o001 != 0
}

fn linux_identity_can_read(metadata: &fs::Metadata, identity: &AgentUnixIdentity) -> bool {
    linux_identity_has_mode(metadata, identity, 0o400, 0o040, 0o004, 0o444)
}

fn linux_identity_can_write(metadata: &fs::Metadata, identity: &AgentUnixIdentity) -> bool {
    linux_identity_has_mode(metadata, identity, 0o200, 0o020, 0o002, 0o222)
}

fn linux_identity_has_mode(
    metadata: &fs::Metadata,
    identity: &AgentUnixIdentity,
    owner_bit: u32,
    group_bit: u32,
    other_bit: u32,
    root_mask: u32,
) -> bool {
    let mode = metadata.permissions().mode();
    if identity.uid() == 0 {
        return mode & root_mask != 0;
    }
    if metadata.uid() == identity.uid() {
        return mode & owner_bit != 0;
    }
    if identity.is_in_group(metadata.gid()) {
        return mode & group_bit != 0;
    }
    mode & other_bit != 0
}

fn most_specific_mount_for_path<'a>(
    mount_table: &'a MountTable,
    path: &Path,
) -> Option<&'a MountEntry> {
    mount_table
        .entries()
        .iter()
        .filter(|entry| path.starts_with(Path::new(entry.target())))
        .max_by_key(|entry| Path::new(entry.target()).components().count())
}

fn is_stable_shared_mount_for(mount: &MountEntry, shared_name: &str) -> bool {
    is_stable_shared_path_for(Path::new(mount.source()), shared_name)
        || is_stable_shared_path_for(Path::new(mount.target()), shared_name)
}

fn is_stable_shared_path_for(path: &Path, shared_name: &str) -> bool {
    let mut parts = path.components().filter_map(|component| match component {
        std::path::Component::Normal(value) => value.to_str(),
        std::path::Component::RootDir
        | std::path::Component::CurDir
        | std::path::Component::ParentDir
        | std::path::Component::Prefix(_) => None,
    });

    match (parts.next(), parts.next(), parts.next()) {
        (Some("ctx"), Some("shared"), Some(name)) | (Some("shared"), Some(name), _) => {
            name == shared_name
        }
        _ => false,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum MountedSessionPath {
    Private {
        uid: u32,
        agent: String,
        session: String,
    },
    Shared {
        shared: String,
        agent: String,
        session: String,
    },
}

impl MountedSessionPath {
    fn session_name(&self) -> &str {
        match *self {
            Self::Private { ref session, .. } | Self::Shared { ref session, .. } => session,
        }
    }

    fn shared_name(&self) -> Option<&str> {
        match *self {
            Self::Shared { ref shared, .. } => Some(shared),
            Self::Private { .. } => None,
        }
    }

    fn home_uid_allows(&self, identity: &AgentUnixIdentity) -> bool {
        match *self {
            Self::Private { uid, .. } => uid == identity.uid(),
            Self::Shared { .. } => true,
        }
    }
}

fn mounted_session_path(mount: &MountEntry, path: &Path) -> Option<MountedSessionPath> {
    let stable = mounted_stable_path(mount, path)?;
    parse_mounted_session_path(&stable)
}

fn mounted_stable_path(mount: &MountEntry, path: &Path) -> Option<PathBuf> {
    let target = Path::new(mount.target());
    if let Ok(relative) = path.strip_prefix(target) {
        return Some(Path::new(mount.source()).join(relative));
    }
    path.starts_with(Path::new(mount.source()))
        .then(|| path.to_path_buf())
}

fn parse_mounted_session_path(path: &Path) -> Option<MountedSessionPath> {
    let parts = stable_path_parts(path)?;
    match *parts.as_slice() {
        ["ctx", "home", uid, "agent", agent, "session", session, ..]
        | ["home", uid, "agent", agent, "session", session, ..] => {
            let uid = uid.parse::<u32>().ok()?;
            (is_object_name(agent) && is_object_name(session)).then(|| {
                MountedSessionPath::Private {
                    uid,
                    agent: (*agent).to_owned(),
                    session: (*session).to_owned(),
                }
            })
        }
        [
            "ctx",
            "shared",
            shared,
            "agent",
            agent,
            "session",
            session,
            ..,
        ]
        | ["shared", shared, "agent", agent, "session", session, ..] => (is_object_name(shared)
            && is_object_name(agent)
            && is_object_name(session))
        .then(|| MountedSessionPath::Shared {
            shared: (*shared).to_owned(),
            agent: (*agent).to_owned(),
            session: (*session).to_owned(),
        }),
        _ => None,
    }
}

fn stable_path_parts(path: &Path) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => parts.push(value.to_str()?),
            std::path::Component::RootDir => {}
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::Prefix(_) => return None,
        }
    }
    Some(parts)
}
