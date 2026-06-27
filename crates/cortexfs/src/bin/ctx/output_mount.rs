fn print_help() -> Result<(), CliError> {
    print_help_lines(&[
        "ctx - CortexFS filesystem management CLI",
        "",
        "usage:",
        "  ctx [--root PATH] status",
        "  ctx [--root PATH] abi",
        "  ctx [--root PATH] env",
        "  ctx [--root PATH] root",
        "  ctx man [agent|tool|model|ctx|root|session|provider]",
        "  ctx bootstrap [SOURCE]",
        "  ctx update [SOURCE]",
        "  ctx [--root PATH] mount [--source SOURCE] [MOUNTPOINT]",
        "  ctx [--root PATH] ls [PATH|model|agent|tool]",
        "  ctx [--root PATH] which model|agent|tool NAME",
        "  ctx [--root PATH] path shared NAME",
        "  ctx [--root PATH] history AGENT [--session SESSION|SESSION]",
        "  ctx [--root PATH] resume AGENT [--session SESSION|SESSION]",
        "  ctx [--root PATH] send AGENT INPUT",
        "  ctx [--root PATH] send AGENT [--session SESSION] INPUT",
        "  ctx [--root PATH] send AGENT SESSION INPUT",
        "  ctx [--root PATH] agent new NAME [--temp] [--label LABEL] [--model MODEL] [--tool TOOL] [--shared NAME:read|write] [--mount SOURCE TARGET ro|rw]",
        "  ctx [--root PATH] agent start NAME [--session SESSION] [--cwd PATH] [--mount SOURCE TARGET ro|rw] [--no-default-workspace]",
        "  ctx [--root PATH] agent stop NAME",
        "  ctx [--root PATH] agent status NAME",
        "  ctx [--root PATH] agent ps",
        "  ctx [--root PATH] agent send NAME [--session SESSION] [--raw] INPUT",
        "  ctx [--root PATH] agent repl NAME [--session SESSION] [--raw]",
        "  ctx [--root PATH] agent resume NAME [--session SESSION] [--raw]",
        "  ctx [--root PATH] agent history NAME [--session SESSION]",
        "  ctx [--root PATH] agent output NAME [--session SESSION]",
        "  ctx [--root PATH] agent pack NAME [--session SESSION]",
        "  ctx [--root PATH] agent prompt NAME",
        "  ctx [--root PATH] agent tools NAME",
        "  ctx [--root PATH] agent children NAME [--session SESSION]",
        "  ctx [--root PATH] agent cancel NAME [--session SESSION] [--raw] [RUN]",
        "  ctx [--root PATH] agent watch NAME [--session SESSION]",
        "  ctx [--root PATH] agent attach NAME [--session SESSION]",
        "  ctx provider oauth login PROVIDER [--timeout SECONDS]",
        "  ctx provider oauth status PROVIDER",
        "  ctx provider oauth refresh PROVIDER",
        "  ctx provider secret set PROVIDER [--slot SLOT]",
        "  ctx provider secret status PROVIDER [--slot SLOT]",
        "  ctx provider preset list",
        "  ctx provider preset show openai|codex|anthropic|google",
        "  ctx provider preset install openai|codex|anthropic|google",
        "  ctx [--root PATH] ping model/NAME|agent/NAME",
        "  ctx [--root PATH] cancel model/NAME|agent/NAME RUN",
        "  ctx [--root PATH] exec model/NAME|agent/NAME|tool/NAME [ARG...]",
        "  ctx [--root PATH] tool NAME [ARG...]",
        "  ctx [--root PATH] cat PATH",
        "  ctx [--root PATH] set PATH VALUE",
        "  ctx [--root PATH] append PATH VALUE",
        "  ctx [--root PATH] file PATH",
        "  ctx [--root PATH] file info PATH",
        "  ctx [--root PATH] file type PATH",
        "  ctx [--root PATH] file check PATH",
        "  ctx [--root PATH] doctor",
        "  ctx validate-name NAME",
        "",
        "principles:",
        "  ctx is a thin Unix client over /ctx",
        "  provider config and OAuth credentials stay outside the /ctx root ABI",
    ])
}

fn print_man(root: &Path, topic: Option<&str>) -> Result<(), CliError> {
    let Some(topic) = topic else {
        return print_shared_or_builtin_man(root, MANUAL_INDEX_FILE, MANUAL_INDEX);
    };
    let Some(manual) = cortexfs_manual(topic) else {
        return Err(CliError::usage(format!(
            "unknown man topic: {topic}; expected agent, tool, model, ctx, root, session, or provider"
        )));
    };
    print_shared_or_builtin_man(root, manual.file_name, manual.content)
}

fn print_shared_or_builtin_man(
    root: &Path,
    file_name: &str,
    fallback: &str,
) -> Result<(), CliError> {
    let shared = root
        .join("shared")
        .join(MANUAL_SHARED_DIR)
        .join(if file_name == MANUAL_INDEX_FILE {
            ""
        } else {
            MANUAL_MAN_DIR
        })
        .join(file_name);
    let content = read_file_to_string(&shared).unwrap_or_else(|_error| fallback.to_owned());
    print_terminal_text(ensure_trailing_newline(&content).as_ref())
}

fn ensure_trailing_newline(content: &str) -> Cow<'_, str> {
    if content.ends_with('\n') {
        Cow::Borrowed(content)
    } else {
        Cow::Owned(format!("{content}\n"))
    }
}

#[expect(clippy::too_many_lines, reason = "CLI help topic table is intentionally flat")]
fn print_help_topic(topic: &str) -> Result<(), CliError> {
    match topic {
        "status" => print_help_lines(&["usage:", "  ctx [--root PATH] status"]),
        "abi" => print_help_lines(&["usage:", "  ctx [--root PATH] abi"]),
        "env" => print_help_lines(&["usage:", "  ctx [--root PATH] env"]),
        "root" => print_help_lines(&["usage:", "  ctx [--root PATH] root"]),
        "man" => print_help_lines(&[
            "usage:",
            "  ctx man",
            "  ctx man agent|tool|model|ctx|root|session|provider",
            "",
            "output:",
            "  prints the full built-in Markdown document to stdout",
            "  does not invoke less or any pager",
        ]),
        "bootstrap" => print_help_lines(&[
            "usage:",
            "  ctx bootstrap [SOURCE]",
            "",
            "alias:",
            "  ctx update [SOURCE]",
        ]),
        "update" => print_help_lines(&[
            "usage:",
            "  ctx update [SOURCE]",
            "",
            "alias:",
            "  ctx bootstrap [SOURCE]",
            "",
            "effect:",
            "  updates the reference source tree only",
            "  does not remount /ctx or start a watcher",
        ]),
        "mount" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] mount [--source SOURCE] [MOUNTPOINT]",
        ]),
        "ls" => print_help_lines(&["usage:", "  ctx [--root PATH] ls [PATH|model|agent|tool]"]),
        "which" => print_help_lines(&["usage:", "  ctx [--root PATH] which model|agent|tool NAME"]),
        "which-tool" => print_help_lines(&["usage:", "  ctx [--root PATH] which-tool NAME"]),
        "path" => print_help_lines(&["usage:", "  ctx [--root PATH] path shared NAME"]),
        "history" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] history AGENT [--session SESSION|SESSION]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
            "  positional SESSION is accepted for compatibility",
        ]),
        "resume" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] resume AGENT [--session SESSION|SESSION]",
            "",
            "output:",
            "  renders assistant events like ctx agent resume",
            "  use ctx agent resume --raw for raw socket JSONL",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
            "  positional SESSION is accepted for compatibility",
        ]),
        "send" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] send AGENT INPUT",
            "  ctx [--root PATH] send AGENT [--session SESSION] INPUT",
            "  ctx [--root PATH] send AGENT SESSION INPUT",
            "",
            "output:",
            "  renders assistant events like ctx agent send",
            "  use ctx agent send --raw for raw socket JSONL",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
            "  positional SESSION is accepted for compatibility",
        ]),
        "agent" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent new NAME [--temp] [--label LABEL] [--model MODEL] [--tool TOOL] [--shared NAME:read|write] [--mount SOURCE TARGET ro|rw]",
            "  ctx [--root PATH] agent start NAME [--session SESSION] [--cwd PATH] [--mount SOURCE TARGET ro|rw] [--no-default-workspace]",
            "  ctx [--root PATH] agent stop NAME",
            "  ctx [--root PATH] agent status NAME",
            "  ctx [--root PATH] agent ps",
            "  ctx [--root PATH] agent send NAME [--session SESSION] [--raw] INPUT",
            "  ctx [--root PATH] agent repl NAME [--session SESSION] [--raw]",
            "  ctx [--root PATH] agent resume NAME [--session SESSION] [--raw]",
            "  ctx [--root PATH] agent history NAME [--session SESSION]",
            "  ctx [--root PATH] agent output NAME [--session SESSION]",
            "  ctx [--root PATH] agent pack NAME [--session SESSION]",
            "  ctx [--root PATH] agent prompt NAME",
            "  ctx [--root PATH] agent tools NAME",
            "  ctx [--root PATH] agent children NAME [--session SESSION]",
            "  ctx [--root PATH] agent cancel NAME [--session SESSION] [--raw] [RUN]",
            "  ctx [--root PATH] agent watch NAME [--session SESSION]",
            "  ctx [--root PATH] agent attach NAME [--session SESSION]",
        ]),
        "agent new" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent new NAME [--temp] [--label LABEL] [--model MODEL] [--tool TOOL] [--shared NAME:read|write] [--mount SOURCE TARGET ro|rw]",
        ]),
        "agent start" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent start NAME [--session SESSION] [--cwd PATH] [--mount SOURCE TARGET ro|rw] [--no-default-workspace]",
            "",
            "default:",
            "  binds the caller current directory to /workspace rw",
            "  starts ctxterm -> tsh inside a bwrap sandbox at /workspace",
        ]),
        "agent stop" => print_help_lines(&["usage:", "  ctx [--root PATH] agent stop NAME"]),
        "agent status" => print_help_lines(&["usage:", "  ctx [--root PATH] agent status NAME"]),
        "agent ps" => print_help_lines(&["usage:", "  ctx [--root PATH] agent ps"]),
        "agent send" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent send NAME [--session SESSION] [--raw] INPUT",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent repl" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent repl NAME [--session SESSION] [--raw]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent resume" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent resume NAME [--session SESSION] [--raw]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent history" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent history NAME [--session SESSION]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent output" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent output NAME [--session SESSION]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent pack" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent pack NAME [--session SESSION]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent prompt" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent prompt NAME",
            "",
            "prints the rendered runtime system prompt for the agent",
        ]),
        "agent tools" => print_help_lines(&["usage:", "  ctx [--root PATH] agent tools NAME"]),
        "agent children" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent children NAME [--session SESSION]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent cancel" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent cancel NAME [--session SESSION] [--raw] [RUN]",
            "",
            "session:",
            "  omitting --session uses session/index/current, then default",
        ]),
        "agent watch" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent watch NAME [--session SESSION]",
        ]),
        "agent attach" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] agent attach NAME [--session SESSION]",
        ]),
        "provider" => print_help_lines(&[
            "usage:",
            "  ctx provider oauth login PROVIDER [--timeout SECONDS]",
            "  ctx provider oauth status PROVIDER",
            "  ctx provider oauth refresh PROVIDER",
            "  ctx provider secret set PROVIDER [--slot SLOT]",
            "  ctx provider secret status PROVIDER [--slot SLOT]",
            "  ctx provider preset list",
            "  ctx provider preset show openai|codex|anthropic|google",
            "  ctx provider preset install openai|codex|anthropic|google",
            "",
            "notes:",
            "  reads /etc/cortexfs/providers.d/*.json",
            "  stores provider API keys in the root-owned CortexFS system secret store",
            "  stores OAuth tokens in the system keychain, not /ctx/model",
            "  provider presets install ordinary JSON files under /etc/cortexfs/providers.d",
        ]),
        "provider oauth" => print_help_lines(&[
            "usage:",
            "  ctx provider oauth login PROVIDER [--timeout SECONDS]",
            "  ctx provider oauth status PROVIDER",
            "  ctx provider oauth refresh PROVIDER",
        ]),
        "provider oauth login" => print_help_lines(&[
            "usage:",
            "  ctx provider oauth login PROVIDER [--timeout SECONDS]",
        ]),
        "provider oauth status" => {
            print_help_lines(&["usage:", "  ctx provider oauth status PROVIDER"])
        },
        "provider oauth refresh" => {
            print_help_lines(&["usage:", "  ctx provider oauth refresh PROVIDER"])
        },
        "provider secret" => print_help_lines(&[
            "usage:",
            "  ctx provider secret set PROVIDER [--slot SLOT]",
            "  ctx provider secret status PROVIDER [--slot SLOT]",
            "",
            "secret:",
            "  set reads the secret value from stdin",
            "  stores it in the root-owned CortexFS system secret store",
            "  model runtimes read the store directly; API keys are not placed in environment variables",
        ]),
        "provider preset" => print_help_lines(&[
            "usage:",
            "  ctx provider preset list",
            "  ctx provider preset show openai|codex|anthropic|google",
            "  ctx provider preset install openai|codex|anthropic|google",
            "",
            "aliases:",
            "  codex -> openai",
            "  gemini -> google",
            "  claude -> anthropic",
        ]),
        "ping" => print_help_lines(&["usage:", "  ctx [--root PATH] ping model/NAME|agent/NAME"]),
        "cancel" => print_help_lines(&["usage:", "  ctx [--root PATH] cancel model/NAME|agent/NAME RUN"]),
        "exec" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] exec model/NAME|agent/NAME|tool/NAME [ARG...]",
        ]),
        "tool" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] tool NAME [ARG...]",
            "",
            "lookup:",
            "  validates NAME through CTX_PATH",
            "  runs allowlisted safe CortexFS core CLIs such as tsh.config",
            "  refuses ordinary visible tools that would bypass CortexFS authorization",
            "",
            "examples:",
            "  ctx tool tsh.config",
            "  ctx tool tsh.config '{\"max_loaded_tools\":32}'",
        ]),
        "cat" => print_help_lines(&["usage:", "  ctx [--root PATH] cat PATH"]),
        "set" => print_help_lines(&["usage:", "  ctx [--root PATH] set PATH VALUE"]),
        "append" => print_help_lines(&["usage:", "  ctx [--root PATH] append PATH VALUE"]),
        "file" => print_help_lines(&[
            "usage:",
            "  ctx [--root PATH] file PATH",
            "  ctx [--root PATH] file info PATH",
            "  ctx [--root PATH] file type PATH",
            "  ctx [--root PATH] file check PATH",
            "",
            "output:",
            "  file PATH prints CortexFS type, stat, token estimate, and user.cortexfs.* xattrs",
        ]),
        "doctor" => print_help_lines(&["usage:", "  ctx [--root PATH] doctor"]),
        "validate-name" => print_help_lines(&["usage:", "  ctx validate-name NAME"]),
        _ => Err(CliError::usage(format!("unknown help topic: {topic}"))),
    }
}

fn print_abi() -> Result<(), CliError> {
    print_line("root=/ctx")?;
    print_line("entries=status bin model agent tool home shared")?;
    print_line("exec=model agent tool")?;
    print_line("socket=name.sock")?;
    print_line("control=name.d")?;
    print_line("policy=allow <subject_type> <object_class>:<object_name> <permission>")
}

fn print_env(root: &Path) -> Result<(), CliError> {
    for line in env_exports(
        root,
        env::var("CTX_HOME").ok().as_deref(),
        env::var("CTX_PATH").ok().as_deref(),
    ) {
        print_line(&line)?;
    }
    Ok(())
}

fn env_exports(root: &Path, home_env: Option<&str>, path_env: Option<&str>) -> [String; 4] {
    let root = root.display().to_string();
    let home = home_env.map_or_else(|| format!("{root}/home/$(id -u)"), str::to_owned);
    let path = path_env.map_or_else(|| format!("{root}/tool:{home}/tool"), str::to_owned);
    let root_bin = format!("{root}/bin");
    [
        format!("export CTX_ROOT={}", shell_quote(&root)),
        format!("export CTX_HOME={}", shell_quote(&home)),
        format!("export CTX_PATH={}", shell_quote(&path)),
        format!("export PATH={}:$PATH", shell_quote(&root_bin)),
    ]
}

fn print_status(root: &Path) -> Result<(), CliError> {
    let (exists, is_dir) = ctx_root_shape(root);
    let mounted = is_mount_point(root).unwrap_or(false);
    let status = read_ctx_status(root);
    let present_entries = ROOT_ENTRIES
        .iter()
        .filter(|entry| ctx_root_entry_present(root, entry))
        .count();
    let missing_entries = ROOT_ENTRIES.len().saturating_sub(present_entries);
    let agents = read_status_agent_processes(root)?;

    let color = color_enabled();
    print_line(&styled(color, ANSI_BOLD_CYAN, "ctx"))?;
    print_status_field(
        color,
        "    State:",
        &status_state_value(color, ctx_state(exists, is_dir, mounted)),
    )?;
    print_status_field(color, "     Root:", &styled(color, ANSI_CYAN, &root.display().to_string()))?;
    print_status_field(color, "   Status:", &status_state_value(color, &status))?;
    print_status_field(
        color,
        "  Mounted:",
        &status_bool_value(color, if mounted { "yes" } else { "no" }, mounted),
    )?;
    print_status_field(
        color,
        "  Entries:",
        &styled(
            color,
            if missing_entries == 0 {
                ANSI_GREEN
            } else {
                ANSI_YELLOW
            },
            &format!("{present_entries}/{} loaded", ROOT_ENTRIES.len()),
        ),
    )?;
    print_status_field(
        color,
        "   Failed:",
        &styled(
            color,
            if missing_entries == 0 {
                ANSI_GREEN
            } else {
                ANSI_RED
            },
            &format!("{missing_entries} entries"),
        ),
    )?;
    print_status_field(
        color,
        "   Agents:",
        &styled(color, ANSI_BLUE, &format!("{} loaded", agents.len())),
    )?;

    if !agents.is_empty() {
        print_line(&styled(color, ANSI_BOLD_BLUE, "    Tree:"))?;
        for line in render_agent_status_lines(&agents) {
            print_line(&format!("          {}", status_tree_line(color, &line)))?;
        }
    }

    Ok(())
}

fn ctx_root_shape(root: &Path) -> (bool, bool) {
    match fs::symlink_metadata(root) {
        Ok(metadata) => (true, metadata.is_dir()),
        Err(_error) => (false, false),
    }
}

fn ctx_root_entry_present(root: &Path, entry: &str) -> bool {
    fs::symlink_metadata(root.join(entry))
        .is_ok_and(|metadata| !metadata.file_type().is_symlink())
}

fn ctx_state(exists: bool, is_dir: bool, mounted: bool) -> &'static str {
    if mounted {
        "running"
    } else if exists && is_dir {
        "available"
    } else if exists {
        "invalid"
    } else {
        "missing"
    }
}

fn read_ctx_status(root: &Path) -> String {
    read_file_to_string(&root.join("status"))
        .ok()
        .map(|content| content.trim().to_owned())
        .filter(|content| !content.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn read_status_agent_processes(root: &Path) -> Result<Vec<AgentProcess>, CliError> {
    match read_agent_processes(root) {
        Ok(processes) => Ok(processes),
        Err(error)
            if error.message.starts_with(&format!(
                "cannot read {}",
                root.join("agent").display()
            )) =>
        {
            Ok(Vec::new())
        }
        Err(error) => Err(error),
    }
}

fn render_agent_status_lines(processes: &[AgentProcess]) -> Vec<String> {
    let mut processes = processes.to_vec();
    processes.sort_by(|left, right| left.name.cmp(&right.name));
    let names = processes
        .iter()
        .map(|process| process.name.clone())
        .collect::<Vec<_>>();
    let mut rendered = Vec::new();
    for process in &processes {
        if process
            .parent
            .as_ref()
            .is_none_or(|parent| !names.contains(parent))
        {
            render_agent_process_tree(process, &processes, "", true, true, &mut rendered);
        }
    }
    rendered
}

fn bootstrap_reference_tree(source: Option<&Path>) -> Result<(), CliError> {
    let source = match source {
        Some(path) => path.to_path_buf(),
        None => default_source_root()?,
    };
    ensure_v1_reference_tree(&source).map_err(|error| {
        CliError::unavailable(format!(
            "cannot bootstrap {}: {} ({error:?})",
            source.display(),
            error.errno(),
        ))
    })?;
    print_line(&format!("source={}", source.display()))
}

fn mount_reference_tree(
    root: &Path,
    source: Option<&Path>,
    mountpoint: Option<&Path>,
) -> Result<(), CliError> {
    let source = match source {
        Some(path) => path.to_path_buf(),
        None => default_source_root()?,
    };
    let mountpoint = mountpoint.unwrap_or(root);

    ensure_v1_reference_tree(&source).map_err(|error| {
        CliError::unavailable(format!(
            "cannot bootstrap {}: {} ({error:?})",
            source.display(),
            error.errno(),
        ))
    })?;
    create_plain_mountpoint_dir(mountpoint)?;
    ensure_plain_mountpoint_dir(mountpoint)?;
    let mountpoint = absolute_existing_path(mountpoint).map_err(|error| {
        CliError::unavailable(format!(
            "cannot resolve mountpoint {}: {error}",
            mountpoint.display()
        ))
    })?;
    if is_mount_point(&mountpoint).unwrap_or(false) {
        return Err(CliError::unavailable(format!(
            "already mounted: {}",
            mountpoint.display()
        )));
    }

    let mount_bin = cortexfs_mount_bin();
    spawn_mount_process(&mount_bin, &source, &mountpoint)?;

    for _attempt in 0..20 {
        if is_mount_point(&mountpoint).unwrap_or(false) {
            print_line(&format!("mounted={}", mountpoint.display()))?;
            print_line(&format!("source={}", source.display()))?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    Err(CliError::unavailable(format!(
        "mount did not become ready: {}",
        mountpoint.display()
    )))
}

fn create_plain_mountpoint_dir(mountpoint: &Path) -> Result<(), CliError> {
    if let Ok(metadata) = fs::symlink_metadata(mountpoint) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            Ok(())
        } else {
            Err(CliError::unavailable(format!(
                "mountpoint is not a plain directory: {}",
                mountpoint.display()
            )))
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(mountpoint);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(CliError::unavailable(format!(
                    "mountpoint path contains a non-directory entry: {}",
                    current.display()
                )));
            }
            Ok(_metadata) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot inspect mountpoint {}: {error}",
                    current.display()
                )));
            }
        }
    }

    let mut parent_dir = if let Some(existing_parent) = missing.last().and_then(|path| path.parent())
    {
        open_plain_file_parent_dir(existing_parent)?
    } else {
        return Ok(());
    };
    for directory in missing.iter().rev() {
        let name = directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                CliError::unavailable(format!(
                    "invalid mountpoint directory name: {}",
                    directory.display()
                ))
            })?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o755),
        )
        .map_err(|error| {
            CliError::unavailable(format!(
                "cannot create mountpoint {}: {error}",
                directory.display()
            ))
        })?;
        parent_dir.sync_all().map_err(|error| {
            CliError::unavailable(format!(
                "cannot sync mountpoint parent {}: {error}",
                directory.display()
            ))
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
            CliError::unavailable(format!(
                "cannot open mountpoint {}: {error}",
                directory.display()
            ))
        })?;
        parent_dir = fs::File::from(child);
        parent_dir.sync_all().map_err(|error| {
            CliError::unavailable(format!(
                "cannot sync mountpoint {}: {error}",
                directory.display()
            ))
        })?;
    }
    Ok(())
}

fn ensure_plain_mountpoint_dir(mountpoint: &Path) -> Result<(), CliError> {
    let directory = open_plain_file_parent_dir(mountpoint)?;
    let metadata = directory.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot stat mountpoint {}: {error}", mountpoint.display()))
    })?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(CliError::unavailable(format!(
            "mountpoint is not a plain directory: {}",
            mountpoint.display()
        )))
    }
}

fn default_source_root() -> Result<PathBuf, CliError> {
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(data_home).join("cortexfs").join("v1-root"));
    }
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("cortexfs")
            .join("v1-root"));
    }
    Err(CliError::unavailable(
        "cannot choose default source root without HOME or XDG_DATA_HOME",
    ))
}

fn cortexfs_mount_bin() -> PathBuf {
    if let Ok(current) = env::current_exe()
        && let Some(sibling) = plain_sibling_mount_bin(&current)
    {
        return sibling;
    }
    PathBuf::from(CORTEXFS_MOUNT_PROGRAM)
}

fn plain_sibling_mount_bin(current_exe: &Path) -> Option<PathBuf> {
    let sibling = current_exe.parent()?.join("cortexfs-mount");
    let metadata = sibling.symlink_metadata().ok()?;
    (metadata.is_file()
        && !metadata.file_type().is_symlink()
        && metadata.permissions().mode() & 0o111 != 0)
        .then_some(sibling)
}

const CORTEXFS_MOUNT_PROGRAM: &str = "/usr/bin/cortexfs-mount";
const TRUSTED_SETSID_BIN: &str = "/usr/bin/setsid";

fn spawn_mount_process(mount_bin: &Path, source: &Path, mountpoint: &Path) -> Result<(), CliError> {
    match spawn_null(detached_mount_command(mount_bin, source, mountpoint)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            spawn_null(direct_mount_command(mount_bin, source, mountpoint)).map_err(|error| {
                CliError::unavailable(format!("cannot start {}: {error}", mount_bin.display()))
            })
        }
        Err(error) => Err(CliError::unavailable(format!(
            "cannot start {}: {error}",
            mount_bin.display()
        ))),
    }
}

fn detached_mount_command(mount_bin: &Path, source: &Path, mountpoint: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(TRUSTED_SETSID_BIN);
    command
        .arg("-f")
        .arg(mount_bin)
        .arg("--source")
        .arg(source)
        .arg(mountpoint)
        .env_clear()
        .env("PATH", "/usr/bin:/bin");
    command
}

fn direct_mount_command(mount_bin: &Path, source: &Path, mountpoint: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(mount_bin);
    command
        .arg("--source")
        .arg(source)
        .arg(mountpoint)
        .env_clear()
        .env("PATH", "/usr/bin:/bin");
    command
}

fn spawn_null(mut command: ProcessCommand) -> io::Result<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_child| ())
}
