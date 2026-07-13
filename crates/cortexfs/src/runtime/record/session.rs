use super::*;
use std::os::unix::fs::MetadataExt;

use nix::{
    fcntl::{AtFlags, OFlag, open, openat},
    libc,
    sys::stat::{Mode, fchmod, fstatat},
    unistd::{Gid, Uid, fchown},
};

#[derive(Debug)]
struct SessionPermissionReceipt {
    path: PathBuf,
    dev: u64,
    ino: u64,
    directory: bool,
}

#[derive(Debug)]
pub(crate) struct OwnedSessionPreparation {
    session: PathBuf,
    session_dev: u64,
    session_ino: u64,
    current_run: PathBuf,
    current_run_dev: u64,
    current_run_ino: u64,
    uid: u32,
    gid: u32,
}

#[expect(
    clippy::too_many_arguments,
    reason = "session ownership preparation keeps identity fields explicit"
)]
pub(crate) fn prepare_owned_durable_session(
    session_root: &Path,
    session: &str,
    cwd: &str,
    model: Option<&str>,
    scope: SocketSessionScope,
    uid: u32,
    gid: u32,
) -> Result<OwnedSessionPreparation, String> {
    let _receipts = ensure_durable_session_layout(session_root, session, cwd, model, scope)
        .map_err(|error| format!("cannot prepare durable session: {}", error.errno()))?;
    let session_dir = session_root.join(session);
    let current_run = session_dir.join("current_run");
    match atomic_create_text_with_mode(&current_run, "", 0o600) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !is_plain_existing_file(&current_run) {
                return Err("invalid current_run base file".to_owned());
            }
        }
        Err(error) => return Err(format!("cannot create current_run base file: {error}")),
    }
    repair_agent_private_home_permissions(session_root, uid, gid)?;
    repair_agent_session_root_permissions(session_root, uid, gid)?;
    let session_file = open_repair_path(&session_dir, true)?;
    let session_metadata = session_file
        .metadata()
        .map_err(|error| format!("cannot bind prepared session: {error}"))?;
    let current_file = open_repair_path(&current_run, false)?;
    let current_metadata = current_file
        .metadata()
        .map_err(|error| format!("cannot bind prepared current_run: {error}"))?;
    Ok(OwnedSessionPreparation {
        session: session_dir,
        session_dev: session_metadata.dev(),
        session_ino: session_metadata.ino(),
        current_run,
        current_run_dev: current_metadata.dev(),
        current_run_ino: current_metadata.ino(),
        uid,
        gid,
    })
}

fn repair_agent_private_home_permissions(
    session_root: &Path,
    uid: u32,
    gid: u32,
) -> Result<(), String> {
    if session_root.file_name().and_then(|name| name.to_str()) != Some("session") {
        return Ok(());
    }
    let agent_home = session_root
        .parent()
        .ok_or_else(|| "session root has no agent home".to_owned())?;
    let file = open_repair_path(agent_home, true)?;
    let metadata = file.metadata().map_err(|error| {
        format!(
            "cannot inspect agent home {}: {error}",
            agent_home.display()
        )
    })?;
    execute_permission_receipt(
        &SessionPermissionReceipt {
            path: agent_home.to_owned(),
            dev: metadata.dev(),
            ino: metadata.ino(),
            directory: true,
        },
        uid,
        gid,
    )
}

pub(crate) fn repair_agent_session_permissions(
    session: &Path,
    uid: u32,
    gid: u32,
) -> Result<(), String> {
    repair_agent_permissions(session, uid, gid, PreflightRoot::Session)
}

fn repair_agent_session_root_permissions(
    session_root: &Path,
    uid: u32,
    gid: u32,
) -> Result<(), String> {
    repair_agent_permissions(session_root, uid, gid, PreflightRoot::Store)
}

fn repair_agent_permissions(
    path: &Path,
    uid: u32,
    gid: u32,
    root: PreflightRoot,
) -> Result<(), String> {
    let receipts = plan_session_permissions(path, root)?;
    for receipt in receipts {
        execute_permission_receipt(&receipt, uid, gid)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PreflightRoot {
    Session,
    Store,
    Nested,
}

fn plan_session_permissions(
    session: &Path,
    root_kind: PreflightRoot,
) -> Result<Vec<SessionPermissionReceipt>, String> {
    let mut receipts = Vec::new();
    let root = open_repair_path(session, true)?;
    let metadata = root
        .metadata()
        .map_err(|error| format!("cannot inspect session path {}: {error}", session.display()))?;
    if !metadata.is_dir() {
        return Err(format!("invalid session root {}", session.display()));
    }
    preflight_session(session, &root, root_kind, &mut receipts)?;
    receipts.push(SessionPermissionReceipt {
        path: session.to_owned(),
        dev: metadata.dev(),
        ino: metadata.ino(),
        directory: true,
    });
    Ok(receipts)
}

fn preflight_session(
    path: &Path,
    directory: &fs::File,
    root: PreflightRoot,
    receipts: &mut Vec<SessionPermissionReceipt>,
) -> Result<(), String> {
    let mut names = fs::read_dir(support::plain::proc_fd_path(directory))
        .map_err(|error| format!("cannot read session dir {}: {error}", path.display()))?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot read session dir {}: {error}", path.display()))?;
    names.sort();
    for name in names {
        if root == PreflightRoot::Session && (name == "workspace-overlay" || name == "terminal") {
            continue;
        }
        let name = name
            .to_str()
            .ok_or_else(|| "session path contains invalid component".to_owned())?;
        if root == PreflightRoot::Store && name == ".archive" {
            continue;
        }
        let stat = fstatat(directory, name, AtFlags::AT_SYMLINK_NOFOLLOW).map_err(|error| {
            format!(
                "cannot inspect session path {}/{name}: {error}",
                path.display()
            )
        })?;
        let directory_kind = stat.st_mode & libc::S_IFMT == libc::S_IFDIR;
        if root == PreflightRoot::Store
            && (!directory_kind || name != "index" && !is_object_name(name))
        {
            return Err(format!("invalid session path {}/{name}", path.display()));
        }
        if !directory_kind && stat.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(format!("invalid session path {}/{name}", path.display()));
        }
        let mut flags = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK;
        if directory_kind {
            flags |= OFlag::O_DIRECTORY;
        }
        let file = openat(directory, name, flags, Mode::empty())
            .map(fs::File::from)
            .map_err(|error| {
                format!(
                    "cannot open session path {}/{name}: {error}",
                    path.display()
                )
            })?;
        let metadata = file.metadata().map_err(|error| {
            format!(
                "cannot inspect session path {}/{name}: {error}",
                path.display()
            )
        })?;
        if (stat.st_dev, stat.st_ino) != (metadata.dev(), metadata.ino())
            || metadata.is_dir() != directory_kind
            || metadata.is_file() == directory_kind
        {
            return Err(format!(
                "session path replacement conflict {}/{name}",
                path.display()
            ));
        }
        let child = path.join(name);
        if directory_kind {
            let child_root = match root {
                PreflightRoot::Store if name == "index" => PreflightRoot::Nested,
                PreflightRoot::Store => PreflightRoot::Session,
                PreflightRoot::Session | PreflightRoot::Nested => PreflightRoot::Nested,
            };
            preflight_session(&child, &file, child_root, receipts)?;
        }
        receipts.push(SessionPermissionReceipt {
            path: child,
            dev: metadata.dev(),
            ino: metadata.ino(),
            directory: directory_kind,
        });
    }
    Ok(())
}

fn execute_permission_receipt(
    receipt: &SessionPermissionReceipt,
    uid: u32,
    gid: u32,
) -> Result<(), String> {
    let file = open_repair_path(&receipt.path, receipt.directory)?;
    let metadata = file.metadata().map_err(|error| {
        format!(
            "cannot inspect session path {}: {error}",
            receipt.path.display()
        )
    })?;
    if (metadata.dev(), metadata.ino()) != (receipt.dev, receipt.ino)
        || metadata.is_dir() != receipt.directory
        || metadata.is_file() == receipt.directory
    {
        return Err(format!(
            "session path replacement conflict {}",
            receipt.path.display()
        ));
    }
    fchown(&file, Some(Uid::from_raw(uid)), Some(Gid::from_raw(gid))).map_err(|error| {
        format!(
            "cannot chown session path {}: {error}",
            receipt.path.display()
        )
    })?;
    fchmod(
        &file,
        Mode::from_bits_truncate(if receipt.directory { 0o700 } else { 0o600 }),
    )
    .map_err(|error| {
        format!(
            "cannot chmod session path {}: {error}",
            receipt.path.display()
        )
    })?;
    file.sync_all().map_err(|error| {
        format!(
            "cannot sync session path {}: {error}",
            receipt.path.display()
        )
    })
}

fn open_repair_path(path: &Path, directory: bool) -> Result<fs::File, String> {
    let mut current = open(
        if path.is_absolute() {
            Path::new("/")
        } else {
            Path::new(".")
        },
        OFlag::O_DIRECTORY | OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map(fs::File::from)
    .map_err(|error| format!("cannot open session path {}: {error}", path.display()))?;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let mut flags = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
                if components.peek().is_some() || directory {
                    flags |= OFlag::O_DIRECTORY;
                } else {
                    flags |= OFlag::O_NONBLOCK;
                }
                current = openat(&current, name, flags, Mode::empty())
                    .map(fs::File::from)
                    .map_err(|error| {
                        format!("cannot open session path {}: {error}", path.display())
                    })?;
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(format!("unsupported session path {}", path.display()));
            }
        }
    }
    Ok(current)
}

pub(crate) fn append_session_lines(
    dir: &Path,
    file: &str,
    lines: &[&str],
) -> SocketRecordResult<()> {
    let stream = match file {
        "messages.jsonl" => columnar::Stream::Messages,
        "events.jsonl" => columnar::Stream::Events,
        _ => return Err(SocketSessionRecordError::CannotRecord),
    };
    let history = columnar::HistoryGuard::exclusive(dir)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    history
        .refresh_claims()
        .and_then(|()| history.append(stream, lines))
        .and_then(|()| history.refresh_claims())
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    Ok(())
}

pub(crate) fn write_session_file(dir: &Path, file: &str, content: &str) -> SocketRecordResult<()> {
    atomic_replace_text_preserving_metadata(&dir.join(file), content)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)
}

pub(crate) fn write_current_run_session_file(
    dir: &Path,
    content: &str,
    preparation: Option<&OwnedSessionPreparation>,
) -> SocketRecordResult<()> {
    let path = dir.join("current_run");
    let result = if let Some(preparation) = preparation {
        verify_owned_session_preparation(preparation, dir, &path)
            .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
        atomic_replace_text_preserving_metadata(&path, content)
    } else {
        atomic_replace_text(&path, content)
    };
    result.map_err(|_error| SocketSessionRecordError::CannotRecord)
}

fn verify_owned_session_preparation(
    preparation: &OwnedSessionPreparation,
    session: &Path,
    current_run: &Path,
) -> Result<(), String> {
    if preparation.session != session || preparation.current_run != current_run {
        return Err("prepared session mismatch".to_owned());
    }
    let session_file = open_repair_path(session, true)?;
    let session_metadata = session_file.metadata().map_err(|error| error.to_string())?;
    let current_file = open_repair_path(current_run, false)?;
    let current_metadata = current_file.metadata().map_err(|error| error.to_string())?;
    if (session_metadata.dev(), session_metadata.ino())
        != (preparation.session_dev, preparation.session_ino)
        || (current_metadata.dev(), current_metadata.ino())
            != (preparation.current_run_dev, preparation.current_run_ino)
        || current_metadata.uid() != preparation.uid
        || current_metadata.gid() != preparation.gid
        || current_metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err("prepared session replacement conflict".to_owned());
    }
    Ok(())
}

pub(crate) fn set_session_state(dir: &Path, state: &str) -> SocketRecordResult<()> {
    write_session_file(dir, "state", &format!("{state}\n"))?;
    touch_session(dir)
}

pub(crate) fn touch_session(dir: &Path) -> SocketRecordResult<()> {
    write_session_file(dir, "updated_at", &unix_timestamp_text())
}

pub(crate) fn write_text_file_if_absent(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "text file must have a parent",
        )
    })?;
    let name = plain_file_name(path)?;
    let parent_dir = open_plain_directory(parent)?;
    match openat(
        &parent_dir,
        name,
        OFlag::O_RDWR | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    ) {
        Ok(file_fd) => {
            let file = fs::File::from(file_fd);
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::other("path is not a regular file"));
            }
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
            file.sync_all()?;
            parent_dir.sync_all()?;
            return Ok(());
        }
        Err(nix::errno::Errno::ENOENT) => {}
        Err(error) => return Err(std::io::Error::from(error)),
    }
    let file_fd = openat(
        &parent_dir,
        name,
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )
    .map_err(std::io::Error::from)?;
    let mut file = fs::File::from(file_fd);
    file.write_all(content.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    parent_dir.sync_all()?;
    Ok(())
}

pub(crate) fn create_private_context_dir(path: &Path) -> std::io::Result<()> {
    match open_private_context_dir(path) {
        Ok(dir) => {
            dir.set_permissions(fs::Permissions::from_mode(0o700))?;
            dir.sync_all()
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "context directory must have a parent",
                )
            })?;
            let name = plain_file_name(path)?;
            let parent_dir = open_plain_directory(parent)?;
            nix::sys::stat::mkdirat(&parent_dir, name, Mode::from_bits_truncate(0o700))
                .map_err(std::io::Error::from)?;
            parent_dir.sync_all()?;
            let dir = open_private_context_dir(path)?;
            dir.set_permissions(fs::Permissions::from_mode(0o700))?;
            dir.sync_all()?;
            parent_dir.sync_all()?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn open_private_context_dir(path: &Path) -> std::io::Result<fs::File> {
    let dir = open_plain_directory(path)?;
    if !dir.metadata()?.is_dir() {
        return Err(std::io::Error::other("path is not a directory"));
    }
    Ok(dir)
}

pub(crate) fn ensure_trailing_newline(content: &str) -> String {
    if content.is_empty() || content.ends_with('\n') {
        content.to_owned()
    } else {
        format!("{content}\n")
    }
}

pub(crate) fn require_socket_session_name(
    session_dir: &Path,
    session: &str,
) -> Result<(), SocketSessionRecordError> {
    if session_dir.file_name().and_then(|name| name.to_str()) == Some(session) {
        Ok(())
    } else {
        Err(SocketSessionRecordError::SessionMismatch)
    }
}

pub(crate) fn require_socket_session_files(
    session_dir: &Path,
) -> Result<(), SocketSessionRecordError> {
    for file in SESSION_REQUIRED_FILES {
        if !is_plain_existing_file(&session_dir.join(file)) {
            return Err(SocketSessionRecordError::MissingSessionFile(file));
        }
    }
    Ok(())
}

pub(crate) fn is_plain_existing_file(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_file())
}

pub(crate) fn is_plain_existing_dir(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
}

pub(crate) fn validate_socket_object_field(
    field: &'static str,
    value: &str,
) -> Result<(), SocketRequestError> {
    if is_object_name(value) {
        Ok(())
    } else {
        Err(SocketRequestError::InvalidField {
            field,
            value: value.to_owned(),
        })
    }
}

#[cfg(test)]
mod permission_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    fn root(name: &str) -> PathBuf {
        env::temp_dir().join(format!("cfs-session-{name}-{}", std::process::id()))
    }

    #[test]
    fn late_invalid_entry_performs_zero_preflight_mutation() {
        let root = root("late-invalid");
        let _ignored = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok());
        assert!(fs::write(root.join("a"), "a").is_ok());
        assert!(fs::set_permissions(root.join("a"), fs::Permissions::from_mode(0o644)).is_ok());
        let socket = root.join("z");
        let listener = UnixListener::bind(&socket);
        assert!(listener.is_ok());

        assert!(plan_session_permissions(&root, PreflightRoot::Session).is_err());
        assert_eq!(
            fs::metadata(root.join("a"))
                .map(|m| m.permissions().mode() & 0o777)
                .ok(),
            Some(0o644)
        );
        drop(listener);
        let _ignored = fs::remove_dir_all(root);
    }

    #[test]
    fn same_type_replacement_conflicts_at_execution() {
        let root = root("replacement");
        let _ignored = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok());
        let file = root.join("state");
        assert!(fs::write(&file, "old").is_ok());
        let plan = plan_session_permissions(&root, PreflightRoot::Session);
        assert!(plan.is_ok());
        let Ok(mut plan) = plan else {
            return;
        };
        let receipt = plan
            .iter()
            .position(|entry| entry.path == file)
            .map(|index| plan.remove(index));
        assert!(receipt.is_some());
        assert!(fs::remove_file(&file).is_ok());
        assert!(fs::write(&file, "new").is_ok());
        let Some(receipt) = receipt else {
            return;
        };
        assert!(
            execute_permission_receipt(
                &receipt,
                nix::unistd::geteuid().as_raw(),
                nix::unistd::getegid().as_raw()
            )
            .is_err()
        );
        let _ignored = fs::remove_dir_all(root);
    }

    #[test]
    fn exact_session_modes_overlay_and_noninterference() {
        let base = root("scope");
        let session = base.join("selected");
        let other = base.join("other");
        let _ignored = fs::remove_dir_all(&base);
        assert!(fs::create_dir_all(session.join("workspace-overlay")).is_ok());
        assert!(fs::create_dir_all(session.join("context/workspace-overlay")).is_ok());
        assert!(fs::create_dir_all(&other).is_ok());
        let selected = session.join("state");
        let top_overlay = session.join("workspace-overlay/keep");
        let nested_overlay = session.join("context/workspace-overlay/state");
        let untouched = other.join("state");
        for path in [&selected, &top_overlay, &nested_overlay, &untouched] {
            assert!(fs::write(path, "state").is_ok());
            assert!(fs::set_permissions(path, fs::Permissions::from_mode(0o644)).is_ok());
        }
        let uid = nix::unistd::geteuid().as_raw();
        let gid = nix::unistd::getegid().as_raw();

        assert!(repair_agent_session_permissions(&session, uid, gid).is_ok());

        assert_eq!(
            fs::metadata(&selected)
                .map(|m| m.permissions().mode() & 0o777)
                .ok(),
            Some(0o600)
        );
        assert_eq!(
            fs::metadata(&nested_overlay)
                .map(|m| m.permissions().mode() & 0o777)
                .ok(),
            Some(0o600)
        );
        assert_eq!(
            fs::metadata(&top_overlay)
                .map(|m| m.permissions().mode() & 0o777)
                .ok(),
            Some(0o644)
        );
        assert_eq!(
            fs::metadata(&untouched)
                .map(|m| m.permissions().mode() & 0o777)
                .ok(),
            Some(0o644)
        );
        let metadata = fs::metadata(selected);
        assert!(metadata.is_ok_and(|m| m.uid() == uid && m.gid() == gid));
        let _ignored = fs::remove_dir_all(base);
    }

    #[test]
    fn exact_session_terminal_symlink_is_excluded() {
        let base = root("terminal-excluded");
        let session = base.join("selected");
        let target = base.join("terminal-target");
        let _ignored = fs::remove_dir_all(&base);
        assert!(fs::create_dir_all(&session).is_ok());
        assert!(fs::create_dir_all(&target).is_ok());
        assert!(fs::write(target.join("main.sock"), "keep").is_ok());
        assert!(
            fs::set_permissions(target.join("main.sock"), fs::Permissions::from_mode(0o644))
                .is_ok()
        );
        assert!(std::os::unix::fs::symlink(&target, session.join("terminal")).is_ok());

        let uid = nix::unistd::geteuid().as_raw();
        let gid = nix::unistd::getegid().as_raw();
        assert!(repair_agent_session_permissions(&session, uid, gid).is_ok());
        assert_eq!(
            fs::metadata(target.join("main.sock"))
                .map(|m| m.permissions().mode() & 0o777)
                .ok(),
            Some(0o644)
        );
        let _ignored = fs::remove_dir_all(base);
    }

    #[test]
    fn session_store_excludes_each_session_terminal_only() {
        let base = root("store-terminal-excluded");
        let session = base.join("selected");
        let target = base.with_extension("terminal-target");
        let _ignored = fs::remove_dir_all(&base);
        let _ignored = fs::remove_dir_all(&target);
        assert!(fs::create_dir_all(base.join("index")).is_ok());
        assert!(fs::create_dir_all(&session).is_ok());
        assert!(fs::create_dir_all(&target).is_ok());
        assert!(fs::write(base.join("index/list"), "").is_ok());
        assert!(fs::write(target.join("main.sock"), "keep").is_ok());
        assert!(
            fs::set_permissions(target.join("main.sock"), fs::Permissions::from_mode(0o644))
                .is_ok()
        );
        assert!(std::os::unix::fs::symlink(&target, session.join("terminal")).is_ok());
        let uid = nix::unistd::geteuid().as_raw();
        let gid = nix::unistd::getegid().as_raw();

        assert!(repair_agent_session_root_permissions(&base, uid, gid).is_ok());
        assert_eq!(
            fs::metadata(target.join("main.sock"))
                .map(|metadata| metadata.permissions().mode() & 0o777)
                .ok(),
            Some(0o644)
        );
        let _ignored = fs::remove_dir_all(base);
        let _ignored = fs::remove_dir_all(target);
    }

    #[test]
    fn session_store_rejects_nested_terminal_without_mutation() {
        let base = root("store-nested-terminal");
        let session = base.join("selected");
        let target = base.with_extension("nested-target");
        let _ignored = fs::remove_dir_all(&base);
        let _ignored = fs::remove_dir_all(&target);
        assert!(fs::create_dir_all(base.join("index")).is_ok());
        assert!(fs::create_dir_all(session.join("context")).is_ok());
        assert!(fs::create_dir_all(&target).is_ok());
        assert!(fs::write(base.join("index/list"), "").is_ok());
        let unchanged = session.join("state");
        assert!(fs::write(&unchanged, "state").is_ok());
        assert!(fs::set_permissions(&unchanged, fs::Permissions::from_mode(0o644)).is_ok());
        assert!(std::os::unix::fs::symlink(&target, session.join("context/terminal")).is_ok());

        assert!(
            repair_agent_session_root_permissions(
                &base,
                nix::unistd::geteuid().as_raw(),
                nix::unistd::getegid().as_raw(),
            )
            .is_err()
        );
        assert_eq!(
            fs::metadata(unchanged)
                .map(|metadata| metadata.permissions().mode() & 0o777)
                .ok(),
            Some(0o644)
        );
        let _ignored = fs::remove_dir_all(base);
        let _ignored = fs::remove_dir_all(target);
    }

    #[test]
    fn nested_terminal_symlink_is_rejected() {
        let base = root("nested-terminal");
        let session = base.join("selected");
        let target = base.join("terminal-target");
        let _ignored = fs::remove_dir_all(&base);
        assert!(fs::create_dir_all(session.join("context")).is_ok());
        assert!(fs::create_dir_all(&target).is_ok());
        assert!(std::os::unix::fs::symlink(&target, session.join("context/terminal")).is_ok());

        assert!(plan_session_permissions(&session, PreflightRoot::Session).is_err());
        let _ignored = fs::remove_dir_all(base);
    }

    #[test]
    fn missing_session_prepares_and_original_record_preserves_current_run_owner() {
        let session_root = root("current-run-owner");
        let session = session_root.join("fresh");
        let _ignored = fs::remove_dir_all(&session_root);
        let uid = nix::unistd::geteuid().as_raw();
        let gid = nix::unistd::getegid().as_raw();
        let preparation = prepare_owned_durable_session(
            &session_root,
            "fresh",
            "/workspace",
            None,
            SocketSessionScope::Private,
            uid,
            gid,
        );
        assert!(preparation.is_ok());
        let Ok(preparation) = preparation else {
            return;
        };
        for (path, mode) in [
            (session_root.clone(), 0o700),
            (session_root.join("index"), 0o700),
            (session_root.join("index/list"), 0o600),
            (session.clone(), 0o700),
        ] {
            assert!(fs::metadata(path).is_ok_and(|metadata| {
                metadata.uid() == uid
                    && metadata.gid() == gid
                    && metadata.permissions().mode() & 0o777 == mode
            }));
        }

        assert!(
            record_socket_send_to_session(
                &session,
                "run",
                "run",
                "fresh",
                SocketSessionScope::Private,
                Some("/workspace"),
                "input",
                Some(&preparation),
                None,
            )
            .is_ok()
        );

        let metadata = fs::metadata(session.join("current_run"));
        assert!(metadata.is_ok_and(|value| {
            value.uid() == uid && value.gid() == gid && value.permissions().mode() & 0o777 == 0o600
        }));
        assert_eq!(
            fs::read_to_string(session.join("current_run"))
                .ok()
                .as_deref(),
            Some("run\n")
        );
        let _ignored = fs::remove_dir_all(session_root);
    }

    #[test]
    fn root_runtime_repairs_private_agent_home_traversal_boundary()
    -> Result<(), Box<dyn std::error::Error>> {
        let base = root("agent-home-owner");
        let agent_home = base.join("home/1000/agent/example-echo");
        let session_root = agent_home.join("session");
        let _ignored = fs::remove_dir_all(&base);
        assert!(fs::create_dir_all(&agent_home).is_ok());
        assert!(fs::set_permissions(&agent_home, fs::Permissions::from_mode(0o700)).is_ok());
        let uid = nix::unistd::geteuid().as_raw();
        let gid = nix::unistd::getegid().as_raw();
        assert!(
            prepare_owned_durable_session(
                &session_root,
                "live-sdk",
                "/workspace",
                None,
                SocketSessionScope::Private,
                uid,
                gid,
            )
            .is_ok()
        );
        for path in [&agent_home, &session_root, &session_root.join("live-sdk")] {
            let metadata = fs::symlink_metadata(path)?;
            assert!(!metadata.file_type().is_symlink());
            assert_eq!((metadata.uid(), metadata.gid()), (uid, gid));
            assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        }
        let shared_parent = base.join("home/1000/agent");
        let metadata = fs::metadata(shared_parent)?;
        assert_ne!(metadata.permissions().mode() & 0o777, 0o700);
        let _ignored = fs::remove_dir_all(base);
        Ok(())
    }

    #[test]
    fn failed_preflight_leaves_start_controls_unchanged_and_retryable() {
        let base = root("repair-failure");
        let session = base.join("retry");
        let _ignored = fs::remove_dir_all(&base);
        assert!(fs::create_dir_all(&session).is_ok());
        assert!(fs::write(session.join("events.jsonl"), "before\n").is_ok());
        assert!(fs::write(session.join("current_run"), "old\n").is_ok());
        let socket = session.join("z-invalid");
        let listener = UnixListener::bind(&socket);
        assert!(listener.is_ok());
        let uid = nix::unistd::geteuid().as_raw();
        let gid = nix::unistd::getegid().as_raw();

        assert!(
            prepare_owned_durable_session(
                &base,
                "retry",
                "/workspace",
                None,
                SocketSessionScope::Private,
                uid,
                gid,
            )
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(session.join("events.jsonl"))
                .ok()
                .as_deref(),
            Some("before\n")
        );
        assert_eq!(
            fs::read_to_string(session.join("current_run"))
                .ok()
                .as_deref(),
            Some("old\n")
        );
        drop(listener);
        assert!(fs::remove_file(socket).is_ok());
        assert!(
            prepare_owned_durable_session(
                &base,
                "retry",
                "/workspace",
                None,
                SocketSessionScope::Private,
                uid,
                gid,
            )
            .is_ok()
        );
        let _ignored = fs::remove_dir_all(base);
    }

    #[test]
    fn shared_record_keeps_shared_layout_outside_private_prepare() {
        let session_root = root("shared-semantics");
        let session = session_root.join("shared");
        let _ignored = fs::remove_dir_all(&session_root);
        assert!(
            ensure_durable_session_layout(
                &session_root,
                "shared",
                "/workspace",
                None,
                SocketSessionScope::Shared,
            )
            .is_ok()
        );
        let marker = session.join("shared-marker");
        assert!(fs::write(&marker, "shared\n").is_ok());
        assert!(fs::set_permissions(&marker, fs::Permissions::from_mode(0o640)).is_ok());
        let before = fs::metadata(&marker)
            .map(|value| (value.uid(), value.gid(), value.permissions().mode() & 0o777))
            .ok();
        assert!(!session.join("current_run").exists());

        let request = SocketRequest::Send {
            id: "run".to_owned(),
            session: "shared".to_owned(),
            scope: SocketSessionScope::Shared,
            cwd: Some("/workspace".to_owned()),
            workspace: None,
            input: "input".to_owned(),
        };
        let response = handle_socket_request(&session_root, "/workspace", None, &request);
        assert!(response.is_ok());
        let run = response.ok().and_then(|response| {
            response.frames().first().and_then(|frame| {
                serde_json::from_str::<Value>(frame)
                    .ok()?
                    .get("run")?
                    .as_str()
                    .map(ToOwned::to_owned)
            })
        });
        assert!(run.as_deref().is_some_and(|run| run != "run"));

        assert_eq!(
            fs::metadata(&marker)
                .map(|value| (value.uid(), value.gid(), value.permissions().mode() & 0o777))
                .ok(),
            before
        );
        assert_eq!(
            fs::read_to_string(session.join("current_run"))
                .ok()
                .as_deref(),
            run.as_ref().map(|run| format!("{run}\n")).as_deref()
        );
        let _ignored = fs::remove_dir_all(session_root);
    }

    #[test]
    fn prepared_token_rejects_session_mismatch_and_current_run_replacement() {
        let root = root("prepared-token");
        let _ignored = fs::remove_dir_all(&root);
        let uid = nix::unistd::geteuid().as_raw();
        let gid = nix::unistd::getegid().as_raw();
        let preparation = prepare_owned_durable_session(
            &root,
            "one",
            "/workspace",
            None,
            SocketSessionScope::Private,
            uid,
            gid,
        );
        assert!(preparation.is_ok());
        assert!(
            ensure_durable_session_layout(
                &root,
                "two",
                "/workspace",
                None,
                SocketSessionScope::Private,
            )
            .is_ok()
        );
        let Ok(preparation) = preparation else {
            return;
        };
        assert!(
            write_current_run_session_file(&root.join("two"), "run\n", Some(&preparation)).is_err()
        );
        let current_run = root.join("one/current_run");
        assert!(fs::remove_file(&current_run).is_ok());
        assert!(fs::write(&current_run, "replacement\n").is_ok());
        assert!(
            write_current_run_session_file(&root.join("one"), "run\n", Some(&preparation)).is_err()
        );
        let _ignored = fs::remove_dir_all(root);
    }

    #[test]
    fn session_store_repair_keeps_archive_sentinel_mode() {
        let base = root("session-store-archive-sentinel");
        let _ignored = fs::remove_dir_all(&base);
        assert!(fs::create_dir_all(base.join("index")).is_ok());
        let archive = base.join(".archive");
        assert!(fs::create_dir_all(archive.join("old-session")).is_ok());
        let archive_sentinel = archive.join("old-session/sentinel");
        assert!(fs::write(&archive_sentinel, "old").is_ok());
        assert!(fs::set_permissions(&archive_sentinel, fs::Permissions::from_mode(0o644)).is_ok());
        let selected = base.join("selected");
        assert!(fs::create_dir_all(&selected).is_ok());
        assert!(fs::write(selected.join("state"), "state").is_ok());
        let uid = nix::unistd::geteuid().as_raw();
        let gid = nix::unistd::getegid().as_raw();
        assert!(repair_agent_session_root_permissions(&base, uid, gid).is_ok());
        assert_eq!(
            fs::metadata(selected.join("state"))
                .map(|metadata| metadata.permissions().mode() & 0o777)
                .ok(),
            Some(0o600)
        );
        assert_eq!(
            fs::metadata(&archive_sentinel)
                .map(|metadata| metadata.permissions().mode() & 0o777)
                .ok(),
            Some(0o644)
        );
        let _ignored = fs::remove_dir_all(base);
    }
}
