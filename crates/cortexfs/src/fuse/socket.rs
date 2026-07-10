use crate::*;

impl FuseV1Projection {
    /// Persists one owner-authorized agent socket placeholder.
    pub fn create_socket_placeholder(
        &self,
        abi_path: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> Result<FuseV1Node, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let Some(SocketAlias::Agent { agent }) = SocketAlias::parse(&normalized) else {
            return Err(FuseV1Error::NotControlFile);
        };
        self.authorize_agent_owner(agent, uid)?;
        let path = self.resolve(&normalized)?;
        let created =
            plain_fs::ensure_socket_placeholder(&path, mode).map_err(|_error| FuseV1Error::Io)?;
        if let Err(error) = Self::chown_fuse_v1_plain_path(&path, uid, gid) {
            if created {
                let _ignored = self.remove_socket_alias(&normalized, uid);
            }
            return Err(error);
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

    /// Persists one owner-authorized runtime socket alias.
    pub fn set_socket_alias(
        &self,
        abi_path: &str,
        target: &Path,
        uid: u32,
        gid: u32,
    ) -> Result<FuseV1Node, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let alias = SocketAlias::parse(&normalized).ok_or(FuseV1Error::NotControlFile)?;
        alias.authorize(self, uid)?;
        alias.validate_target(target, uid)?;
        let path = self.resolve(&normalized)?;
        let parent = path.parent().ok_or(FuseV1Error::InvalidPath)?;
        let parent_dir =
            plain_fs::open_plain_directory(parent).map_err(|_error| FuseV1Error::Io)?;
        let name = plain_fs::plain_file_name(&path).map_err(|_error| FuseV1Error::InvalidPath)?;
        let created = match nix::fcntl::readlinkat(&parent_dir, name).map(PathBuf::from) {
            Ok(existing) if existing == target => false,
            Ok(_existing) => return Err(FuseV1Error::InvalidPath),
            Err(nix::errno::Errno::ENOENT) => {
                nix::unistd::symlinkat(target, &parent_dir, name)
                    .map_err(|_error| FuseV1Error::Io)?;
                true
            }
            Err(nix::errno::Errno::EINVAL) => return Err(FuseV1Error::InvalidPath),
            Err(_error) => return Err(FuseV1Error::Io),
        };
        if nix::unistd::fchownat(
            &parent_dir,
            name,
            Some(nix::unistd::Uid::from_raw(uid)),
            Some(nix::unistd::Gid::from_raw(gid)),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .is_err()
        {
            remove_created_socket_entry(&parent_dir, name, created);
            return Err(FuseV1Error::Io);
        }
        if parent_dir.sync_all().is_err() {
            remove_created_socket_entry(&parent_dir, name, created);
            return Err(FuseV1Error::Io);
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
    pub fn remove_socket_alias(&self, abi_path: &str, uid: u32) -> Result<(), FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let alias = SocketAlias::parse(&normalized).ok_or(FuseV1Error::NotControlFile)?;
        alias.authorize(self, uid)?;
        let path = self.resolve(&normalized)?;
        let parent = path.parent().ok_or(FuseV1Error::InvalidPath)?;
        let parent_dir =
            plain_fs::open_plain_directory(parent).map_err(|_error| FuseV1Error::Io)?;
        let name = plain_fs::plain_file_name(&path).map_err(|_error| FuseV1Error::InvalidPath)?;
        let stat = match nix::sys::stat::fstatat(
            &parent_dir,
            name,
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        ) {
            Ok(stat) => stat,
            Err(nix::errno::Errno::ENOENT) => return Ok(()),
            Err(_error) => return Err(FuseV1Error::Io),
        };
        let kind = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode);
        if !kind.intersects(nix::sys::stat::SFlag::S_IFLNK | nix::sys::stat::SFlag::S_IFSOCK) {
            return Err(FuseV1Error::InvalidPath);
        }
        nix::unistd::unlinkat(&parent_dir, name, nix::unistd::UnlinkatFlags::NoRemoveDir)
            .map_err(|_error| FuseV1Error::Io)?;
        parent_dir.sync_all().map_err(|_error| FuseV1Error::Io)
    }

    /// Returns whether the path is one of the two writable socket-alias shapes.
    #[doc(hidden)]
    #[must_use]
    pub fn is_socket_alias_path(abi_path: &str) -> bool {
        SocketAlias::parse(abi_path).is_some()
    }

    /// Validates ownership for one writable socket-alias shape.
    #[doc(hidden)]
    pub fn authorize_socket_alias(&self, abi_path: &str, uid: u32) -> Result<(), FuseV1Error> {
        SocketAlias::parse(abi_path)
            .ok_or(FuseV1Error::NotControlFile)?
            .authorize(self, uid)
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

    fn authorize(self, projection: &FuseV1Projection, uid: u32) -> Result<(), FuseV1Error> {
        match self {
            Self::Agent { agent } => projection.authorize_agent_owner(agent, uid),
            Self::Terminal {
                home_uid, agent, ..
            } if home_uid == uid => projection.authorize_agent_owner(agent, uid),
            Self::Terminal { .. } => Err(FuseV1Error::PermissionDenied),
        }
    }

    fn validate_target(self, target: &Path, uid: u32) -> Result<(), FuseV1Error> {
        let components = absolute_socket_components(target).ok_or(FuseV1Error::InvalidPath)?;
        let uid = uid.to_string();
        match self {
            Self::Agent { agent } => {
                let prefix = ["run", "user", uid.as_str(), "cortexfs", "agent"];
                let rest = components
                    .strip_prefix(prefix.as_slice())
                    .ok_or(FuseV1Error::InvalidPath)?;
                let expected = format!("{agent}.sock");
                let Some((socket, scope)) = rest.split_last() else {
                    return Err(FuseV1Error::InvalidPath);
                };
                if scope.is_empty()
                    || *socket != expected
                    || !scope.iter().all(|part| is_object_name(part))
                {
                    return Err(FuseV1Error::InvalidPath);
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
                    .ok_or(FuseV1Error::InvalidPath)
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
