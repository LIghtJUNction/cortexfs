use crate::*;

impl FuseProjection {
    /// Persists one owner-authorized agent socket placeholder.
    pub fn create_socket_placeholder(
        &self,
        abi_path: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<FuseNode, FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let Some(SocketAlias::Agent { agent }) = SocketAlias::parse(&normalized) else {
            return Err(FuseError::NotControlFile);
        };
        self.authorize_agent_owner(agent, uid)?;
        let path = self.resolve(&normalized)?;
        let parent = path.parent().ok_or(FuseError::InvalidPath)?;
        let parent_dir = open_plain_directory(parent).map_err(|_error| FuseError::Io)?;
        let name = plain_file_name(&path).map_err(|_error| FuseError::InvalidPath)?;
        let created = support::plain::ensure_socket_placeholder(&path, mode)
            .map_err(|_error| FuseError::Io)?;
        if chown_socket_entry(&parent_dir, name, uid, gid).is_err() {
            if created {
                let _ignored = self.remove_socket_alias(&normalized, uid);
            }
            return Err(FuseError::Io);
        }
        match self.node_for_path(&normalized) {
            Ok(node) => Ok(node),
            Err(error) => {
                if created {
                    let _ignored = self.remove_socket_alias(&normalized, uid);
                }
                Err(error)
            }
        }
    }

    /// Applies mode bits to one owner-authorized plain socket placeholder.
    pub fn set_socket_placeholder_mode(
        &self,
        abi_path: &str,
        uid: u32,
        mode: u32,
    ) -> Result<(), FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let alias = SocketAlias::parse(&normalized).ok_or(FuseError::NotControlFile)?;
        alias.authorize(self, uid)?;
        let path = self.resolve(&normalized)?;
        let parent = path.parent().ok_or(FuseError::InvalidPath)?;
        let parent_dir = open_plain_directory(parent).map_err(|_error| FuseError::Io)?;
        let name = plain_file_name(&path).map_err(|_error| FuseError::InvalidPath)?;
        let stat =
            nix::sys::stat::fstatat(&parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(|_error| FuseError::Io)?;
        if !nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
            .contains(nix::sys::stat::SFlag::S_IFSOCK)
        {
            return Err(FuseError::InvalidPath);
        }
        if stat.st_uid != uid {
            return Err(FuseError::PermissionDenied);
        }
        nix::sys::stat::fchmodat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(mode & 0o7777),
            nix::sys::stat::FchmodatFlags::NoFollowSymlink,
        )
        .map_err(|_error| FuseError::Io)?;
        parent_dir.sync_all().map_err(|_error| FuseError::Io)
    }

    /// Persists one owner-authorized runtime socket alias.
    pub fn set_socket_alias(
        &self,
        abi_path: &str,
        target: &Path,
        uid: u32,
        gid: u32,
    ) -> Result<FuseNode, FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let alias = SocketAlias::parse(&normalized).ok_or(FuseError::NotControlFile)?;
        alias.authorize(self, uid)?;
        alias.validate_target(target, uid)?;
        let path = self.resolve(&normalized)?;
        let parent = path.parent().ok_or(FuseError::InvalidPath)?;
        let parent_dir = open_plain_directory(parent).map_err(|_error| FuseError::Io)?;
        let name = plain_file_name(&path).map_err(|_error| FuseError::InvalidPath)?;
        let created = match nix::fcntl::readlinkat(&parent_dir, name).map(PathBuf::from) {
            Ok(existing) if existing == target => false,
            Ok(_existing) => return Err(FuseError::InvalidPath),
            Err(nix::errno::Errno::ENOENT) => {
                nix::unistd::symlinkat(target, &parent_dir, name)
                    .map_err(|_error| FuseError::Io)?;
                true
            }
            Err(nix::errno::Errno::EINVAL) => return Err(FuseError::InvalidPath),
            Err(_error) => return Err(FuseError::Io),
        };
        if chown_socket_entry(&parent_dir, name, uid, gid).is_err() {
            remove_created_socket_entry(&parent_dir, name, created);
            return Err(FuseError::Io);
        }
        if parent_dir.sync_all().is_err() {
            remove_created_socket_entry(&parent_dir, name, created);
            return Err(FuseError::Io);
        }
        match self.node_for_path(&normalized) {
            Ok(node) => Ok(node),
            Err(error) => {
                remove_created_socket_entry(&parent_dir, name, created);
                Err(error)
            }
        }
    }

    /// Removes one owner-authorized runtime socket alias or placeholder.
    pub fn remove_socket_alias(&self, abi_path: &str, uid: u32) -> Result<(), FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let alias = SocketAlias::parse(&normalized).ok_or(FuseError::NotControlFile)?;
        alias.authorize(self, uid)?;
        let path = self.resolve(&normalized)?;
        let parent = path.parent().ok_or(FuseError::InvalidPath)?;
        let parent_dir = open_plain_directory(parent).map_err(|_error| FuseError::Io)?;
        let name = plain_file_name(&path).map_err(|_error| FuseError::InvalidPath)?;
        let stat = match nix::sys::stat::fstatat(
            &parent_dir,
            name,
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        ) {
            Ok(stat) => stat,
            Err(nix::errno::Errno::ENOENT) => return Ok(()),
            Err(_error) => return Err(FuseError::Io),
        };
        let kind = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode);
        if kind != nix::sys::stat::SFlag::S_IFLNK && kind != nix::sys::stat::SFlag::S_IFSOCK {
            return Err(FuseError::InvalidPath);
        }
        nix::unistd::unlinkat(&parent_dir, name, nix::unistd::UnlinkatFlags::NoRemoveDir)
            .map_err(|_error| FuseError::Io)?;
        parent_dir.sync_all().map_err(|_error| FuseError::Io)
    }

    /// Returns whether the path is one of the two writable socket-alias shapes.
    #[doc(hidden)]
    #[must_use]
    pub fn is_socket_alias_path(abi_path: &str) -> bool {
        SocketAlias::parse(abi_path).is_some()
    }

    /// Returns whether the path is a generated claim for a writable socket alias.
    #[doc(hidden)]
    #[must_use]
    pub fn is_socket_alias_claim_path(abi_path: &str) -> bool {
        socket_alias_for_claim(abi_path).is_some()
    }

    /// Validates ownership for one writable socket-alias shape.
    #[doc(hidden)]
    pub fn authorize_socket_alias(&self, abi_path: &str, uid: u32) -> Result<(), FuseError> {
        SocketAlias::parse(abi_path)
            .ok_or(FuseError::NotControlFile)?
            .authorize(self, uid)
    }

    /// Atomically moves one owner socket alias to or from its generated claim.
    #[doc(hidden)]
    pub fn rename_socket_alias_claim(
        &self,
        from: &str,
        to: &str,
        uid: u32,
    ) -> Result<(), FuseError> {
        let from = normalize_fuse_abi_path(from)?;
        let to = normalize_fuse_abi_path(to)?;
        let (alias_path, claim_path) = socket_alias_claim_pair(&from, &to)
            .or_else(|| socket_alias_claim_pair(&to, &from))
            .ok_or(FuseError::NotControlFile)?;
        SocketAlias::parse(alias_path)
            .ok_or(FuseError::NotControlFile)?
            .authorize(self, uid)?;

        let from_path = self.resolve(&from)?;
        let to_path = self.resolve(&to)?;
        let parent = from_path.parent().ok_or(FuseError::InvalidPath)?;
        if to_path.parent() != Some(parent) || self.resolve(claim_path)?.parent() != Some(parent) {
            return Err(FuseError::InvalidPath);
        }
        let parent_dir = open_plain_directory(parent).map_err(|_error| FuseError::Io)?;
        let from_name = plain_file_name(&from_path).map_err(|_error| FuseError::InvalidPath)?;
        let to_name = plain_file_name(&to_path).map_err(|_error| FuseError::InvalidPath)?;
        require_socket_claim_entry(&parent_dir, from_name)?;
        nix::fcntl::renameat2(
            &parent_dir,
            from_name,
            &parent_dir,
            to_name,
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        )
        .map_err(rename_socket_claim_error)?;
        parent_dir.sync_all().map_err(|_error| FuseError::Io)
    }

    /// Removes one owner-authorized generated socket claim.
    #[doc(hidden)]
    pub fn remove_socket_alias_claim(&self, abi_path: &str, uid: u32) -> Result<(), FuseError> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let alias_path = socket_alias_for_claim(&normalized).ok_or(FuseError::NotControlFile)?;
        SocketAlias::parse(&alias_path)
            .ok_or(FuseError::NotControlFile)?
            .authorize(self, uid)?;

        let path = self.resolve(&normalized)?;
        let parent = path.parent().ok_or(FuseError::InvalidPath)?;
        let parent_dir = open_plain_directory(parent).map_err(|_error| FuseError::Io)?;
        let name = plain_file_name(&path).map_err(|_error| FuseError::InvalidPath)?;
        require_socket_claim_entry(&parent_dir, name)?;
        nix::unistd::unlinkat(&parent_dir, name, nix::unistd::UnlinkatFlags::NoRemoveDir)
            .map_err(|error| fuse_metadata_error(&std::io::Error::from(error)))?;
        parent_dir.sync_all().map_err(|_error| FuseError::Io)
    }
}

fn chown_socket_entry(parent: &fs::File, name: &str, uid: u32, gid: u32) -> nix::Result<()> {
    nix::unistd::fchownat(
        parent,
        name,
        Some(nix::unistd::Uid::from_raw(uid)),
        Some(nix::unistd::Gid::from_raw(gid)),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
}

/// Returns a `(alias, claim)` pair when paths and generated sibling marker match.
fn socket_alias_claim_pair<'a>(alias: &'a str, claim: &'a str) -> Option<(&'a str, &'a str)> {
    let (alias_parent, alias_name) = alias.rsplit_once('/')?;
    let (claim_parent, claim_name) = claim.rsplit_once('/')?;
    (alias_parent == claim_parent
        && generated_sibling_target(claim_name, "claim").is_some_and(|target| target == alias_name)
        && SocketAlias::parse(alias).is_some())
    .then_some((alias, claim))
}

fn socket_alias_for_claim(claim: &str) -> Option<String> {
    let (parent, name) = claim.rsplit_once('/')?;
    let alias_name = generated_sibling_target(name, "claim")?;
    let alias = format!("{parent}/{alias_name}");
    SocketAlias::parse(&alias).is_some().then_some(alias)
}

fn require_socket_claim_entry(parent: &fs::File, name: &str) -> Result<(), FuseError> {
    let stat = nix::sys::stat::fstatat(parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(|error| fuse_metadata_error(&std::io::Error::from(error)))?;
    let kind = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode);
    (kind == nix::sys::stat::SFlag::S_IFLNK || kind == nix::sys::stat::SFlag::S_IFSOCK)
        .then_some(())
        .ok_or(FuseError::InvalidPath)
}

fn rename_socket_claim_error(error: nix::errno::Errno) -> FuseError {
    match error {
        nix::errno::Errno::EEXIST => FuseError::AlreadyExists,
        nix::errno::Errno::ENOENT => FuseError::NotFound,
        _ => FuseError::Io,
    }
}

fn remove_created_socket_entry(parent: &fs::File, name: &str, created: bool) {
    if created {
        let _ignored = nix::unistd::unlinkat(parent, name, nix::unistd::UnlinkatFlags::NoRemoveDir);
        let _ignored = parent.sync_all();
    }
}

#[derive(Clone, Copy)]
enum SocketAlias<'a> {
    Agent {
        agent: &'a str,
    },
    Terminal {
        home_uid: u32,
        agent: &'a str,
        session: &'a str,
    },
}

impl<'a> SocketAlias<'a> {
    fn parse(path: &'a str) -> Option<Self> {
        let parts = path.split('/').collect::<Vec<_>>();
        match *parts.as_slice() {
            ["agent", socket] => {
                let agent = socket.strip_suffix(".sock")?;
                is_object_name(agent).then_some(Self::Agent { agent })
            }
            [
                "home",
                uid,
                "agent",
                agent,
                "session",
                session,
                "terminal",
                "main.sock",
            ] if is_object_name(agent) && is_object_name(session) => Some(Self::Terminal {
                home_uid: uid.parse().ok()?,
                agent,
                session,
            }),
            _ => None,
        }
    }

    fn authorize(self, projection: &FuseProjection, uid: u32) -> Result<(), FuseError> {
        match self {
            Self::Agent { agent } => projection.authorize_agent_owner(agent, uid),
            Self::Terminal {
                home_uid, agent, ..
            } if home_uid == uid => projection.authorize_agent_owner(agent, uid),
            Self::Terminal { .. } => Err(FuseError::PermissionDenied),
        }
    }

    fn validate_target(self, target: &Path, uid: u32) -> Result<(), FuseError> {
        let components = absolute_socket_components(target).ok_or(FuseError::InvalidPath)?;
        let uid = uid.to_string();
        match self {
            Self::Agent { agent } => {
                let prefix = ["run", "user", uid.as_str(), "cortexfs", "agent"];
                let rest = components
                    .strip_prefix(prefix.as_slice())
                    .ok_or(FuseError::InvalidPath)?;
                let expected = format!("{agent}.sock");
                let Some((socket, scope)) = rest.split_last() else {
                    return Err(FuseError::InvalidPath);
                };
                if scope.is_empty()
                    || *socket != expected
                    || !scope.iter().all(|part| is_object_name(part))
                {
                    return Err(FuseError::InvalidPath);
                }
                Ok(())
            }
            Self::Terminal { agent, session, .. } => {
                let expected = [
                    "run",
                    "user",
                    uid.as_str(),
                    "cortexfs",
                    "terminal",
                    agent,
                    session,
                    "main.sock",
                ];
                (components == expected)
                    .then_some(())
                    .ok_or(FuseError::InvalidPath)
            }
        }
    }
}

fn absolute_socket_components(path: &Path) -> Option<Vec<&str>> {
    if !path.is_absolute() {
        return None;
    }
    path.components()
        .filter_map(|component| match component {
            std::path::Component::RootDir => None,
            std::path::Component::Normal(part) => part.to_str(),
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::Prefix(_) => Some(""),
        })
        .map(|part| (!part.is_empty()).then_some(part))
        .collect()
}
