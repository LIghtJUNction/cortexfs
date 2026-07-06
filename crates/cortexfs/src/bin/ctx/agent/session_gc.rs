const DEFAULT_SESSION_GC_PATTERNS: &[&str] = &[
    "*smoke*",
    "e2e-*",
    "repro-*",
    "install-*",
    "deterministic-*",
    "ansi-*",
    "spark-*",
];

fn agent_session_gc(root: &Path, args: &AgentSessionGcArgs) -> Result<(), CliError> {
    require_cli_name("agent name", &args.name)?;
    for keep in &args.keep {
        require_cli_name("session name", keep)?;
    }
    let session_root = ctx_home(root)?.join("agent").join(&args.name).join("session");
    let delete_session_root = agent_session_gc_delete_session_root(root, &args.name, &session_root);
    let current = current_session_name(&session_root).unwrap_or_else(|_error| "default".to_owned());
    let protected = agent_session_gc_protected(args, &current);
    let patterns = agent_session_gc_patterns(args);
    let threshold = args
        .older_than_days
        .map(agent_session_gc_threshold)
        .transpose()?;
    let mut candidates = agent_session_gc_candidates(&session_root, &protected, &patterns, threshold)?;

    candidates.sort();
    if candidates.is_empty() {
        print_line("ctx: no matching agent sessions")?;
        return Ok(());
    }

    if args.dry_run || !args.yes {
        print_line("ctx: dry-run; pass --yes to delete")?;
        for session in candidates {
            print_line(&format!("delete {}", terminal_safe_text(&session)))?;
        }
        return Ok(());
    }

    for session in candidates {
        let path = delete_session_root.join(&session);
        remove_plain_session_dir(&path)?;
        print_line(&format!("deleted {}", terminal_safe_text(&session)))?;
    }
    Ok(())
}

fn agent_session_gc_delete_session_root(root: &Path, name: &str, session_root: &Path) -> PathBuf {
    const SYSTEM_STORAGE_ROOT: &str = "/var/lib/cortexfs/storage/v1-root";
    if root != Path::new("/ctx") {
        return session_root.to_path_buf();
    }
    let storage_root = Path::new(SYSTEM_STORAGE_ROOT);
    let Ok(home) = ctx_home(storage_root) else {
        return session_root.to_path_buf();
    };
    let storage_session_root = home.join("agent").join(name).join("session");
    if storage_session_root.is_dir() {
        storage_session_root
    } else {
        session_root.to_path_buf()
    }
}

fn agent_session_gc_protected(args: &AgentSessionGcArgs, current: &str) -> HashSet<String> {
    let mut protected = HashSet::from([
        "index".to_owned(),
        "default".to_owned(),
        current.to_owned(),
    ]);
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
        if !patterns.iter().any(|pattern| simple_glob_match(pattern, &name)) {
            continue;
        }
        if let Some(threshold) = threshold
            && session_latest_modified(&path)? >= threshold
        {
            continue;
        }
        candidates.push(name);
    }
    Ok(candidates)
}

fn remove_plain_session_dir(path: &Path) -> Result<(), CliError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| CliError::unavailable(format!("cannot stat {}: {error}", path.display())))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(CliError::unavailable(format!(
            "refusing non-plain session dir: {}",
            path.display()
        )));
    }
    fs::remove_dir_all(path)
        .map_err(|error| CliError::unavailable(format!("cannot remove {}: {error}", path.display())))
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
