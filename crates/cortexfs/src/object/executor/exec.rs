use super::*;
use std::ffi::OsStr;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::FileTypeExt;

pub(crate) fn execute_agent_tool_call(
    config: &AgentModelRunConfig,
    tool_call: &AgentToolCall,
) -> Result<String, String> {
    execute_agent_tool_call_with(&AgentToolExecutionConfig::from_model(config), tool_call)
}

pub(crate) struct AgentToolExecutionConfig<'a> {
    pub(crate) agent: &'a str,
    pub(crate) source: &'a Path,
    pub(crate) ctx_root: &'a Path,
    pub(crate) run: &'a str,
    pub(crate) session: &'a str,
    pub(crate) inherit_control: bool,
    pub(crate) cancel: Option<(&'a Path, &'a str)>,
}

impl<'a> AgentToolExecutionConfig<'a> {
    pub(crate) fn from_model(config: &'a AgentModelRunConfig) -> Self {
        Self {
            agent: &config.agent,
            source: &config.source,
            ctx_root: &config.ctx_root,
            run: &config.run,
            session: &config.session,
            inherit_control: true,
            cancel: None,
        }
    }
}

pub(crate) fn execute_agent_tool_call_with(
    config: &AgentToolExecutionConfig<'_>,
    tool_call: &AgentToolCall,
) -> Result<String, String> {
    prepare_agent_tool_call(config, tool_call)?.execute(config)
}

pub(crate) struct PreparedAgentToolCall {
    command: Command,
    home_dir: fs::File,
    home_alias_dir: fs::File,
    tool_executable: fs::File,
    name: String,
    approval: cortexfs::AgentApprovalMode,
    working_set: Option<(PathBuf, cortexfs::TshLoadedToolState, usize)>,
}

pub(crate) fn prepare_agent_tool_call(
    config: &AgentToolExecutionConfig<'_>,
    tool_call: &AgentToolCall,
) -> Result<PreparedAgentToolCall, String> {
    let view = derive_agent_runtime_view(config.ctx_root, config.agent)
        .map_err(|error| format!("cannot derive agent authority: {}", error.errno()))?;
    let owner = view.owner().to_string();
    let home_source = config
        .source
        .join("home")
        .join(&owner)
        .join("agent")
        .join(view.agent_name());
    let context_path =
        cortexfs::tsh_context_state_path(&home_source.join("session").join(config.session));
    let network_allowed = view.policy().allows(
        view.policy_subject(),
        PolicyObjectClass::Network,
        "default",
        PolicyPermission::Connect,
    );
    if tool_call.name == "tsh" {
        validate_agent_tsh_args(&tool_call.args)?;
    } else if !view.declared_tools().contains(&tool_call.name)
        && !cortexfs::tsh_context_contains(&context_path, &tool_call.name)
            .map_err(|error| format!("cannot read session tool context: {error}"))?
    {
        return Err(format!(
            "unsupported native tool {}; declare it in the agent tools control",
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
    let git_write_declared = view.mount_table().entries().iter().any(|mount| {
        mount.target() == "/workspace/.git" && mount.mode() == cortexfs::MountMode::ReadWrite
    });
    let sandbox = if git_write_declared {
        None
    } else {
        prepare_agent_tool_sandbox(&view, config.source)?
    };
    let ctx_home_target = Path::new(DEFAULT_CTX_ROOT).join("home").join(&owner);
    let home_target = ctx_home_target.join("agent").join(view.agent_name());
    let home_dir = open_plain_directory(&home_source)
        .map_err(|error| format!("cannot open agent home {}: {error}", home_source.display()))?;
    let home_alias_dir = home_dir
        .try_clone()
        .map_err(|error| format!("cannot duplicate agent home fd: {error}"))?;
    crate::provider::name::files::clear_fd_cloexec(&home_dir)
        .map_err(|error| format!("cannot preserve agent home fd: {error:?}"))?;
    crate::provider::name::files::clear_fd_cloexec(&home_alias_dir)
        .map_err(|error| format!("cannot preserve agent home alias fd: {error:?}"))?;
    let mut command = Command::new(BWRAP_PROGRAM);
    let control = if config.inherit_control {
        nested_control_environment(
            env::var_os("CTX_CONTROL_SOCKET"),
            env::var_os("CTX_CONTROL_TOKEN"),
        )?
    } else {
        None
    };
    command.args(agent_tool_bwrap_args(&AgentToolBwrapArgs {
        config,
        tool_executable: &proc_fd_path(&tool_executable),
        tool_args: &tool_call.args,
        env: view.env(),
        mount_table: view.mount_table(),
        cwd: view.cwd(),
        sandbox: sandbox.as_ref(),
        network_allowed,
        home_fd: home_dir.as_raw_fd(),
        home_alias_fd: home_alias_dir.as_raw_fd(),
        home_target: &home_target,
        ctx_home_target: &ctx_home_target,
        control: control.as_ref(),
    }));
    Ok(PreparedAgentToolCall {
        command,
        home_dir,
        home_alias_dir,
        tool_executable,
        name: tool_call.name.clone(),
        approval: view.approval(),
        working_set: (tool_call.name != "tsh").then(|| {
            (
                context_path,
                cortexfs::TshLoadedToolState {
                    name: tool_call.name.clone(),
                    path: hit.path().to_path_buf(),
                    description: String::new(),
                    schema: None,
                    dynamic_resident: true,
                    pinned: false,
                    last_used: 0,
                },
                tsh_working_set_limit(&view),
            )
        }),
    })
}

fn tsh_working_set_limit(view: &cortexfs::AgentRuntimeView) -> usize {
    let Some(hit) = view.tool_path().find("tsh").ok().flatten() else {
        return cortexfs::tool::core::tools::TshRuntimeConfig::default().max_loaded_tools;
    };
    let path = hit.control_dir().join("config");
    let Ok(content) = read_small_plain_text_file(&path, MAX_RUNNER_CONTROL_BYTES, "runner") else {
        return cortexfs::tool::core::tools::TshRuntimeConfig::default().max_loaded_tools;
    };
    cortexfs::tool::core::tools::parse_tsh_runtime_config(&content)
        .unwrap_or_default()
        .max_loaded_tools
}

impl PreparedAgentToolCall {
    pub(crate) const fn approval(&self) -> cortexfs::AgentApprovalMode {
        self.approval
    }
    pub(crate) fn execute(
        mut self,
        config: &AgentToolExecutionConfig<'_>,
    ) -> Result<String, String> {
        let output = if let Some((session_dir, run)) = config.cancel {
            run_agent_tool_process_cancellable(&mut self.command, || {
                crate::agent_run_cancelled(session_dir, run)
            })
        } else {
            run_agent_tool_process(&mut self.command)
        }
        .map_err(|error| format!("cannot run tool:{}: {error}", self.name))?;
        drop(self.home_dir);
        drop(self.home_alias_dir);
        drop(self.tool_executable);
        let result = finish_agent_tool_output(&output)?;
        if let Some((path, tool, limit)) = self.working_set {
            cortexfs::retain_tsh_context_tool(&path, tool, limit)
                .map_err(|error| format!("cannot persist session tool context: {error}"))?;
        }
        Ok(result)
    }
}

fn finish_agent_tool_output(output: &std::process::Output) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed = parse_tool_stdout(&stdout).map_err(|error| trim_tool_result(&error))?;
    let mut result = match parsed {
        ToolStdout::Legacy(text) | ToolStdout::SdkSuccess(text) => text,
        ToolStdout::SdkError(error) => return Err(trim_tool_result(&error)),
    };
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

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ToolStdout {
    Legacy(String),
    SdkSuccess(String),
    SdkError(String),
}

pub(crate) fn parse_tool_stdout(output: &str) -> Result<ToolStdout, String> {
    let Some(first_line) = output.lines().find(|line| !line.is_empty()) else {
        return Ok(ToolStdout::Legacy(output.to_owned()));
    };
    let Ok(first) = serde_json::from_str::<Value>(first_line) else {
        return Ok(ToolStdout::Legacy(output.to_owned()));
    };
    if first.get("type").and_then(Value::as_str) != Some("start") {
        return Ok(ToolStdout::Legacy(output.to_owned()));
    }
    let run = sdk_frame_string(&first, &["type", "run", "tool"], "run")?;
    let mut text = String::new();
    let mut error = None;
    let mut done = None;
    for line in output.lines().skip_while(|line| line.is_empty()).skip(1) {
        if line.is_empty() || done.is_some() {
            return Err("invalid CortexFS Tool SDK output after start".to_owned());
        }
        let frame = serde_json::from_str::<Value>(line)
            .map_err(|_error| "invalid CortexFS Tool SDK JSONL after start".to_owned())?;
        if frame.get("run").and_then(Value::as_str) != Some(run) {
            return Err("CortexFS Tool SDK output run mismatch".to_owned());
        }
        match frame.get("type").and_then(Value::as_str) {
            Some("message") if error.is_none() => {
                sdk_exact_keys(&frame, &["type", "run", "role", "content"])?;
                if frame.get("role").and_then(Value::as_str) != Some("tool") {
                    return Err("invalid CortexFS Tool SDK message role".to_owned());
                }
                let items = frame
                    .get("content")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "invalid CortexFS Tool SDK message content".to_owned())?;
                for item in items {
                    sdk_exact_keys(item, &["type", "text"])?;
                    if item.get("type").and_then(Value::as_str) != Some("text") {
                        return Err("invalid CortexFS Tool SDK content item".to_owned());
                    }
                    text.push_str(sdk_frame_string(item, &["type", "text"], "text")?);
                }
            }
            Some("error") if error.is_none() => {
                sdk_exact_keys(&frame, &["type", "run", "code", "message"])?;
                let code = sdk_frame_string(&frame, &["type", "run", "code", "message"], "code")?;
                let message =
                    sdk_frame_string(&frame, &["type", "run", "code", "message"], "message")?;
                error = Some(format!("{code}: {message}"));
            }
            Some("done") => {
                sdk_exact_keys(&frame, &["type", "run", "status"])?;
                done = Some(
                    sdk_frame_string(&frame, &["type", "run", "status"], "status")?.to_owned(),
                );
            }
            _ => return Err("invalid CortexFS Tool SDK frame sequence".to_owned()),
        }
    }
    match (error, done.as_deref()) {
        (None, Some("ok")) => Ok(ToolStdout::SdkSuccess(text)),
        (Some(error), Some("error")) => Ok(ToolStdout::SdkError(error)),
        _ => Err("invalid CortexFS Tool SDK terminal status".to_owned()),
    }
}

fn sdk_frame_string<'a>(frame: &'a Value, keys: &[&str], name: &str) -> Result<&'a str, String> {
    sdk_exact_keys(frame, keys)?;
    frame
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("invalid CortexFS Tool SDK {name}"))
}

fn sdk_exact_keys(frame: &Value, keys: &[&str]) -> Result<(), String> {
    let object = frame
        .as_object()
        .ok_or_else(|| "CortexFS Tool SDK frame is not an object".to_owned())?;
    if object.len() != keys.len() || !keys.iter().all(|key| object.contains_key(*key)) {
        return Err("CortexFS Tool SDK frame has invalid fields".to_owned());
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct AgentToolSandbox {
    pub(crate) workspace: PathBuf,
    pub(crate) upper: PathBuf,
    pub(crate) work: PathBuf,
}

pub(crate) struct AgentToolBwrapArgs<'a> {
    pub(crate) config: &'a AgentToolExecutionConfig<'a>,
    pub(crate) tool_executable: &'a Path,
    pub(crate) tool_args: &'a [OsString],
    pub(crate) env: &'a [(String, String)],
    pub(crate) mount_table: &'a cortexfs::MountTable,
    pub(crate) cwd: &'a Path,
    pub(crate) sandbox: Option<&'a AgentToolSandbox>,
    pub(crate) network_allowed: bool,
    pub(crate) home_fd: RawFd,
    pub(crate) home_alias_fd: RawFd,
    pub(crate) home_target: &'a Path,
    pub(crate) ctx_home_target: &'a Path,
    pub(crate) control: Option<&'a (PathBuf, OsString)>,
}

pub(crate) fn nested_control_environment(
    socket: Option<OsString>,
    token: Option<OsString>,
) -> Result<Option<(PathBuf, OsString)>, String> {
    match (socket, token) {
        (None, None) => Ok(None),
        (Some(socket), Some(token)) => {
            validate_nested_control_values(&socket, &token)?;
            let socket = PathBuf::from(socket);
            let metadata = fs::symlink_metadata(&socket)
                .map_err(|error| format!("cannot inspect CTX_CONTROL_SOCKET: {error}"))?;
            if !nested_control_socket_is_plain(&metadata) {
                return Err("CTX_CONTROL_SOCKET is not a plain socket".to_owned());
            }
            Ok(Some((socket, token)))
        }
        _ => Err("incomplete CTX_CONTROL_SOCKET/CTX_CONTROL_TOKEN pair".to_owned()),
    }
}

pub(crate) fn nested_control_socket_is_plain(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_socket() && !metadata.file_type().is_symlink()
}

pub(crate) fn validate_nested_control_values(socket: &OsStr, token: &OsStr) -> Result<(), String> {
    if socket != OsStr::new(crate::runtime::socket::SOCKET_RUN_CONTROL_PATH) {
        return Err("CTX_CONTROL_SOCKET is not the fixed runtime control path".to_owned());
    }
    let token = token
        .to_str()
        .ok_or_else(|| "CTX_CONTROL_TOKEN is not ASCII hex".to_owned())?;
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("CTX_CONTROL_TOKEN is not a 32-byte hex token".to_owned());
    }
    Ok(())
}

pub(crate) fn prepare_agent_tool_sandbox(
    view: &cortexfs::AgentRuntimeView,
    source: &Path,
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
    let root = source
        .join("home")
        .join(view.owner().to_string())
        .join("agent")
        .join(view.agent_name())
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
    if !request.network_allowed {
        args.push(OsString::from("--unshare-net"));
    }
    args.extend(optional_dev_toolchain_bind_args());
    args.extend(agent_tool_env_bwrap_args(request));
    if let Some(control) = request.control {
        let socket = &control.0;
        args.extend(bwrap_dir_args_for_parent(&socket.display().to_string()));
        args.extend([
            OsString::from("--bind"),
            socket.as_os_str().to_owned(),
            socket.as_os_str().to_owned(),
        ]);
    }
    args.extend(bwrap_source_root_bind_args(request.config.source));
    for mount in request.mount_table.entries() {
        if request.sandbox.is_some() && path_uses_workspace(mount.target()) {
            continue;
        }
        let Some(source) =
            visible_mount_source(request.config.source, mount.source(), mount.target())
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
    args.extend([
        OsString::from("--bind-fd"),
        OsString::from(request.home_fd.to_string()),
        request.home_target.as_os_str().to_owned(),
        OsString::from("--bind-fd"),
        OsString::from(request.home_alias_fd.to_string()),
        OsString::from("/home/agent"),
    ]);
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

fn agent_tool_env_bwrap_args(request: &AgentToolBwrapArgs<'_>) -> Vec<OsString> {
    let mut args = Vec::new();
    for env in request.env {
        if is_secret_env_name(&env.0)
            || matches!(
                env.0.as_str(),
                "CTX_ROOT" | "CTX_PROVIDER_CONFIG_DIR" | "CTX_HOME" | "HOME" | "PATH" | "CTX_PATH"
            )
        {
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
        OsString::from("CTX_SESSION"),
        OsString::from(&request.config.session),
        OsString::from("--setenv"),
        OsString::from("CTX_RUN_ID"),
        OsString::from(&request.config.run),
        OsString::from("--setenv"),
        OsString::from("CTX_ROOT"),
        OsString::from(DEFAULT_CTX_ROOT),
        OsString::from("--setenv"),
        OsString::from("CTX_PROVIDER_CONFIG_DIR"),
        OsString::from("/ctx/shared/providers.d"),
        OsString::from("--setenv"),
        OsString::from("CTX_HOME"),
        request.ctx_home_target.as_os_str().to_owned(),
        OsString::from("--setenv"),
        OsString::from("HOME"),
        OsString::from("/home/agent"),
        OsString::from("--setenv"),
        OsString::from("CTX_PATH"),
        OsString::from(format!(
            "/ctx/tool:{}/tool",
            request.ctx_home_target.display()
        )),
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
    if let Some(control) = request.control {
        let socket = &control.0;
        let token = &control.1;
        args.extend([
            OsString::from("--setenv"),
            OsString::from("CTX_CONTROL_SOCKET"),
            socket.as_os_str().to_owned(),
            OsString::from("--setenv"),
            OsString::from("CTX_CONTROL_TOKEN"),
            token.clone(),
        ]);
    }
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
    if crate::is_stable_chroot_absolute_path(target) && Path::new(target).exists() {
        return Some(target.to_owned());
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

pub(crate) fn run_agent_tool_process_cancellable(
    command: &mut Command,
    mut cancelled: impl FnMut() -> bool,
) -> Result<std::process::Output, String> {
    run_agent_tool_process_with_timeout_and_cancel(
        command,
        Duration::from_secs(agent_tool_timeout_seconds()),
        &mut cancelled,
    )
}

#[cfg_attr(not(test), expect(dead_code, reason = "focused timeout seam"))]
pub(crate) fn run_agent_tool_process_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    run_agent_tool_process_with_timeout_and_cancel(command, timeout, &mut || false)
}

fn run_agent_tool_process_with_timeout_and_cancel(
    command: &mut Command,
    timeout: Duration,
    cancelled: &mut impl FnMut() -> bool,
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
        if cancelled() {
            terminate_process_group(&mut child);
            let _ignored = child.wait();
            return Err("tool cancelled".to_owned());
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
