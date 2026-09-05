use super::*;
use crate::support::atomic::{
    atomic_create_text_with_mode, atomic_replace_text, atomic_replace_text_preserving_metadata,
};
use crate::support::plain::create_exclusive_file_at;
use crate::support::unix_timestamp_text;
use std::os::unix::fs::MetadataExt;

use nix::{
    fcntl::{AtFlags, OFlag, open, openat},
    libc,
    sys::stat::{Mode, fchmod, fstatat},
    unistd::{Gid, Uid, fchown},
};

const MAX_SESSION_TRANSITION_FILE_BYTES: u64 = 64 * 1024;
const MAX_SESSION_TRANSITION_EVENTS_BYTES: u64 = 1024 * 1024;

#[derive(Debug)]
struct SessionPermissionReceipt {
    path: PathBuf,
    dev: u64,
    ino: u64,
    directory: bool,
    file: fs::File,
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
    repair_agent_session_permissions(&session_dir, uid, gid)?;
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
            file,
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

#[cfg(test)]
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
        file: root,
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
            file,
        });
    }
    Ok(())
}

fn execute_permission_receipt(
    receipt: &SessionPermissionReceipt,
    uid: u32,
    gid: u32,
) -> Result<(), String> {
    let held = receipt.file.metadata().map_err(|error| {
        format!(
            "cannot inspect session path {}: {error}",
            receipt.path.display()
        )
    })?;
    let visible = open_repair_path(&receipt.path, receipt.directory)?;
    let visible_metadata = visible.metadata().map_err(|error| {
        format!(
            "cannot inspect session path {}: {error}",
            receipt.path.display()
        )
    })?;
    if (held.dev(), held.ino()) != (receipt.dev, receipt.ino)
        || held.is_dir() != receipt.directory
        || held.is_file() == receipt.directory
        || (visible_metadata.dev(), visible_metadata.ino()) != (receipt.dev, receipt.ino)
        || visible_metadata.is_dir() != receipt.directory
        || visible_metadata.is_file() == receipt.directory
    {
        return Err(format!(
            "session path replacement conflict {}",
            receipt.path.display()
        ));
    }
    fchown(
        &receipt.file,
        Some(Uid::from_raw(uid)),
        Some(Gid::from_raw(gid)),
    )
    .map_err(|error| {
        format!(
            "cannot chown session path {}: {error}",
            receipt.path.display()
        )
    })?;
    fchmod(
        &receipt.file,
        Mode::from_bits_truncate(if receipt.directory { 0o700 } else { 0o600 }),
    )
    .map_err(|error| {
        format!(
            "cannot chmod session path {}: {error}",
            receipt.path.display()
        )
    })?;
    receipt.file.sync_all().map_err(|error| {
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
    set_session_state_with_error(dir, state, None)
}

pub(crate) fn set_session_state_with_error(
    dir: &Path,
    state: &str,
    error: Option<&str>,
) -> SocketRecordResult<()> {
    write_session_file(dir, "state", &format!("{state}\n"))?;
    let current = support::plain::read_small_text_file(
        &dir.join("state.json"),
        MAX_SESSION_TRANSITION_FILE_BYTES,
    )
    .unwrap_or_default();
    let run = support::plain::read_small_text_file(
        &dir.join("current_run"),
        MAX_SESSION_TRANSITION_FILE_BYTES,
    )
    .ok()
    .map(|run| run.trim().to_owned());
    write_runtime_state_file(
        dir,
        "state.json",
        &RuntimeState::transition_json(
            &current,
            state,
            run.as_deref(),
            &unix_timestamp_text(),
            error,
        ),
    )?;
    touch_session(dir)
}

pub(crate) fn set_session_runtime_observation(
    dir: &Path,
    run: &str,
    step: u8,
    action: &str,
    tool: Option<&str>,
    context_revision: Option<&str>,
) -> SocketRecordResult<()> {
    let current = support::plain::read_small_text_file(
        &dir.join("state.json"),
        MAX_SESSION_TRANSITION_FILE_BYTES,
    )
    .unwrap_or_default();
    write_runtime_state_file(
        dir,
        "state.json",
        &RuntimeState::observe_json(
            &current,
            &runtime::observation::RuntimeObservation {
                run,
                step,
                action,
                tool,
                context_revision,
                updated_at: &unix_timestamp_text(),
            },
        ),
    )?;
    touch_session(dir)
}

fn write_runtime_state_file(dir: &Path, file: &str, content: &str) -> SocketRecordResult<()> {
    let path = dir.join(file);
    match support::plain::path_metadata_no_follow(&path) {
        Ok(metadata) if metadata.is_file() => write_session_file(dir, file, content),
        Ok(_metadata) => Err(SocketSessionRecordError::CannotRecord),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match atomic_create_text_with_mode(&path, content, 0o600) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    write_session_file(dir, file, content)
                }
                Err(_error) => Err(SocketSessionRecordError::CannotRecord),
            }
        }
        Err(_error) => Err(SocketSessionRecordError::CannotRecord),
    }
}

pub(crate) fn set_active_session_run_locked(
    _history: &columnar::HistoryGuard<'_>,
    dir: &Path,
    run_id: &str,
    preparation: Option<&OwnedSessionPreparation>,
) -> SocketRecordResult<()> {
    write_current_run_session_file(dir, &format!("{run_id}\n"), preparation)?;
    set_session_state(dir, "active")
}

pub(crate) fn transition_active_session_run_locked(
    history: &columnar::HistoryGuard<'_>,
    dir: &Path,
    run_id: &str,
    terminal_state: &str,
    error: Option<&str>,
) -> SocketRecordResult<bool> {
    if !active_session_run_matches_locked(history, dir, run_id)? {
        return Ok(false);
    }
    set_session_state_with_error(dir, terminal_state, error)?;
    Ok(true)
}

pub(crate) fn active_session_run_matches_locked(
    _history: &columnar::HistoryGuard<'_>,
    dir: &Path,
    run_id: &str,
) -> SocketRecordResult<bool> {
    let state =
        support::plain::read_small_text_file(&dir.join("state"), MAX_SESSION_TRANSITION_FILE_BYTES)
            .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    let current_run = support::plain::read_small_text_file(
        &dir.join("current_run"),
        MAX_SESSION_TRANSITION_FILE_BYTES,
    )
    .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    Ok(state.trim() == "active" && current_run.trim() == run_id)
}

pub(crate) fn resolve_active_session_cancel_run_locked(
    history: &columnar::HistoryGuard<'_>,
    dir: &Path,
    requested_id: &str,
) -> SocketRecordResult<Option<String>> {
    let current_run = support::plain::read_small_text_file(
        &dir.join("current_run"),
        MAX_SESSION_TRANSITION_FILE_BYTES,
    )
    .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    let current_run = current_run.trim();
    if !active_session_run_matches_locked(history, dir, current_run)? {
        return Ok(None);
    }
    if requested_id == current_run {
        return Ok(Some(current_run.to_owned()));
    }
    let events = history
        .read_text(
            columnar::Stream::Events,
            MAX_SESSION_TRANSITION_EVENTS_BYTES,
        )
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    Ok(events.lines().rev().find_map(|line| {
        serde_json::from_str::<Value>(line)
            .is_ok_and(|value| {
                value.get("type").and_then(Value::as_str) == Some("start")
                    && value.get("client_id").and_then(Value::as_str) == Some(requested_id)
                    && value.get("run").and_then(Value::as_str) == Some(current_run)
            })
            .then(|| current_run.to_owned())
    }))
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
    let mut file =
        create_exclusive_file_at(&parent_dir, name, 0o600).map_err(std::io::Error::from)?;
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

#[cfg(test)]
mod permission_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    macro_rules! assert_ok {
        ($($result:expr),+ $(,)?) => {
            $(assert!($result.is_ok());)+
        };
    }

    struct Fixture {
        root: PathBuf,
        uid: u32,
        gid: u32,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = env::temp_dir().join(format!("cfs-session-{name}-{}", std::process::id()));
            let _ignored = fs::remove_dir_all(&root);
            Self {
                root,
                uid: nix::unistd::geteuid().as_raw(),
                gid: nix::unistd::getegid().as_raw(),
            }
        }

        fn prepare(&self, session: &str) -> Result<OwnedSessionPreparation, String> {
            prepare_owned_durable_session(
                &self.root,
                session,
                "/workspace",
                None,
                SocketSessionScope::Private,
                self.uid,
                self.gid,
            )
        }
    }

    impl std::ops::Deref for Fixture {
        type Target = Path;

        fn deref(&self) -> &Self::Target {
            &self.root
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.root);
        }
    }

    fn mode(path: &Path) -> Option<u32> {
        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok()
    }

    fn owner_mode(path: &Path) -> Option<(u32, u32, u32)> {
        fs::metadata(path)
            .map(|metadata| {
                (
                    metadata.uid(),
                    metadata.gid(),
                    metadata.permissions().mode() & 0o777,
                )
            })
            .ok()
    }

    fn text(path: &Path) -> Option<String> {
        fs::read_to_string(path).ok()
    }

    #[test]
    fn late_invalid_entry_performs_zero_preflight_mutation() {
        let root = Fixture::new("late-invalid");
        assert_ok!(
            fs::create_dir_all(&*root),
            fs::write(root.join("a"), "a"),
            fs::set_permissions(root.join("a"), fs::Permissions::from_mode(0o644)),
        );
        let listener = UnixListener::bind(root.join("z"));
        assert!(listener.is_ok());
        assert!(plan_session_permissions(&root, PreflightRoot::Session).is_err());
        assert_eq!(mode(&root.join("a")), Some(0o644));
        drop(listener);
    }

    #[test]
    fn same_type_replacement_conflicts_at_execution() {
        let root = Fixture::new("replacement");
        assert_ok!(fs::create_dir_all(&*root));
        let file = root.join("state");
        assert_ok!(fs::write(&file, "old"));
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
        assert_ok!(fs::remove_file(&file), fs::write(&file, "new"));
        let Some(receipt) = receipt else {
            return;
        };
        assert!(execute_permission_receipt(&receipt, root.uid, root.gid).is_err());
    }

    #[test]
    fn exact_session_modes_overlay_and_noninterference() {
        let base = Fixture::new("scope");
        let session = base.join("selected");
        let other = base.join("other");
        assert_ok!(
            fs::create_dir_all(session.join("workspace-overlay")),
            fs::create_dir_all(session.join("context/workspace-overlay")),
            fs::create_dir_all(&other),
        );
        let selected = session.join("state");
        let top_overlay = session.join("workspace-overlay/keep");
        let nested_overlay = session.join("context/workspace-overlay/state");
        let untouched = other.join("state");
        for path in [&selected, &top_overlay, &nested_overlay, &untouched] {
            assert_ok!(
                fs::write(path, "state"),
                fs::set_permissions(path, fs::Permissions::from_mode(0o644)),
            );
        }
        assert!(repair_agent_session_permissions(&session, base.uid, base.gid).is_ok());
        assert_eq!(mode(&selected), Some(0o600));
        assert_eq!(mode(&nested_overlay), Some(0o600));
        assert_eq!(mode(&top_overlay), Some(0o644));
        assert_eq!(mode(&untouched), Some(0o644));
        let metadata = fs::metadata(selected);
        assert!(metadata.is_ok_and(|value| value.uid() == base.uid && value.gid() == base.gid));
    }

    #[test]
    fn exact_session_terminal_symlink_is_excluded() {
        let base = Fixture::new("terminal-excluded");
        let session = base.join("selected");
        let target = base.join("terminal-target");
        assert_ok!(
            fs::create_dir_all(&session),
            fs::create_dir_all(&target),
            fs::write(target.join("main.sock"), "keep"),
            fs::set_permissions(target.join("main.sock"), fs::Permissions::from_mode(0o644)),
            std::os::unix::fs::symlink(&target, session.join("terminal")),
        );
        assert!(repair_agent_session_permissions(&session, base.uid, base.gid).is_ok());
        assert_eq!(mode(&target.join("main.sock")), Some(0o644));
    }

    #[test]
    fn session_store_excludes_each_session_terminal_only() {
        let base = Fixture::new("store-terminal-excluded");
        let session = base.join("selected");
        let target = base.with_extension("terminal-target");
        let _ignored = fs::remove_dir_all(&target);
        assert_ok!(
            fs::create_dir_all(base.join("index")),
            fs::create_dir_all(&session),
            fs::create_dir_all(&target),
            fs::write(base.join("index/list"), ""),
            fs::write(target.join("main.sock"), "keep"),
            fs::set_permissions(target.join("main.sock"), fs::Permissions::from_mode(0o644)),
            std::os::unix::fs::symlink(&target, session.join("terminal")),
        );
        assert!(repair_agent_session_root_permissions(&base, base.uid, base.gid).is_ok());
        assert_eq!(mode(&target.join("main.sock")), Some(0o644));
        let _ignored = fs::remove_dir_all(target);
    }

    #[test]
    fn session_store_rejects_nested_terminal_without_mutation() {
        let base = Fixture::new("store-nested-terminal");
        let session = base.join("selected");
        let target = base.with_extension("nested-target");
        let _ignored = fs::remove_dir_all(&target);
        assert_ok!(
            fs::create_dir_all(base.join("index")),
            fs::create_dir_all(session.join("context")),
            fs::create_dir_all(&target),
            fs::write(base.join("index/list"), ""),
        );
        let unchanged = session.join("state");
        assert_ok!(
            fs::write(&unchanged, "state"),
            fs::set_permissions(&unchanged, fs::Permissions::from_mode(0o644)),
            std::os::unix::fs::symlink(&target, session.join("context/terminal")),
        );
        assert!(repair_agent_session_root_permissions(&base, base.uid, base.gid).is_err());
        assert_eq!(mode(&unchanged), Some(0o644));
        let _ignored = fs::remove_dir_all(target);
    }

    #[test]
    fn nested_terminal_symlink_is_rejected() {
        let base = Fixture::new("nested-terminal");
        let session = base.join("selected");
        let target = base.join("terminal-target");
        assert_ok!(
            fs::create_dir_all(session.join("context")),
            fs::create_dir_all(&target),
            std::os::unix::fs::symlink(&target, session.join("context/terminal")),
        );
        assert!(plan_session_permissions(&session, PreflightRoot::Session).is_err());
    }

    #[test]
    fn missing_session_prepares_and_original_record_preserves_current_run_owner() {
        let session_root = Fixture::new("current-run-owner");
        let session = session_root.join("fresh");
        let preparation = session_root.prepare("fresh");
        assert!(preparation.is_ok());
        let Ok(preparation) = preparation else {
            return;
        };
        for (path, expected_mode) in [
            (session_root.root.clone(), 0o700),
            (session_root.join("index"), 0o700),
            (session_root.join("index/list"), 0o600),
            (session.clone(), 0o700),
        ] {
            assert_eq!(
                owner_mode(&path),
                Some((session_root.uid, session_root.gid, expected_mode))
            );
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
        assert_eq!(
            owner_mode(&session.join("current_run")),
            Some((session_root.uid, session_root.gid, 0o600))
        );
        assert_eq!(text(&session.join("current_run")).as_deref(), Some("run\n"));
    }

    #[test]
    fn root_runtime_repairs_private_agent_home_traversal_boundary()
    -> Result<(), Box<dyn std::error::Error>> {
        let base = Fixture::new("agent-home-owner");
        let shared_parent = base.join("home/1000/agent");
        let agent_home = shared_parent.join("example-echo");
        let session_root = agent_home.join("session");
        assert_ok!(
            fs::create_dir_all(&agent_home),
            fs::set_permissions(&shared_parent, fs::Permissions::from_mode(0o755)),
            fs::set_permissions(&agent_home, fs::Permissions::from_mode(0o700)),
        );
        assert!(
            prepare_owned_durable_session(
                &session_root,
                "live-sdk",
                "/workspace",
                None,
                SocketSessionScope::Private,
                base.uid,
                base.gid,
            )
            .is_ok()
        );
        for path in [&agent_home, &session_root, &session_root.join("live-sdk")] {
            let metadata = fs::symlink_metadata(path)?;
            assert!(!metadata.file_type().is_symlink());
            assert_eq!((metadata.uid(), metadata.gid()), (base.uid, base.gid));
            assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        }
        let metadata = fs::metadata(shared_parent)?;
        assert_eq!((metadata.uid(), metadata.gid()), (base.uid, base.gid));
        assert_eq!(metadata.permissions().mode() & 0o777, 0o755);
        Ok(())
    }

    #[test]
    fn failed_preflight_leaves_start_controls_unchanged_and_retryable() {
        let base = Fixture::new("repair-failure");
        let session = base.join("retry");
        assert_ok!(
            fs::create_dir_all(&session),
            fs::write(session.join("events.jsonl"), "before\n"),
            fs::write(session.join("current_run"), "old\n"),
        );
        let socket = session.join("z-invalid");
        let listener = UnixListener::bind(&socket);
        assert!(listener.is_ok());
        assert!(base.prepare("retry").is_err());
        assert_eq!(
            text(&session.join("events.jsonl")).as_deref(),
            Some("before\n")
        );
        assert_eq!(text(&session.join("current_run")).as_deref(), Some("old\n"));
        drop(listener);
        assert_ok!(fs::remove_file(socket));
        assert!(base.prepare("retry").is_ok());
    }

    #[test]
    fn shared_record_keeps_shared_layout_outside_private_prepare() {
        let session_root = Fixture::new("shared-semantics");
        let session = session_root.join("shared");
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
        assert_ok!(
            fs::write(&marker, "shared\n"),
            fs::set_permissions(&marker, fs::Permissions::from_mode(0o640)),
        );
        let before = owner_mode(&marker);
        assert!(!session.join("current_run").exists());
        let request = SocketRequest::Send {
            id: "run".to_owned(),
            session: "shared".to_owned(),
            scope: SocketSessionScope::Shared,
            cwd: Some("/workspace".to_owned()),
            workspace: None,
            input: "input".to_owned(),
            event: None,
            origin: None,
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
        assert_eq!(owner_mode(&marker), before);
        assert_eq!(
            text(&session.join("current_run")).as_deref(),
            run.as_ref().map(|run| format!("{run}\n")).as_deref()
        );
    }

    #[test]
    fn prepared_token_rejects_session_mismatch_and_current_run_replacement() {
        let root = Fixture::new("prepared-token");
        let preparation = root.prepare("one");
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
        assert_ok!(
            fs::remove_file(&current_run),
            fs::write(&current_run, "replacement\n"),
        );
        assert!(
            write_current_run_session_file(&root.join("one"), "run\n", Some(&preparation)).is_err()
        );
    }

    #[test]
    fn session_store_repair_keeps_archive_sentinel_mode() {
        let base = Fixture::new("session-store-archive-sentinel");
        assert_ok!(fs::create_dir_all(base.join("index")));
        let archive = base.join(".archive");
        assert_ok!(fs::create_dir_all(archive.join("old-session")));
        let archive_sentinel = archive.join("old-session/sentinel");
        let selected = base.join("selected");
        assert_ok!(
            fs::write(&archive_sentinel, "old"),
            fs::set_permissions(&archive_sentinel, fs::Permissions::from_mode(0o644)),
            fs::create_dir_all(&selected),
            fs::write(selected.join("state"), "state"),
        );
        assert!(repair_agent_session_root_permissions(&base, base.uid, base.gid).is_ok());
        assert_eq!(mode(&selected.join("state")), Some(0o600));
        assert_eq!(mode(&archive_sentinel), Some(0o644));
    }
}
