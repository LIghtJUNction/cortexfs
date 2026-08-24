use crate::*;

const SESSION_ARCHIVE_DIR: &str = "archived_sessions";
const MAX_SESSION_GC_STATE_BYTES: u64 = 64;
const MAX_SESSION_GC_INDEX_BYTES: u64 = 64 * 1024;
const SYSTEM_STORAGE_ROOT: &str = cortexfs_paths::SYSTEM_STORAGE_CURRENT;

#[cfg(test)]
thread_local! {
    static GC_DELETE_FAULT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static GC_DELETE_SYNC_FAULT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static GC_LIST_PUBLISH_REPLACEMENT: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
    static GC_LIST_ROLLBACK_REPLACEMENT: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
    static GC_SOURCE_CLAIM_FAULT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

const DEFAULT_SESSION_GC_PATTERNS: &[&str] = &[
    "*smoke*",
    "e2e-*",
    "repro-*",
    "install-*",
    "deterministic-*",
    "ansi-*",
    "spark-*",
];

pub(crate) fn agent_session_gc(root: &Path, args: &AgentSessionGcArgs) -> Result<(), CliError> {
    require_cli_name("agent name", &args.name)?;
    validate_gc_archive_args(args.delete, args.archive_dir.as_deref())?;
    for keep in &args.keep {
        require_cli_name("session name", keep)?;
    }
    let session_root = gc_session_root(root, &args.name)?;
    let _index_guard = SessionIndexGuard::exclusive(&session_root).map_err(|error| {
        CliError::unavailable(format!("cannot lock session index: {}", error.errno()))
    })?;
    let current = current_session_name(&session_root)?;
    let protected = agent_session_gc_protected(args, &current);
    let patterns = agent_session_gc_patterns(args);
    let threshold = args
        .older_than_days
        .map(agent_session_gc_threshold)
        .transpose()?;
    let mut candidates =
        agent_session_gc_candidates(&session_root, &protected, &patterns, threshold)?;

    candidates.sort();
    if candidates.is_empty() {
        print_line("ctx: no matching agent sessions")?;
        return Ok(());
    }

    if args.dry_run || !args.yes {
        let action = if args.delete { "delete" } else { "archive" };
        print_line(&format!("ctx: dry-run; pass --yes to {action}"))?;
        for session in candidates {
            print_line(&format!("{action} {}", terminal_safe_text(&session)))?;
        }
        return Ok(());
    }

    let archive = (!args.delete)
        .then(|| gc_archive_agent_root(&session_root, &args.name, args.archive_dir.as_deref()))
        .transpose()?;
    apply_gc_candidates(&session_root, archive.as_deref(), args.delete, &candidates)
}

/// Archives exactly one inactive, non-current durable session.
pub(crate) fn agent_session_archive(
    root: &Path,
    args: &AgentSessionArchiveArgs,
) -> Result<(), CliError> {
    require_cli_name("agent name", &args.name)?;
    require_cli_name("session name", &args.session)?;
    validate_gc_archive_args(false, args.archive_dir.as_deref())?;
    let session_root = gc_session_root(root, &args.name)?;
    let _index_guard = SessionIndexGuard::exclusive(&session_root).map_err(|error| {
        CliError::unavailable(format!("cannot lock session index: {}", error.errno()))
    })?;
    let current = current_session_name(&session_root)?;
    if args.session == "default" || args.session == current {
        return Err(CliError::unavailable(format!(
            "refusing to archive protected session: {}",
            args.session
        )));
    }
    let candidates = vec![args.session.clone()];
    if gc_active_or_unsafe(&session_root.join(&args.session)) {
        return Err(CliError::unavailable(format!(
            "refusing to archive active or unsafe session: {}",
            args.session
        )));
    }
    let archive = gc_archive_agent_root(&session_root, &args.name, args.archive_dir.as_deref())?;
    apply_gc_candidates(&session_root, Some(&archive), false, &candidates)
}

/// Applies archive or delete to an already selected set of sessions.
fn apply_gc_candidates(
    session_root: &Path,
    archive_path: Option<&Path>,
    delete: bool,
    candidates: &[String],
) -> Result<(), CliError> {
    let sources = preflight_gc_sources(session_root, candidates)?;
    let existing_archive = archive_path
        .map(|archive| preflight_gc_archive(archive, candidates))
        .transpose()?
        .flatten();
    let mut index = preflight_gc_index(session_root, candidates)?;
    let archive = archive_path
        .map(|path| open_gc_archive(session_root, path, existing_archive))
        .transpose()?;
    let archive_display = archive_path.unwrap_or(session_root);

    for source in sources {
        let transaction = stage_gc_index(&index, &source.name)?;
        let claim = match claim_gc_source(session_root, &source) {
            Ok(claim) => claim,
            Err(error) => {
                return Err(gc_rollback_error(
                    error,
                    rollback_gc_index(&index, &transaction),
                ));
            }
        };
        let result = archive.as_ref().map_or_else(
            || delete_gc_source(session_root, &source, &claim),
            |archive| archive_gc_source(session_root, archive_display, archive, &source, &claim),
        );
        if let Err(failure) = result {
            match failure {
                GcSourceFailure::Committed(error) => {
                    let cleanup = commit_gc_index(&mut index, transaction);
                    return Err(gc_rollback_error(error, cleanup));
                }
                GcSourceFailure::Rollbackable(error) => {
                    let source_rollback = restore_gc_source(session_root, &source, &claim);
                    let index_rollback = rollback_gc_index(&index, &transaction);
                    return Err(gc_rollback_error(
                        error,
                        source_rollback.and(index_rollback),
                    ));
                }
            }
        }
        commit_gc_index(&mut index, transaction)?;
        let action = if delete { "deleted" } else { "archived" };
        print_line(&format!("{action} {}", terminal_safe_text(&source.name)))?;
    }
    Ok(())
}

/// Validates archive options at the execution boundary.
fn validate_gc_archive_args(delete: bool, archive_dir: Option<&Path>) -> Result<(), CliError> {
    if delete && archive_dir.is_some() {
        return Err(CliError::usage(
            "agent session gc --archive-dir cannot be used with --delete",
        ));
    }
    if archive_dir.is_some_and(|path| !path.is_absolute()) {
        return Err(CliError::usage("--archive-dir must be an absolute path"));
    }
    Ok(())
}

/// Resolves the per-agent archive directory.
pub(crate) fn gc_archive_agent_root(
    session_root: &Path,
    agent: &str,
    archive_dir: Option<&Path>,
) -> Result<PathBuf, CliError> {
    let root = match archive_dir {
        Some(path) => normalize_gc_path(path)?,
        None => session_root
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .ok_or_else(|| CliError::unavailable("cannot resolve owning CTX_HOME"))?
            .join(SESSION_ARCHIVE_DIR),
    };
    let archive = normalize_gc_path(&root.join(agent))?;
    let live = normalize_gc_path(session_root)?;
    if archive.starts_with(&live) || live.starts_with(&archive) {
        return Err(CliError::unavailable(format!(
            "session archive dir overlaps live session storage: {}",
            archive.display()
        )));
    }
    Ok(archive)
}

/// Lexically normalizes a path while rejecting parent traversal.
fn normalize_gc_path(path: &Path) -> Result<PathBuf, CliError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map_err(|error| CliError::unavailable(format!("cannot resolve current dir: {error}")))?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(CliError::usage(
                    "--archive-dir must not contain parent path components",
                ));
            }
        }
    }
    Ok(normalized)
}

pub(crate) fn agent_session_select(
    root: &Path,
    name: &str,
    target: &str,
    expected_current: &str,
) -> Result<(), CliError> {
    require_cli_name("agent name", name)?;
    require_cli_name("session name", target)?;
    require_cli_name("expected current session name", expected_current)?;
    let session_root = gc_session_root(root, name)?;
    compare_and_update_session_index(&session_root, target, expected_current).map_err(|error| {
        CliError::unavailable(format!(
            "cannot select session {target} from {expected_current}: {}",
            error.errno()
        ))
    })
}

fn gc_rollback_error(error: CliError, rollback: Result<(), CliError>) -> CliError {
    match rollback {
        Ok(()) => error,
        Err(rollback) => CliError {
            code: error.code,
            message: format!("{}; rollback failed: {}", error.message, rollback.message),
        },
    }
}

fn gc_session_root(root: &Path, name: &str) -> Result<PathBuf, CliError> {
    let session_root = cortexfs_paths::agent_sessions_from_home_path(&ctx_home(root)?, name);
    if root != Path::new(CTX_ROOT) {
        return Ok(session_root);
    }
    let Ok(relative) = session_root.strip_prefix(root) else {
        return Ok(session_root);
    };
    gc_storage_session_root(Path::new(SYSTEM_STORAGE_ROOT), relative)?.map_or(Ok(session_root), Ok)
}

pub(crate) fn gc_storage_session_root(
    storage: &Path,
    relative: &Path,
) -> Result<Option<PathBuf>, CliError> {
    let storage = pin_storage_source(storage)
        .map_err(|error| CliError::unavailable(format!("cannot pin storage current: {error}")))?;
    let session_root = storage.join(relative);
    Ok(plain_session_dir_exists(&session_root).then_some(session_root))
}

fn agent_session_gc_protected(args: &AgentSessionGcArgs, current: &str) -> HashSet<String> {
    let mut protected =
        HashSet::from(["index".to_owned(), "default".to_owned(), current.to_owned()]);
    protected.extend(args.keep.iter().cloned());
    protected
}

fn agent_session_gc_patterns(args: &AgentSessionGcArgs) -> Vec<String> {
    if args.patterns.is_empty() {
        DEFAULT_SESSION_GC_PATTERNS
            .iter()
            .map(|pattern| (*pattern).to_owned())
            .collect()
    } else {
        args.patterns.clone()
    }
}

fn agent_session_gc_threshold(days: u64) -> Result<SystemTime, CliError> {
    let seconds = days
        .checked_mul(24 * 60 * 60)
        .ok_or_else(|| CliError::usage("invalid --older-than-days value"))?;
    SystemTime::now()
        .checked_sub(Duration::from_secs(seconds))
        .ok_or_else(|| CliError::usage("invalid --older-than-days value"))
}

fn agent_session_gc_candidates(
    session_root: &Path,
    protected: &HashSet<String>,
    patterns: &[String],
    threshold: Option<SystemTime>,
) -> Result<Vec<String>, CliError> {
    let entries = match fs::read_dir(session_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot read {}: {error}",
                session_root.display()
            )));
        }
    };
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            CliError::unavailable(format!("cannot read {}: {error}", session_root.display()))
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if protected.contains(&name) || !is_object_name(&name) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        if !patterns
            .iter()
            .any(|pattern| simple_glob_match(pattern, &name))
        {
            continue;
        }
        if let Some(threshold) = threshold
            && session_latest_modified(&path)? >= threshold
        {
            continue;
        }
        if gc_active_or_unsafe(&path) {
            continue;
        }
        candidates.push(name);
    }
    Ok(candidates)
}

fn gc_active_or_unsafe(path: &Path) -> bool {
    match read_small_plain_text_file(
        &path.join("state"),
        MAX_SESSION_GC_STATE_BYTES,
        "session state",
    ) {
        Ok(state) => state.trim() == "active",
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(_error) => true,
    }
}

struct GcIndexEntry {
    path: PathBuf,
    name: String,
    content: String,
    session: String,
    dev: u64,
    ino: u64,
}

#[derive(Clone)]
struct GcListReceipt {
    content: String,
    dev: u64,
    ino: u64,
}

struct GcIndexClaim {
    parent: PathBuf,
    original: String,
    claimed: String,
    content: String,
    dev: u64,
    ino: u64,
}

struct GcIndexTransaction {
    old_list: GcListReceipt,
    new_list: GcListReceipt,
    claims: Vec<GcIndexClaim>,
    session: String,
}

struct GcSource {
    name: String,
    dev: u64,
    ino: u64,
}

struct GcSourceClaim {
    name: String,
}

enum GcSourceFailure {
    Rollbackable(CliError),
    Committed(CliError),
}

struct GcIndex {
    list_path: PathBuf,
    list: GcListReceipt,
    entries: Vec<GcIndexEntry>,
}

fn preflight_gc_sources(
    session_root: &Path,
    candidates: &[String],
) -> Result<Vec<GcSource>, CliError> {
    open_plain_directory(session_root).map_err(|error| {
        CliError::unavailable(format!(
            "cannot open session root {}: {error}",
            session_root.display()
        ))
    })?;
    let mut sources = Vec::with_capacity(candidates.len());
    for session in candidates {
        let path = session_root.join(session);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(CliError::unavailable(format!(
                "refusing non-plain session dir: {}",
                path.display()
            )));
        }
        sources.push(GcSource {
            name: session.clone(),
            dev: metadata.dev(),
            ino: metadata.ino(),
        });
    }
    Ok(sources)
}

fn preflight_gc_archive(
    archive: &Path,
    candidates: &[String],
) -> Result<Option<fs::File>, CliError> {
    let directory = match open_plain_directory(archive) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot open session archive {}: {error}",
                archive.display()
            )));
        }
    };
    for session in candidates {
        match nix::sys::stat::fstatat(
            &directory,
            session.as_str(),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        ) {
            Err(nix::errno::Errno::ENOENT) => {}
            Ok(_stat) => {
                return Err(CliError::unavailable(format!(
                    "session archive already exists: {}",
                    archive.join(session).display()
                )));
            }
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot stat {}: {error}",
                    archive.join(session).display()
                )));
            }
        }
    }
    Ok(Some(directory))
}

fn open_gc_archive(
    session_root: &Path,
    archive: &Path,
    existing: Option<fs::File>,
) -> Result<fs::File, CliError> {
    let directory = if let Some(existing) = existing {
        existing
    } else {
        create_plain_directory(
            archive,
            0o700,
            "session archive path is not a plain directory",
            "session archive path contains a non-directory entry",
            "invalid session archive directory name",
        )
        .map_err(|error| {
            CliError::unavailable(format!(
                "cannot create session archive {}: {error}",
                archive.display()
            ))
        })?;
        open_plain_directory(archive).map_err(|error| {
            CliError::unavailable(format!(
                "cannot open session archive {}: {error}",
                archive.display()
            ))
        })?
    };
    let session_dev = open_plain_directory(session_root)
        .and_then(|directory| directory.metadata())
        .map_err(|error| {
            CliError::unavailable(format!(
                "cannot stat session root {}: {error}",
                session_root.display()
            ))
        })?
        .dev();
    let archive_dev = directory.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", archive.display()))
    })?;
    if session_dev != archive_dev.dev() {
        return Err(CliError::unavailable(format!(
            "session archive is on another filesystem: {}",
            archive.display()
        )));
    }
    Ok(directory)
}

fn validate_gc_source(session_root: &Path, source: &GcSource) -> Result<(), CliError> {
    let path = session_root.join(&source.name);
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || (metadata.dev(), metadata.ino()) != (source.dev, source.ino)
        || gc_active_or_unsafe(&path)
    {
        return Err(CliError::unavailable(format!(
            "session became active, unsafe, or changed during gc: {}",
            path.display()
        )));
    }
    Ok(())
}

fn claim_gc_source(session_root: &Path, source: &GcSource) -> Result<GcSourceClaim, CliError> {
    validate_gc_source(session_root, source)?;
    #[cfg(test)]
    if GC_SOURCE_CLAIM_FAULT.with(|fault| fault.replace(false)) {
        return Err(CliError::unavailable("injected session claim failure"));
    }
    let parent = open_plain_directory(session_root).map_err(|error| {
        CliError::unavailable(format!(
            "cannot open session root {}: {error}",
            session_root.display()
        ))
    })?;
    for attempt in 0..16_u8 {
        let name = cortexfs::support::atomic::generated_sibling_name(&source.name, "gc", attempt);
        match nix::fcntl::renameat2(
            &parent,
            source.name.as_str(),
            &parent,
            name.as_str(),
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        ) {
            Ok(()) => {
                let claim = GcSourceClaim { name };
                let claimed = parent
                    .sync_all()
                    .map_err(|error| {
                        CliError::unavailable(format!(
                            "cannot sync session root {}: {error}",
                            session_root.display()
                        ))
                    })
                    .and_then(|()| validate_gc_source_claim(session_root, source, &claim));
                if let Err(error) = claimed {
                    return Err(gc_rollback_error(
                        error,
                        restore_gc_source(session_root, source, &claim),
                    ));
                }
                return Ok(claim);
            }
            Err(nix::errno::Errno::EEXIST) => {}
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot claim {}: {error}",
                    session_root.join(&source.name).display()
                )));
            }
        }
    }
    Err(CliError::unavailable(format!(
        "cannot create session claim for {}",
        session_root.join(&source.name).display()
    )))
}

fn validate_gc_source_claim(
    session_root: &Path,
    source: &GcSource,
    claim: &GcSourceClaim,
) -> Result<(), CliError> {
    let directory = open_gc_source_claim(session_root, source, claim)?;
    let active_or_unsafe = match cortexfs::support::plain::read_small_text_file_at(
        &directory,
        "state",
        MAX_SESSION_GC_STATE_BYTES,
        "invalid session state",
    ) {
        Ok(state) => state.trim() == "active",
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(_error) => true,
    };
    if active_or_unsafe {
        return Err(CliError::unavailable(format!(
            "session claim became active or unsafe during gc: {}",
            session_root.join(&claim.name).display()
        )));
    }
    Ok(())
}

fn open_gc_source_claim(
    session_root: &Path,
    source: &GcSource,
    claim: &GcSourceClaim,
) -> Result<fs::File, CliError> {
    let parent = open_plain_directory(session_root).map_err(|error| {
        CliError::unavailable(format!(
            "cannot open session root {}: {error}",
            session_root.display()
        ))
    })?;
    let fd = nix::fcntl::openat(
        &parent,
        claim.name.as_str(),
        nix::fcntl::OFlag::O_DIRECTORY
            | nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|error| {
        CliError::unavailable(format!(
            "cannot open session claim {}: {error}",
            session_root.join(&claim.name).display()
        ))
    })?;
    let directory = fs::File::from(fd);
    let metadata = directory.metadata().map_err(|error| {
        CliError::unavailable(format!(
            "cannot stat session claim {}: {error}",
            session_root.join(&claim.name).display()
        ))
    })?;
    if (metadata.dev(), metadata.ino()) != (source.dev, source.ino) {
        return Err(CliError::unavailable(format!(
            "session claim changed during gc: {}",
            session_root.join(&claim.name).display()
        )));
    }
    Ok(directory)
}

fn restore_gc_source(
    session_root: &Path,
    source: &GcSource,
    claim: &GcSourceClaim,
) -> Result<(), CliError> {
    open_gc_source_claim(session_root, source, claim)?;
    let parent = open_plain_directory(session_root).map_err(|error| {
        CliError::unavailable(format!(
            "cannot open session root {}: {error}",
            session_root.display()
        ))
    })?;
    nix::fcntl::renameat2(
        &parent,
        claim.name.as_str(),
        &parent,
        source.name.as_str(),
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|error| {
        CliError::unavailable(format!(
            "cannot restore session {}: {error}",
            session_root.join(&source.name).display()
        ))
    })?;
    parent.sync_all().map_err(|error| {
        CliError::unavailable(format!(
            "cannot sync session root {}: {error}",
            session_root.display()
        ))
    })
}

fn archive_gc_source(
    session_root: &Path,
    archive_path: &Path,
    archive: &fs::File,
    source: &GcSource,
    claim: &GcSourceClaim,
) -> Result<(), GcSourceFailure> {
    validate_gc_source_claim(session_root, source, claim).map_err(GcSourceFailure::Rollbackable)?;
    let parent = open_plain_directory(session_root).map_err(|error| {
        GcSourceFailure::Rollbackable(CliError::unavailable(format!(
            "cannot open session root {}: {error}",
            session_root.display()
        )))
    })?;
    nix::fcntl::renameat2(
        &parent,
        claim.name.as_str(),
        archive,
        source.name.as_str(),
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|error| {
        GcSourceFailure::Rollbackable(CliError::unavailable(format!(
            "cannot archive {}: {error}",
            session_root.join(&source.name).display()
        )))
    })?;
    archive
        .sync_all()
        .and_then(|()| parent.sync_all())
        .map_err(|error| {
            GcSourceFailure::Committed(CliError::unavailable(format!(
                "cannot sync session archive {}: {error}",
                archive_path.display()
            )))
        })
}

fn delete_gc_source(
    session_root: &Path,
    source: &GcSource,
    claim: &GcSourceClaim,
) -> Result<(), GcSourceFailure> {
    validate_gc_source_claim(session_root, source, claim).map_err(GcSourceFailure::Rollbackable)?;
    let parent = open_plain_directory(session_root).map_err(|error| {
        GcSourceFailure::Rollbackable(CliError::unavailable(format!(
            "cannot open session root {}: {error}",
            session_root.display()
        )))
    })?;
    let path = session_root.join(&claim.name);
    remove_gc_quarantine(&path).map_err(|error| {
        GcSourceFailure::Committed(CliError::unavailable(format!(
            "cannot remove {}: {error}; delete is irreversible; quarantine path: {}",
            path.display(),
            path.display()
        )))
    })?;
    sync_gc_delete_parent(&parent).map_err(|error| {
        GcSourceFailure::Committed(CliError::unavailable(format!(
            "cannot sync session root {}: {error}; delete is irreversible; quarantine already removed: {}",
            session_root.display(),
            path.display()
        )))
    })
}

fn remove_gc_quarantine(path: &Path) -> io::Result<()> {
    #[cfg(test)]
    if GC_DELETE_FAULT.with(|fault| fault.replace(false)) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "injected recursive delete failure",
        ));
    }
    fs::remove_dir_all(path)
}

fn sync_gc_delete_parent(parent: &fs::File) -> io::Result<()> {
    #[cfg(test)]
    if GC_DELETE_SYNC_FAULT.with(|fault| fault.replace(false)) {
        return Err(io::Error::other("injected delete parent sync failure"));
    }
    parent.sync_all()
}

#[cfg(test)]
pub(crate) fn set_gc_delete_fault_for_test(enabled: bool) {
    GC_DELETE_FAULT.with(|fault| fault.set(enabled));
}

#[cfg(test)]
pub(crate) fn set_gc_delete_sync_fault_for_test(enabled: bool) {
    GC_DELETE_SYNC_FAULT.with(|fault| fault.set(enabled));
}

#[cfg(test)]
pub(crate) fn set_gc_list_publish_replacement_for_test(replacement: Option<PathBuf>) {
    GC_LIST_PUBLISH_REPLACEMENT.with(|slot| *slot.borrow_mut() = replacement);
}

#[cfg(test)]
pub(crate) fn set_gc_list_rollback_replacement_for_test(replacement: Option<PathBuf>) {
    GC_LIST_ROLLBACK_REPLACEMENT.with(|slot| *slot.borrow_mut() = replacement);
}

#[cfg(test)]
pub(crate) fn set_gc_source_claim_fault_for_test(enabled: bool) {
    GC_SOURCE_CLAIM_FAULT.with(|fault| fault.set(enabled));
}

fn preflight_gc_index(session_root: &Path, candidates: &[String]) -> Result<GcIndex, CliError> {
    let index = session_root.join("index");
    let metadata = fs::symlink_metadata(&index).map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", index.display()))
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(CliError::unavailable(format!(
            "refusing non-plain session index dir: {}",
            index.display()
        )));
    }
    let list_path = index.join("list");
    let list = read_gc_list(&list_path)?;
    if !inspect_session_index(SessionIndexKind::List, &list.content).is_ok() {
        return Err(CliError::unavailable(format!(
            "invalid session index: {}",
            list_path.display()
        )));
    }

    let candidate_set = candidates
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut entries = Vec::new();
    for (directory, kind) in [
        ("by-cwd", SessionIndexKind::ByCwd),
        ("by-hash", SessionIndexKind::ByHash),
        ("by-uuid", SessionIndexKind::ByUuid),
    ] {
        let directory = index.join(directory);
        let directory_fd = match open_plain_directory(&directory) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot open session index dir {}: {error}",
                    directory.display()
                )));
            }
        };
        let directory_entries = fs::read_dir(proc_fd_path(&directory_fd)).map_err(|error| {
            CliError::unavailable(format!("cannot read {}: {error}", directory.display()))
        })?;
        for entry in directory_entries {
            let entry = entry.map_err(|error| {
                CliError::unavailable(format!("cannot read {}: {error}", directory.display()))
            })?;
            let name = entry.file_name().into_string().map_err(|_name| {
                CliError::unavailable(format!(
                    "invalid session index path under {}",
                    directory.display()
                ))
            })?;
            if name.starts_with('.') {
                continue;
            }
            let path = directory.join(&name);
            let stat = nix::sys::stat::fstatat(
                &directory_fd,
                name.as_str(),
                nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
            )
            .map_err(|error| {
                CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
            })?;
            if !nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
                .contains(nix::sys::stat::SFlag::S_IFREG)
            {
                continue;
            }
            let content = cortexfs::support::plain::read_small_text_file_at(
                &directory_fd,
                &name,
                MAX_SESSION_GC_INDEX_BYTES,
                "invalid session index file",
            )
            .map_err(|error| {
                CliError::unavailable(format!("cannot read {}: {error}", path.display()))
            })?;
            if !inspect_session_index(kind, &content).is_ok() {
                return Err(CliError::unavailable(format!(
                    "invalid session index: {}",
                    path.display()
                )));
            }
            let session = content.lines().next().unwrap_or_default().to_owned();
            if candidate_set.contains(session.as_str()) {
                entries.push(GcIndexEntry {
                    path,
                    name,
                    content,
                    session,
                    dev: stat.st_dev,
                    ino: stat.st_ino,
                });
            }
        }
    }
    preflight_gc_channels(session_root, &candidate_set, &mut entries)?;
    Ok(GcIndex {
        list_path,
        list,
        entries,
    })
}

fn preflight_gc_channels(
    session_root: &Path,
    candidates: &HashSet<&str>,
    entries: &mut Vec<GcIndexEntry>,
) -> Result<(), CliError> {
    let directory = cortexfs_paths::session_channel_index_path(session_root);
    let directory_fd = match open_plain_directory(&directory) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot open session channel index dir {}: {error}",
                directory.display()
            )));
        }
    };
    for entry in fs::read_dir(proc_fd_path(&directory_fd)).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", directory.display()))
    })? {
        let entry = entry.map_err(|error| {
            CliError::unavailable(format!("cannot read {}: {error}", directory.display()))
        })?;
        let name = entry.file_name().into_string().map_err(|_name| {
            CliError::unavailable(format!(
                "invalid session channel index path under {}",
                directory.display()
            ))
        })?;
        if name.starts_with('.') {
            continue;
        }
        let path = directory.join(&name);
        let stat = nix::sys::stat::fstatat(
            &directory_fd,
            name.as_str(),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|error| {
            CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
        })?;
        if !nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
            .contains(nix::sys::stat::SFlag::S_IFREG)
        {
            continue;
        }
        let content = cortexfs::support::plain::read_small_text_file_at(
            &directory_fd,
            &name,
            MAX_SESSION_GC_INDEX_BYTES,
            "invalid session channel index file",
        )
        .map_err(|error| {
            CliError::unavailable(format!("cannot read {}: {error}", path.display()))
        })?;
        let session = inspect_gc_channel(&content, &name).ok_or_else(|| {
            CliError::unavailable(format!("invalid session channel index: {}", path.display()))
        })?;
        if candidates.contains(session.as_str()) {
            entries.push(GcIndexEntry {
                path,
                name,
                content,
                session,
                dev: stat.st_dev,
                ino: stat.st_ino,
            });
        }
    }
    Ok(())
}

fn inspect_gc_channel(content: &str, filename: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(content).ok()?;
    let object = value.as_object()?;
    (object.get("version").and_then(serde_json::Value::as_u64) == Some(1)).then_some(())?;
    (object.get("name").and_then(serde_json::Value::as_str) == Some(filename)).then_some(())?;
    object
        .get("agent")
        .and_then(serde_json::Value::as_str)
        .filter(|agent| is_object_name(agent))?;
    let scope = object.get("scope").and_then(serde_json::Value::as_str)?;
    matches!(scope, "private" | "shared").then_some(())?;
    object
        .get("session")
        .and_then(serde_json::Value::as_str)
        .filter(|session| is_object_name(session))
        .map(str::to_owned)
}

fn read_gc_list(path: &Path) -> Result<GcListReceipt, CliError> {
    let before = fs::symlink_metadata(path).map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    let content = read_small_plain_text_file(path, MAX_SESSION_GC_INDEX_BYTES, "session index")
        .map_err(|error| {
            CliError::unavailable(format!("cannot read {}: {error}", path.display()))
        })?;
    let after = fs::symlink_metadata(path).map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    if !before.is_file()
        || before.file_type().is_symlink()
        || (before.dev(), before.ino()) != (after.dev(), after.ino())
    {
        return Err(CliError::unavailable(format!(
            "session index changed during gc: {}",
            path.display()
        )));
    }
    Ok(GcListReceipt {
        content,
        dev: after.dev(),
        ino: after.ino(),
    })
}

fn validate_gc_list(path: &Path, expected: &GcListReceipt) -> Result<(), CliError> {
    let actual = read_gc_list(path)?;
    if actual.content != expected.content
        || actual.dev != expected.dev
        || actual.ino != expected.ino
    {
        return Err(CliError::unavailable(format!(
            "session index changed during gc: {}",
            path.display()
        )));
    }
    Ok(())
}

fn validate_gc_index(index: &GcIndex, session: &str) -> Result<(), CliError> {
    validate_gc_list(&index.list_path, &index.list)?;
    for entry in index
        .entries
        .iter()
        .filter(|entry| entry.session == session)
    {
        validate_gc_index_entry(entry)?;
    }
    Ok(())
}

fn validate_gc_index_entry(entry: &GcIndexEntry) -> Result<(), CliError> {
    let metadata = fs::symlink_metadata(&entry.path).map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", entry.path.display()))
    })?;
    let content =
        read_small_plain_text_file(&entry.path, MAX_SESSION_GC_INDEX_BYTES, "session index")
            .map_err(|error| {
                CliError::unavailable(format!("cannot read {}: {error}", entry.path.display()))
            })?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || (metadata.dev(), metadata.ino()) != (entry.dev, entry.ino)
        || content != entry.content
    {
        return Err(CliError::unavailable(format!(
            "session index changed during gc: {}",
            entry.path.display()
        )));
    }
    Ok(())
}

fn stage_gc_index(index: &GcIndex, session: &str) -> Result<GcIndexTransaction, CliError> {
    validate_gc_index(index, session)?;
    let mut claims = Vec::new();
    for entry in index
        .entries
        .iter()
        .filter(|entry| entry.session == session)
    {
        match claim_gc_index_entry(entry) {
            Ok(claim) => claims.push(claim),
            Err(error) => {
                return Err(gc_rollback_error(error, restore_gc_index_claims(&claims)));
            }
        }
    }

    let sessions = index
        .list
        .content
        .lines()
        .filter(|existing| *existing != session)
        .collect::<Vec<_>>();
    let next_content = if sessions.is_empty() {
        String::new()
    } else {
        format!("{}\n", sessions.join("\n"))
    };
    let new_list = if next_content == index.list.content {
        index.list.clone()
    } else {
        if let Err(error) = atomic_replace_text_preserving_metadata(&index.list_path, &next_content)
        {
            let error = CliError::unavailable(format!(
                "cannot update {}: {error}",
                index.list_path.display()
            ));
            return Err(gc_rollback_error(error, restore_gc_index_claims(&claims)));
        }
        #[cfg(test)]
        if let Some(replacement) = GC_LIST_PUBLISH_REPLACEMENT.with(|slot| slot.borrow_mut().take())
            && let Err(error) = fs::rename(replacement, &index.list_path)
        {
            let error = CliError::unavailable(format!(
                "cannot inject session index replacement {}: {error}; rollback conflict: published session index receipt could not be proven",
                index.list_path.display()
            ));
            return Err(gc_rollback_error(error, restore_gc_index_claims(&claims)));
        }
        match read_gc_list(&index.list_path) {
            Ok(receipt) if receipt.content == next_content => receipt,
            Ok(_receipt) => {
                let error = CliError::unavailable(format!(
                    "session index changed during gc: {}; rollback conflict: published session index receipt could not be proven",
                    index.list_path.display()
                ));
                return Err(gc_rollback_error(error, restore_gc_index_claims(&claims)));
            }
            Err(error) => {
                let error = CliError {
                    code: error.code,
                    message: format!(
                        "{}; rollback conflict: published session index receipt could not be proven",
                        error.message
                    ),
                };
                return Err(gc_rollback_error(error, restore_gc_index_claims(&claims)));
            }
        }
    };
    Ok(GcIndexTransaction {
        old_list: index.list.clone(),
        new_list,
        claims,
        session: session.to_owned(),
    })
}

fn claim_gc_index_entry(entry: &GcIndexEntry) -> Result<GcIndexClaim, CliError> {
    validate_gc_index_entry(entry)?;
    let parent_path = entry.path.parent().ok_or_else(|| {
        CliError::unavailable(format!(
            "invalid session index path: {}",
            entry.path.display()
        ))
    })?;
    let parent = open_plain_directory(parent_path).map_err(|error| {
        CliError::unavailable(format!("cannot open {}: {error}", parent_path.display()))
    })?;
    for attempt in 0..16_u8 {
        let claimed = cortexfs::support::atomic::generated_sibling_name(&entry.name, "gc", attempt);
        match nix::fcntl::renameat2(
            &parent,
            entry.name.as_str(),
            &parent,
            claimed.as_str(),
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        ) {
            Ok(()) => {
                let claim = GcIndexClaim {
                    parent: parent_path.to_path_buf(),
                    original: entry.name.clone(),
                    claimed,
                    content: entry.content.clone(),
                    dev: entry.dev,
                    ino: entry.ino,
                };
                let claimed = parent
                    .sync_all()
                    .map_err(|error| {
                        CliError::unavailable(format!(
                            "cannot sync {}: {error}",
                            parent_path.display()
                        ))
                    })
                    .and_then(|()| validate_gc_index_claim(&claim));
                if let Err(error) = claimed {
                    return Err(gc_rollback_error(error, restore_gc_index_claim(&claim)));
                }
                return Ok(claim);
            }
            Err(nix::errno::Errno::EEXIST) => {}
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot claim {}: {error}",
                    entry.path.display()
                )));
            }
        }
    }
    Err(CliError::unavailable(format!(
        "cannot create session index claim for {}",
        entry.path.display()
    )))
}

fn validate_gc_index_claim(claim: &GcIndexClaim) -> Result<(), CliError> {
    let parent = open_plain_directory(&claim.parent).map_err(|error| {
        CliError::unavailable(format!("cannot open {}: {error}", claim.parent.display()))
    })?;
    let stat = nix::sys::stat::fstatat(
        &parent,
        claim.claimed.as_str(),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|error| {
        CliError::unavailable(format!(
            "cannot stat session index claim {}: {error}",
            claim.claimed
        ))
    })?;
    let content = cortexfs::support::plain::read_small_text_file_at(
        &parent,
        &claim.claimed,
        MAX_SESSION_GC_INDEX_BYTES,
        "invalid session index claim",
    )
    .map_err(|error| {
        CliError::unavailable(format!(
            "cannot read session index claim {}: {error}",
            claim.claimed
        ))
    })?;
    if (stat.st_dev, stat.st_ino) != (claim.dev, claim.ino) || content != claim.content {
        return Err(CliError::unavailable(format!(
            "session index claim changed during gc: {}",
            claim.claimed
        )));
    }
    Ok(())
}

fn restore_gc_index_claim(claim: &GcIndexClaim) -> Result<(), CliError> {
    validate_gc_index_claim(claim)?;
    let parent = open_plain_directory(&claim.parent).map_err(|error| {
        CliError::unavailable(format!("cannot open {}: {error}", claim.parent.display()))
    })?;
    nix::fcntl::renameat2(
        &parent,
        claim.claimed.as_str(),
        &parent,
        claim.original.as_str(),
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|error| {
        CliError::unavailable(format!(
            "cannot restore session index {}: {error}",
            claim.parent.join(&claim.original).display()
        ))
    })?;
    parent.sync_all().map_err(|error| {
        CliError::unavailable(format!("cannot sync {}: {error}", claim.parent.display()))
    })
}

fn restore_gc_index_claims(claims: &[GcIndexClaim]) -> Result<(), CliError> {
    let mut failure = None;
    for claim in claims.iter().rev() {
        if let Err(error) = restore_gc_index_claim(claim)
            && failure.is_none()
        {
            failure = Some(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

fn rollback_gc_index(index: &GcIndex, transaction: &GcIndexTransaction) -> Result<(), CliError> {
    let claims = restore_gc_index_claims(&transaction.claims);
    let list = if transaction.old_list.content == transaction.new_list.content {
        Ok(())
    } else {
        restore_gc_list_content(
            &index.list_path,
            &transaction.new_list,
            &transaction.old_list.content,
        )
        .map_err(|error| CliError {
            code: error.code,
            message: format!(
                "{}; rollback conflict: list restore was not committed",
                error.message
            ),
        })
    };
    list.and(claims)
}

fn restore_gc_list_content(
    path: &Path,
    expected: &GcListReceipt,
    content: &str,
) -> Result<(), CliError> {
    validate_gc_list(path, expected)?;
    #[cfg(test)]
    if let Some(replacement) = GC_LIST_ROLLBACK_REPLACEMENT.with(|slot| slot.borrow_mut().take())
        && let Err(error) = fs::rename(replacement, path)
    {
        return Err(CliError::unavailable(format!(
            "cannot inject session index rollback replacement {}: {error}",
            path.display()
        )));
    }
    atomic_replace_text_preserving_metadata_if_matches(path, content, (expected.dev, expected.ino))
        .map_err(|error| {
            CliError::unavailable(format!("cannot restore {}: {error}", path.display()))
        })?;
    let restored = read_gc_list(path)?;
    if restored.content != content {
        return Err(CliError::unavailable(format!(
            "cannot restore session index: {}",
            path.display()
        )));
    }
    Ok(())
}

fn commit_gc_index(index: &mut GcIndex, transaction: GcIndexTransaction) -> Result<(), CliError> {
    index.list = transaction.new_list;
    index
        .entries
        .retain(|entry| entry.session != transaction.session);
    for claim in transaction.claims {
        validate_gc_index_claim(&claim)?;
        let parent = open_plain_directory(&claim.parent).map_err(|error| {
            CliError::unavailable(format!("cannot open {}: {error}", claim.parent.display()))
        })?;
        nix::unistd::unlinkat(
            &parent,
            claim.claimed.as_str(),
            nix::unistd::UnlinkatFlags::NoRemoveDir,
        )
        .map_err(|error| {
            CliError::unavailable(format!(
                "cannot remove session index claim {}: {error}",
                claim.claimed
            ))
        })?;
        parent.sync_all().map_err(|error| {
            CliError::unavailable(format!("cannot sync {}: {error}", claim.parent.display()))
        })?;
    }
    Ok(())
}

fn session_latest_modified(path: &Path) -> Result<SystemTime, CliError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            return Ok(SystemTime::UNIX_EPOCH);
        }
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot stat {}: {error}",
                path.display()
            )));
        }
    };
    let mut latest = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(latest);
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return Ok(latest),
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot read {}: {error}",
                path.display()
            )));
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => continue,
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot read {}: {error}",
                    path.display()
                )));
            }
        };
        latest = latest.max(session_latest_modified(&entry.path())?);
    }
    Ok(latest)
}

fn simple_glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }
    let mut rest = value;
    if let Some(first) = parts.first()
        && !first.is_empty()
    {
        let Some(stripped) = rest.strip_prefix(first) else {
            return false;
        };
        rest = stripped;
    }
    for part in parts.iter().skip(1).take(parts.len().saturating_sub(2)) {
        if part.is_empty() {
            continue;
        }
        let Some(index) = rest.find(part) else {
            return false;
        };
        let Some(next) = rest.get(index + part.len()..) else {
            return false;
        };
        rest = next;
    }
    if let Some(last) = parts.last()
        && !last.is_empty()
    {
        return rest.ends_with(last);
    }
    true
}
