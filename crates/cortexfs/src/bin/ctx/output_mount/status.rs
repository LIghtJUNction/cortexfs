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
    render_agent_process_forest(processes)
}
