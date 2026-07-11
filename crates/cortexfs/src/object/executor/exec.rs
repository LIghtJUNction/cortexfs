use super::*;

pub(crate) fn execute_agent_tool_call(
    config: &AgentModelRunConfig,
    tool_call: &AgentToolCall,
) -> Result<String, String> {
    let view = derive_agent_runtime_view(&config.ctx_root, &config.agent)
        .map_err(|error| format!("cannot derive agent authority: {}", error.errno()))?;
    if tool_call.name == "tsh" {
        validate_agent_tsh_args(&tool_call.args)?;
    } else if !agent_tool_is_loaded(&view, &tool_call.name)? {
        return Err(format!(
            "unsupported native tool {}; load it through tsh first",
            tool_call.name
        ));
    }
    let Some(hit) = view
        .tool_path()
        .find(&tool_call.name)
        .map_err(|error| format!("cannot inspect CTX_PATH: {error:?}"))?
    else {
        return Err(format!("tool not found: {}", tool_call.name));
    };
    let policy_path = hit.control_dir().join("policy");
    let policy_text = read_small_plain_text_file(&policy_path, MAX_RUNNER_CONTROL_BYTES, "runner")
        .map_err(|error| format!("cannot read {}: {error}", policy_path.display()))?;
    let tool_policy = PolicyV0::parse(&policy_text)
        .map_err(|_error| format!("invalid policy for tool:{}", tool_call.name))?;
    let grant = authorize_tool_execution(
        view.tool_path(),
        &tool_call.name,
        ToolExecutionAuthority::new(
            view.identity(),
            view.mount_table(),
            view.policy_subject(),
            view.policy(),
            &tool_policy,
        ),
    )
    .map_err(|denial| tool_denial_message(&tool_call.name, denial))?;
    let tool_executable = open_executable_no_follow(grant.hit().path())
        .map_err(|error| format!("cannot run tool:{}: {error}", tool_call.name))?;
    let sandbox = prepare_agent_tool_sandbox(&view)?;
    let mut command = Command::new(BWRAP_PROGRAM);
    command.args(agent_tool_bwrap_args(&AgentToolBwrapArgs {
        config,
        tool_executable: &proc_fd_path(&tool_executable),
        tool_args: &tool_call.args,
        env: view.env(),
        mount_table: view.mount_table(),
        cwd: view.cwd(),
        sandbox: sandbox.as_ref(),
    }));
    let output = run_agent_tool_process(&mut command)
        .map_err(|error| format!("cannot run tool:{}: {error}", tool_call.name))?;
    let mut result = String::new();
    result.push_str(&String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(stderr.trim_end());
        result.push('\n');
    }
    if !output.status.success() {
        if result.trim().is_empty() {
            result.push_str("tool exited with ");
            result.push_str(&output.status.to_string());
            result.push('\n');
        }
        return Err(trim_tool_result(&result));
    }
    Ok(trim_tool_result(&result))
}

pub(crate) fn agent_tool_is_loaded(
    view: &cortexfs::AgentRuntimeView,
    name: &str,
) -> Result<bool, String> {
    let state_path = cortexfs::tsh_context_state_path(view.home());
    let state = cortexfs::read_tsh_context_state(&state_path)
        .map_err(|error| format!("cannot read {}: {error}", state_path.display()))?;
    Ok(state.tools.iter().any(|tool| tool.name == name))
}

#[derive(Debug)]
pub(crate) struct AgentToolSandbox {
    pub(crate) workspace: PathBuf,
    pub(crate) upper: PathBuf,
    pub(crate) work: PathBuf,
}

pub(crate) struct AgentToolBwrapArgs<'a> {
    pub(crate) config: &'a AgentModelRunConfig,
    pub(crate) tool_executable: &'a Path,
    pub(crate) tool_args: &'a [OsString],
    pub(crate) env: &'a [(String, String)],
    pub(crate) mount_table: &'a cortexfs::MountTable,
    pub(crate) cwd: &'a Path,
    pub(crate) sandbox: Option<&'a AgentToolSandbox>,
}

pub(crate) fn prepare_agent_tool_sandbox(
    view: &cortexfs::AgentRuntimeView,
) -> Result<Option<AgentToolSandbox>, String> {
    let Some(workspace) = env::var_os("CTX_WORKSPACE").map(PathBuf::from) else {
        return Ok(None);
    };
    let workspace = visible_workspace_source(workspace);
    if !is_absolute_host_workspace_path(&workspace) {
        return Ok(None);
    }
    let hash = workspace_overlay_hash(&workspace);
    let session = env::var("CTX_SESSION")
        .ok()
        .filter(|value| is_stable_overlay_component(value))
        .unwrap_or_else(|| "default".to_owned());
    let root = view
        .home()
        .join("session")
        .join(session)
        .join("workspace-overlay")
        .join(hash);
    let upper = root.join("upper");
    let work = root.join("work");
    fs::create_dir_all(&upper)
        .map_err(|error| format!("cannot create overlay upper {}: {error}", upper.display()))?;
    fs::create_dir_all(&work)
        .map_err(|error| format!("cannot create overlay work {}: {error}", work.display()))?;
    Ok(Some(AgentToolSandbox {
        workspace,
        upper,
        work,
    }))
}

pub(crate) fn visible_workspace_source(workspace: PathBuf) -> PathBuf {
    if workspace.exists() {
        return workspace;
    }
    let mounted_workspace = PathBuf::from("/workspace");
    if mounted_workspace.exists() {
        return mounted_workspace;
    }
    workspace
}

pub(crate) fn agent_tool_bwrap_args(request: &AgentToolBwrapArgs<'_>) -> Vec<OsString> {
    let cwd = request.cwd.to_str().unwrap_or("/workspace");
    let mut args = vec![
        OsString::from("--clearenv"),
        OsString::from("--die-with-parent"),
        OsString::from("--unshare-pid"),
        OsString::from("--unshare-net"),
        OsString::from("--proc"),
        OsString::from("/proc"),
        OsString::from("--dev"),
        OsString::from("/dev"),
        OsString::from("--tmpfs"),
        OsString::from("/tmp"),
        OsString::from("--dir"),
        OsString::from("/run"),
        OsString::from("--dir"),
        OsString::from("/home"),
        OsString::from("--ro-bind"),
        OsString::from("/usr"),
        OsString::from("/usr"),
        OsString::from("--ro-bind"),
        OsString::from("/etc"),
        OsString::from("/etc"),
        OsString::from("--tmpfs"),
        OsString::from("/etc/profile.d"),
        OsString::from("--symlink"),
        OsString::from("usr/bin"),
        OsString::from("/bin"),
        OsString::from("--symlink"),
        OsString::from("usr/lib"),
        OsString::from("/lib"),
        OsString::from("--symlink"),
        OsString::from("usr/lib"),
        OsString::from("/lib64"),
    ];
    args.extend(optional_dev_toolchain_bind_args());
    for env in request.env {
        if is_secret_env_name(&env.0) || env.0 == "CTX_PROVIDER_CONFIG_DIR" {
            continue;
        }
        args.extend([
            OsString::from("--setenv"),
            OsString::from(&env.0),
            OsString::from(&env.1),
        ]);
    }
    args.extend([
        OsString::from("--setenv"),
        OsString::from("CTX_AGENT"),
        OsString::from(&request.config.agent),
        OsString::from("--setenv"),
        OsString::from("CTX_ROOT"),
        request.config.ctx_root.as_os_str().to_owned(),
        OsString::from("--setenv"),
        OsString::from("CTX_SOURCE"),
        request.config.source.as_os_str().to_owned(),
        OsString::from("--setenv"),
        OsString::from("CTX_TOOL_MODE"),
        OsString::from("cli"),
        OsString::from("--setenv"),
        OsString::from("PATH"),
        OsString::from("/usr/bin:/bin"),
    ]);
    if let Some(rustup_home) = rustup_home() {
        args.extend([
            OsString::from("--setenv"),
            OsString::from("RUSTUP_HOME"),
            rustup_home.as_os_str().to_owned(),
        ]);
    }
    if let Some(cargo_home) = cargo_home() {
        args.extend([
            OsString::from("--setenv"),
            OsString::from("CARGO_HOME"),
            cargo_home.as_os_str().to_owned(),
        ]);
    }
    if let Some(toolchain) = env::var_os("RUSTUP_TOOLCHAIN") {
        args.extend([
            OsString::from("--setenv"),
            OsString::from("RUSTUP_TOOLCHAIN"),
            toolchain,
        ]);
    }
    args.extend(bwrap_source_root_bind_args(&request.config.source));
    for mount in request.mount_table.entries() {
        if request.sandbox.is_some() && path_uses_workspace(mount.target()) {
            continue;
        }
        let Some(source) =
            visible_mount_source(&request.config.source, mount.source(), mount.target())
        else {
            continue;
        };
        args.push(match mount.mode() {
            cortexfs::MountMode::ReadOnly => OsString::from("--ro-bind"),
            cortexfs::MountMode::ReadWrite => OsString::from("--bind"),
        });
        args.push(OsString::from(source));
        args.push(OsString::from(mount.target()));
    }
    if let Some(sandbox) = request.sandbox {
        args.extend(overlay_workspace_bwrap_args(sandbox));
    }
    args.extend(bwrap_dir_args_for_chdir(cwd));
    args.extend([
        OsString::from("--chdir"),
        OsString::from(cwd),
        OsString::from("--"),
        request.tool_executable.as_os_str().to_owned(),
    ]);
    args.extend(request.tool_args.iter().cloned());
    args
}

pub(crate) fn overlay_workspace_bwrap_args(sandbox: &AgentToolSandbox) -> Vec<OsString> {
    let mut args = bwrap_dir_args_for_chdir("/workspace");
    args.extend([
        OsString::from("--bind"),
        sandbox.upper.as_os_str().to_owned(),
        sandbox.upper.as_os_str().to_owned(),
        OsString::from("--bind"),
        sandbox.work.as_os_str().to_owned(),
        sandbox.work.as_os_str().to_owned(),
        OsString::from("--overlay-src"),
        sandbox.workspace.as_os_str().to_owned(),
        OsString::from("--overlay"),
        sandbox.upper.as_os_str().to_owned(),
        sandbox.work.as_os_str().to_owned(),
        OsString::from("/workspace"),
    ]);
    args
}

pub(crate) fn is_secret_env_name(name: &str) -> bool {
    name.starts_with("CTX_PROVIDER_SECRET_")
}

pub(crate) fn optional_dev_toolchain_bind_args() -> Vec<OsString> {
    let mut args = Vec::new();
    for path in [rustup_home(), cargo_home()].into_iter().flatten() {
        if path.is_dir() {
            args.extend(bwrap_dir_args_for_parent(&path.display().to_string()));
            args.push(OsString::from("--ro-bind"));
            args.push(path.as_os_str().to_owned());
            args.push(path.as_os_str().to_owned());
        }
    }
    args
}

pub(crate) fn rustup_home() -> Option<PathBuf> {
    env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".rustup")))
}

pub(crate) fn cargo_home() -> Option<PathBuf> {
    env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
}

pub(crate) fn path_uses_workspace(path: &str) -> bool {
    path == "/workspace" || path.starts_with("/workspace/")
}

pub(crate) fn is_absolute_host_workspace_path(path: &Path) -> bool {
    path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
}

pub(crate) fn is_stable_overlay_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub(crate) fn workspace_overlay_hash(path: &Path) -> String {
    let mut digest = Sha256::new();
    digest.update(path.as_os_str().as_encoded_bytes());
    let hash = digest.finalize();
    let mut output = String::with_capacity(32);
    for byte in hash.iter().take(16) {
        let _ignored = write!(&mut output, "{byte:02x}");
    }
    output
}

pub(crate) fn socket_runtime_host_mount_source(source_root: &Path, source: &str) -> String {
    let source_path = Path::new(source);
    if source_path == Path::new(DEFAULT_CTX_ROOT) {
        if !source_root.exists() && Path::new(DEFAULT_CTX_ROOT).exists() {
            return DEFAULT_CTX_ROOT.to_owned();
        }
        return source_root.display().to_string();
    }
    if let Ok(relative) = source_path.strip_prefix(DEFAULT_CTX_ROOT) {
        if !source_root.exists() && Path::new(DEFAULT_CTX_ROOT).exists() {
            return Path::new(DEFAULT_CTX_ROOT)
                .join(relative)
                .display()
                .to_string();
        }
        return source_root.join(relative).display().to_string();
    }
    source.to_owned()
}

pub(crate) fn visible_mount_source(
    source_root: &Path,
    source: &str,
    target: &str,
) -> Option<String> {
    let source = socket_runtime_host_mount_source(source_root, source);
    if Path::new(&source).exists() {
        return Some(source);
    }
    if target == DEFAULT_CTX_ROOT && Path::new(DEFAULT_CTX_ROOT).exists() {
        return Some(DEFAULT_CTX_ROOT.to_owned());
    }
    None
}

pub(crate) fn bwrap_source_root_bind_args(source_root: &Path) -> Vec<OsString> {
    let Some(source_root) = source_root.to_str() else {
        return Vec::new();
    };
    if !source_root.starts_with('/') || source_root == "/" || !Path::new(source_root).exists() {
        return Vec::new();
    }
    let mut args = bwrap_dir_args_for_parent(source_root);
    args.push(OsString::from("--ro-bind"));
    args.push(OsString::from(source_root));
    args.push(OsString::from(source_root));
    args
}

pub(crate) fn bwrap_dir_args_for_parent(path: &str) -> Vec<OsString> {
    let Some((parent, _name)) = path.rsplit_once('/') else {
        return Vec::new();
    };
    if parent.is_empty() {
        Vec::new()
    } else {
        bwrap_dir_args_for_chdir(parent)
    }
}

pub(crate) fn bwrap_dir_args_for_chdir(cwd: &str) -> Vec<OsString> {
    let mut args = Vec::new();
    if !cwd.starts_with('/') {
        return args;
    }
    let mut path = String::new();
    for component in cwd.split('/').filter(|component| !component.is_empty()) {
        path.push('/');
        path.push_str(component);
        args.push(OsString::from("--dir"));
        args.push(OsString::from(path.clone()));
    }
    args
}

pub(crate) fn run_agent_tool_process(
    command: &mut Command,
) -> Result<std::process::Output, String> {
    run_agent_tool_process_with_timeout(command, Duration::from_secs(agent_tool_timeout_seconds()))
}

pub(crate) fn run_agent_tool_process_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = spawn_with_etxtbsy_retry(command).map_err(|error| error.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cannot read tool stdout".to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "cannot read tool stderr".to_owned())?;
    let stdout_reader =
        thread::spawn(move || read_limited_bytes(stdout, MAX_AGENT_TOOL_OUTPUT_BYTES + 1));
    let stderr_reader =
        thread::spawn(move || read_limited_bytes(stderr, MAX_AGENT_TOOL_OUTPUT_BYTES + 1));
    let mut stdout_reader = Some(stdout_reader);
    let mut stderr_reader = Some(stderr_reader);
    let mut stdout = None;
    let mut stderr = None;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if stdout.is_none()
            && stdout_reader
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
        {
            let output = stdout_reader
                .take()
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default();
            if output.len() > MAX_AGENT_TOOL_OUTPUT_BYTES {
                terminate_process_group(&mut child);
                let _ignored = child.wait();
                if let Some(reader) = stderr_reader.take() {
                    let _ignored = reader.join();
                }
                return Err(format!(
                    "tool output exceeds {MAX_AGENT_TOOL_OUTPUT_BYTES} bytes"
                ));
            }
            stdout = Some(output);
        }
        if stderr.is_none()
            && stderr_reader
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
        {
            let output = stderr_reader
                .take()
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default();
            if output.len() > MAX_AGENT_TOOL_OUTPUT_BYTES {
                terminate_process_group(&mut child);
                let _ignored = child.wait();
                if let Some(reader) = stdout_reader.take() {
                    let _ignored = reader.join();
                }
                return Err(format!(
                    "tool output exceeds {MAX_AGENT_TOOL_OUTPUT_BYTES} bytes"
                ));
            }
            stderr = Some(output);
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            terminate_process_group(&mut child);
            let _ignored = child.wait();
            return Err(format!("tool timed out after {}s", timeout.as_secs()));
        }
        thread::sleep(Duration::from_millis(50));
    };
    terminate_process_group(&mut child);
    let stdout = match stdout {
        Some(output) => output,
        None => {
            collect_agent_tool_output_reader(stdout_reader.take(), AGENT_TOOL_OUTPUT_DRAIN_TIMEOUT)?
        }
    };
    let stderr = match stderr {
        Some(output) => output,
        None => {
            collect_agent_tool_output_reader(stderr_reader.take(), AGENT_TOOL_OUTPUT_DRAIN_TIMEOUT)?
        }
    };
    if stdout.len() > MAX_AGENT_TOOL_OUTPUT_BYTES || stderr.len() > MAX_AGENT_TOOL_OUTPUT_BYTES {
        return Err(format!(
            "tool output exceeds {MAX_AGENT_TOOL_OUTPUT_BYTES} bytes"
        ));
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

pub(crate) fn collect_agent_tool_output_reader(
    reader: Option<thread::JoinHandle<Vec<u8>>>,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let Some(reader) = reader else {
        return Ok(Vec::new());
    };
    let deadline = Instant::now() + timeout;
    while !reader.is_finished() {
        if Instant::now() >= deadline {
            return Err(format!(
                "tool output did not close within {}s",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(reader.join().unwrap_or_default())
}
