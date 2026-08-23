use super::*;
use crate::ToolPath;
use std::os::fd::{AsRawFd, RawFd};
use std::sync::Arc;
pub(crate) struct AgentToolExecutionConfig<'a> {
    pub(crate) agent: &'a str,
    pub(crate) source: &'a Path,
    pub(crate) ctx_root: &'a Path,
    pub(crate) run: &'a str,
    pub(crate) session: &'a str,
    pub(crate) control: Option<AgentToolControl>,
    pub(crate) cancel: Option<(&'a Path, &'a str)>,
    pub(crate) tool_path: Option<&'a ToolPath>,
    pub(crate) channel: Option<&'a crate::runtime::channelenv::ChannelRuntimeContext>,
}

#[derive(Clone, Debug)]
pub(crate) struct AgentToolControl {
    pub(crate) source: PathBuf,
    pub(crate) target: PathBuf,
    capability: Arc<crate::runtime::control::RunCapability>,
}

impl AgentToolControl {
    pub(crate) fn new(
        source: PathBuf,
        target: PathBuf,
        capability: Arc<crate::runtime::control::RunCapability>,
    ) -> Self {
        Self {
            source,
            target,
            capability,
        }
    }

    pub(crate) fn launch_gate(&self) -> Result<crate::runtime::control::LaunchGate, ExecError> {
        self.capability
            .launch_gate()
            .map_err(|_error| ExecError::new("cannot create run control launch gate"))
    }
}
pub(crate) struct PreparedAgentToolCall {
    command: Command,
    home_dir: fs::File,
    home_alias_dir: fs::File,
    tool_executable: fs::File,
    name: String,
    approval: cortexfs::AgentApprovalMode,
    working_set: Option<(PathBuf, cortexfs::TshLoadedToolState, usize)>,
    control_gate: Option<crate::runtime::control::LaunchGate>,
}
pub(crate) fn prepare_agent_tool_call(
    config: &AgentToolExecutionConfig<'_>,
    tool_call: &AgentToolCall,
) -> Result<PreparedAgentToolCall, ExecError> {
    let view = derive_agent_runtime_view(config.ctx_root, config.agent).map_err(|error| {
        ExecError::new(format!("cannot derive agent authority: {}", error.errno()))
    })?;
    let owner = view.owner().to_string();
    let home_source = cortexfs_paths::agent_home_path(config.source, &owner, view.agent_name());
    let context_path = cortexfs::tsh_context_state_path(&cortexfs_paths::agent_session_path(
        config.source,
        &owner,
        view.agent_name(),
        config.session,
    ));
    let network_allowed = authorize_network_connect(
        "default",
        NetworkConnectAuthority::new(view.policy_subject(), view.policy()),
    )
    .is_ok();
    let tool_path = config.tool_path.unwrap_or_else(|| view.tool_path());
    validate_tool_admission(&view, config, tool_call)?;
    let Some(hit) = tool_path
        .find(&tool_call.name)
        .map_err(|error| ExecError::new(format!("cannot inspect CTX_PATH: {error:?}")))?
    else {
        return Err(ExecError::new(format!(
            "tool not found: {}",
            tool_call.name
        )));
    };
    let policy_path = hit.control_dir().join("policy");
    let policy_text = read_small_plain_text_file(&policy_path, MAX_RUNNER_CONTROL_BYTES, "runner")
        .map_err(|error| {
            ExecError::new(format!("cannot read {}: {error}", policy_path.display()))
        })?;
    let tool_policy = PolicyV0::parse(&policy_text)
        .map_err(|_error| ExecError::new(format!("invalid policy for tool:{}", tool_call.name)))?;
    let grant = authorize_tool_execution(
        tool_path,
        &tool_call.name,
        ToolExecutionAuthority::new(
            view.identity(),
            view.mount_table(),
            view.policy_subject(),
            view.policy(),
            &tool_policy,
            view.permissions(),
        ),
    )
    .map_err(|denial| ExecError::new(tool_denial_message(&tool_call.name, denial)))?;
    let tool_executable = open_executable_no_follow(grant.hit().path())
        .map_err(|error| ExecError::new(format!("cannot run tool:{}: {error}", tool_call.name)))?;
    let sandbox = prepare_agent_tool_sandbox(&view, config.source)?;
    let ctx_home_target = cortexfs_paths::ctx_home_path(&cortexfs_paths::ctx_root(), &owner);
    let authorized_object = authorized_tool_target(config.source, grant.hit());
    let home_target =
        cortexfs_paths::agent_home_path(&cortexfs_paths::ctx_root(), &owner, view.agent_name());
    let (home_dir, home_alias_dir) = open_agent_home_fds(&home_source)?;
    let mut command =
        crate::runtime::socket::command_for_agent_identity(BWRAP_PROGRAM, view.identity());
    let name = tool_call.name.as_str();
    let control = config
        .control
        .as_ref()
        .filter(|_| crate::runtime::control::consumes_run_control(name) || name == "tsh");
    let control_gate = control.map(AgentToolControl::launch_gate).transpose()?;
    command.args(agent_tool_bwrap_args(&AgentToolBwrapArgs {
        config,
        authorized_object: &authorized_object,
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
        control,
        control_gate: control_gate
            .as_ref()
            .map(crate::runtime::control::LaunchGate::block_fd),
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
                tsh_working_set_limit(tool_path),
            )
        }),
        control_gate,
    })
}

fn validate_tool_admission(
    view: &cortexfs::AgentRuntimeView,
    config: &AgentToolExecutionConfig<'_>,
    call: &AgentToolCall,
) -> Result<(), ExecError> {
    if call.name == "tsh" {
        return validate_agent_tsh_args(&call.args);
    }
    if config
        .channel
        .is_some_and(|channel| channel.is_channel_tool(&call.name))
    {
        if config
            .channel
            .is_none_or(|channel| !channel.allows_tool(&call.name))
        {
            return Err(ExecError::new(format!(
                "channel capability denied for tool {}",
                call.name
            )));
        }
        return Ok(());
    }
    if view.declared_tools().contains(&call.name) {
        Ok(())
    } else {
        Err(ExecError::new(format!(
            "unsupported native tool {}; declare it in the agent tools control",
            call.name
        )))
    }
}

fn open_agent_home_fds(home_source: &Path) -> Result<(fs::File, fs::File), ExecError> {
    let home_dir = open_plain_directory(home_source).map_err(|error| {
        ExecError::with_io(
            &format!("cannot open agent home {}", home_source.display()),
            &error,
        )
    })?;
    let home_alias_dir = home_dir
        .try_clone()
        .map_err(|error| ExecError::with_io("cannot duplicate agent home fd", &error))?;
    crate::provider::name::files::clear_fd_cloexec(&home_dir)
        .map_err(|error| ExecError::new(format!("cannot preserve agent home fd: {error:?}")))?;
    crate::provider::name::files::clear_fd_cloexec(&home_alias_dir).map_err(|error| {
        ExecError::new(format!("cannot preserve agent home alias fd: {error:?}"))
    })?;
    Ok((home_dir, home_alias_dir))
}

fn tsh_working_set_limit(tool_path: &ToolPath) -> usize {
    let Some(hit) = tool_path.find("tsh").ok().flatten() else {
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
    ) -> Result<String, ExecError> {
        let output = run_prepared_agent_tool(
            &mut self.command,
            self.control_gate.as_mut(),
            config.cancel,
        )
        .map_err(|error| ExecError::new(format!("cannot run tool:{}: {error}", self.name)))?;
        drop(self.home_dir);
        drop(self.home_alias_dir);
        drop(self.tool_executable);
        let result = finish_agent_tool_output(&output, &self.name)?;
        if let Some((path, tool, limit)) = self.working_set {
            cortexfs::retain_tsh_context_tool(&path, tool, limit).map_err(|error| {
                ExecError::new(format!("cannot persist session tool context: {error}"))
            })?;
        }
        Ok(result)
    }
}

pub(crate) fn finish_agent_tool_output(
    output: &std::process::Output,
    tool_name: &str,
) -> Result<String, ExecError> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut result = if is_passthrough_tool(tool_name) {
        stdout.into_owned()
    } else {
        let parsed = parse_tool_stdout(&stdout)
            .map_err(|error| ExecError::new(trim_tool_result(error.message())))?;
        match parsed {
            ToolStdout::SdkSuccess(text) => text,
            ToolStdout::SdkError { mut content, error } => {
                if !content.is_empty() && !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str(&error);
                return Err(ExecError::new(trim_tool_result(&content)));
            }
        }
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
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str("tool exited with ");
        result.push_str(&output.status.to_string());
        result.push('\n');
        return Err(ExecError::new(trim_tool_result(&result)));
    }
    Ok(trim_tool_result(&result))
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ToolStdout {
    SdkSuccess(String),
    SdkError { content: String, error: String },
}

pub(crate) fn parse_tool_stdout(output: &str) -> Result<ToolStdout, ExecError> {
    let Some(first_line) = output.lines().find(|line| !line.is_empty()) else {
        return Err(ExecError::new("empty CortexFS Tool SDK output"));
    };
    let first = serde_json::from_str::<Value>(first_line)
        .map_err(|_error| ExecError::new("invalid CortexFS Tool SDK JSONL start"))?;
    if first.get("type").and_then(Value::as_str) != Some("start") {
        return Err(ExecError::new(
            "CortexFS Tool SDK output must start with a start frame",
        ));
    }
    let run = sdk_frame_string(&first, &["type", "run", "tool"], "run")?;
    let mut content = Vec::new();
    let mut error = None;
    let mut done = None;
    for line in output.lines().skip_while(|line| line.is_empty()).skip(1) {
        if line.is_empty() || done.is_some() {
            return Err(ExecError::new(
                "invalid CortexFS Tool SDK output after start",
            ));
        }
        let frame = serde_json::from_str::<Value>(line)
            .map_err(|_error| ExecError::new("invalid CortexFS Tool SDK JSONL after start"))?;
        if frame.get("run").and_then(Value::as_str) != Some(run) {
            return Err(ExecError::new("CortexFS Tool SDK output run mismatch"));
        }
        match frame.get("type").and_then(Value::as_str) {
            Some("message") if error.is_none() => {
                sdk_exact_keys(&frame, &["type", "run", "role", "content"])?;
                if frame.get("role").and_then(Value::as_str) != Some("tool") {
                    return Err(ExecError::new("invalid CortexFS Tool SDK message role"));
                }
                let items = frame
                    .get("content")
                    .and_then(Value::as_array)
                    .ok_or_else(|| ExecError::new("invalid CortexFS Tool SDK message content"))?;
                for item in items {
                    if !item.is_object() || item.get("type").and_then(Value::as_str).is_none() {
                        return Err(ExecError::new("invalid CortexFS Tool SDK content item"));
                    }
                    content.push(item.clone());
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
            _ => return Err(ExecError::new("invalid CortexFS Tool SDK frame sequence")),
        }
    }
    let content = render_sdk_content(&content)?;
    match (error, done.as_deref()) {
        (None, Some("ok")) => Ok(ToolStdout::SdkSuccess(content)),
        (Some(error), Some("error")) => Ok(ToolStdout::SdkError { content, error }),
        _ => Err(ExecError::new("invalid CortexFS Tool SDK terminal status")),
    }
}

fn render_sdk_content(content: &[Value]) -> Result<String, ExecError> {
    let plain_text = content.iter().all(|item| {
        item.as_object().is_some_and(|object| {
            object.len() == 2
                && item.get("type").and_then(Value::as_str) == Some("text")
                && item.get("text").is_some_and(Value::is_string)
        })
    });
    if !plain_text {
        return serde_json::to_string(content)
            .map_err(|_error| ExecError::new("cannot serialize CortexFS Tool SDK content"));
    }
    let mut text = String::new();
    for item in content {
        text.push_str(sdk_frame_string(item, &["type", "text"], "text")?);
    }
    Ok(text)
}

/// Extracts and returns a required string field after validating exact frame keys.
fn sdk_frame_string<'a>(frame: &'a Value, keys: &[&str], name: &str) -> Result<&'a str, ExecError> {
    sdk_exact_keys(frame, keys)?;
    frame
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| ExecError::new(format!("invalid CortexFS Tool SDK {name}")))
}

fn sdk_exact_keys(frame: &Value, keys: &[&str]) -> Result<(), ExecError> {
    let object = frame
        .as_object()
        .ok_or_else(|| ExecError::new("CortexFS Tool SDK frame is not an object"))?;
    if object.len() != keys.len() || !keys.iter().all(|key| object.contains_key(*key)) {
        return Err(ExecError::new("CortexFS Tool SDK frame has invalid fields"));
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
    pub(crate) authorized_object: &'a Path,
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
    pub(crate) control: Option<&'a AgentToolControl>,
    pub(crate) control_gate: Option<RawFd>,
}

pub(crate) fn authorized_tool_target(source: &Path, hit: &cortexfs::ToolHit) -> PathBuf {
    hit.path().strip_prefix(source).map_or_else(
        |_error| hit.path().to_path_buf(),
        |relative| cortexfs_paths::ctx_root().join(relative),
    )
}

pub(crate) fn prepare_agent_tool_sandbox(
    view: &cortexfs::AgentRuntimeView,
    source: &Path,
) -> Result<Option<AgentToolSandbox>, ExecError> {
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
    let root = cortexfs_paths::session_file_path(
        source,
        &view.owner().to_string(),
        view.agent_name(),
        &session,
        "workspace-overlay",
    )
    .join(hash);
    let upper = root.join("upper");
    let work = root.join("work");
    fs::create_dir_all(&upper).map_err(|error| {
        ExecError::with_io(
            &format!("cannot create overlay upper {}", upper.display()),
            &error,
        )
    })?;
    fs::create_dir_all(&work).map_err(|error| {
        ExecError::with_io(
            &format!("cannot create overlay work {}", work.display()),
            &error,
        )
    })?;
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
    ];
    args.extend(bwrap_process_setup_args().into_iter().map(OsString::from));
    args.extend(bwrap_system_layout_args().into_iter().map(OsString::from));
    if let Some(channel) = request.config.channel {
        let socket = cortexfs_paths::channel_driver_socket(channel.channel());
        if socket.exists() {
            args.extend(
                crate::support::bwrap::readonly_bind_args(&socket)
                    .into_iter()
                    .map(OsString::from),
            );
        }
    }
    if !request.network_allowed {
        args.push(OsString::from("--unshare-net"));
    }
    args.extend(optional_dev_toolchain_bind_args());
    args.extend(agent_tool_env_bwrap_args(request));
    if let Some(control) = request.control {
        args.extend(bwrap_dir_args_for_parent(
            &control.target.display().to_string(),
        ));
        args.extend([
            OsString::from("--bind"),
            control.source.as_os_str().to_owned(),
            control.target.as_os_str().to_owned(),
        ]);
    }
    if let Some(gate) = request.control_gate {
        args.extend([
            OsString::from("--block-fd"),
            OsString::from(gate.to_string()),
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

fn sandbox_tool_path(path: &ToolPath, source: &Path) -> String {
    path.dirs()
        .iter()
        .map(|entry| {
            entry
                .strip_prefix(source)
                .map_or_else(
                    |_| entry.clone(),
                    |relative| cortexfs_paths::ctx_root().join(relative),
                )
                .display()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(":")
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
    let default_path = format!(
        "{}:{}",
        cortexfs_paths::tool_root_path(&cortexfs_paths::ctx_root()).display(),
        cortexfs_paths::home_tool_from_home_path(request.ctx_home_target).display()
    );
    let tool_path = request.config.tool_path.map_or(default_path, |path| {
        sandbox_tool_path(path, request.config.source)
    });
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
        OsString::from(cortexfs_paths::CTX_ROOT),
        OsString::from("--setenv"),
        OsString::from("CTX_PROVIDER_CONFIG_DIR"),
        OsString::from(cortexfs_paths::shared_path(
            &cortexfs_paths::ctx_root(),
            "providers.d",
        )),
        OsString::from("--setenv"),
        OsString::from("CTX_HOME"),
        request.ctx_home_target.as_os_str().to_owned(),
        OsString::from("--setenv"),
        OsString::from("HOME"),
        OsString::from("/home/agent"),
        OsString::from("--setenv"),
        OsString::from("CTX_PATH"),
        OsString::from(tool_path),
        OsString::from("--setenv"),
        OsString::from("CTX_SOURCE"),
        request.config.source.as_os_str().to_owned(),
        OsString::from("--setenv"),
        OsString::from("CTX_TOOL_MODE"),
        OsString::from("cli"),
        OsString::from("--setenv"),
        OsString::from("CTX_AUTHORIZED_OBJECT"),
        request.authorized_object.as_os_str().to_owned(),
        OsString::from("--setenv"),
        OsString::from("PATH"),
        OsString::from(crate::support::command::TRUSTED_PATH),
    ]);
    if let Some(channel) = request.config.channel {
        args.extend([
            OsString::from("--setenv"),
            OsString::from("CTX_CHANNEL_ID"),
            OsString::from(channel.channel()),
            OsString::from("--setenv"),
            OsString::from("CTX_CHANNEL_SESSION"),
            OsString::from(&request.config.session),
            OsString::from("--setenv"),
            OsString::from("CTX_CHANNEL_CAPS"),
            OsString::from(channel.caps()),
            OsString::from("--setenv"),
            OsString::from("CTX_CHANNEL_SOCKET"),
            OsString::from(
                cortexfs_paths::channel_driver_socket(channel.channel())
                    .display()
                    .to_string(),
            ),
        ]);
        append_channel_conversation(&mut args, channel);
    }
    if let Some(control) = request.control {
        let socket = &control.target;
        args.extend([
            OsString::from("--setenv"),
            OsString::from("CTX_CONTROL_SOCKET"),
            socket.as_os_str().to_owned(),
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

fn append_channel_conversation(
    args: &mut Vec<OsString>,
    channel: &crate::runtime::channelenv::ChannelRuntimeContext,
) {
    if let Some(conversation) = channel.conversation() {
        args.extend([
            OsString::from("--setenv"),
            OsString::from("CTX_CHANNEL_CONVERSATION"),
            OsString::from(conversation),
        ]);
    }
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
    let ctx_root = cortexfs_paths::ctx_root();
    if source_path == ctx_root {
        if !source_root.exists() && ctx_root.exists() {
            return cortexfs_paths::CTX_ROOT.to_owned();
        }
        return source_root.display().to_string();
    }
    if let Ok(relative) = source_path.strip_prefix(&ctx_root) {
        if !source_root.exists() && ctx_root.exists() {
            return ctx_root.join(relative).display().to_string();
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
    crate::support::bwrap::dir_args_for_parent(path)
        .into_iter()
        .map(OsString::from)
        .collect()
}

pub(crate) fn bwrap_dir_args_for_chdir(cwd: &str) -> Vec<OsString> {
    crate::support::bwrap::dir_args_for_chdir(cwd)
        .into_iter()
        .map(OsString::from)
        .collect()
}

pub(crate) fn run_agent_tool_process(
    command: &mut Command,
) -> Result<std::process::Output, ExecError> {
    run_agent_tool_process_with_timeout(command, Duration::from_secs(agent_tool_timeout_seconds()))
}

pub(crate) fn run_agent_tool_process_cancellable(
    command: &mut Command,
    mut cancelled: impl FnMut() -> bool,
) -> Result<std::process::Output, ExecError> {
    run_agent_tool_process_with_timeout_and_cancel(
        command,
        Duration::from_secs(agent_tool_timeout_seconds()),
        &mut cancelled,
        None,
    )
}

fn run_prepared_agent_tool(
    command: &mut Command,
    gate: Option<&mut crate::runtime::control::LaunchGate>,
    cancel: Option<(&Path, &str)>,
) -> Result<std::process::Output, ExecError> {
    if gate.is_none() {
        return match cancel {
            Some((session_dir, run)) => run_agent_tool_process_cancellable(command, || {
                crate::agent_run_cancelled(session_dir, run)
            }),
            None => run_agent_tool_process(command),
        };
    }
    let mut cancelled =
        || cancel.is_some_and(|(session_dir, run)| crate::agent_run_cancelled(session_dir, run));
    run_agent_tool_process_with_timeout_and_cancel(
        command,
        Duration::from_secs(agent_tool_timeout_seconds()),
        &mut cancelled,
        gate,
    )
}

#[cfg_attr(not(test), expect(dead_code, reason = "focused timeout seam"))]
pub(crate) fn run_agent_tool_process_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<std::process::Output, ExecError> {
    run_agent_tool_process_with_timeout_and_cancel(command, timeout, &mut || false, None)
}

fn run_agent_tool_process_with_timeout_and_cancel(
    command: &mut Command,
    timeout: Duration,
    cancelled: &mut impl FnMut() -> bool,
    gate: Option<&mut crate::runtime::control::LaunchGate>,
) -> Result<std::process::Output, ExecError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child =
        spawn_with_etxtbsy_retry(command).map_err(|error| ExecError::new(error.to_string()))?;
    if let Some(gate) = gate
        && gate.register_and_release(child.id()).is_err()
    {
        terminate_process_group(&mut child);
        let _ignored = child.wait();
        return Err(ExecError::new("cannot register run control launch root"));
    }
    wait_capped_child_output(
        &mut child,
        CappedOutputWait {
            max_output_bytes: MAX_AGENT_TOOL_OUTPUT_BYTES,
            timeout,
            capture_stderr: true,
            drain_timeout: Some(AGENT_TOOL_OUTPUT_DRAIN_TIMEOUT),
            terminate_group_after_exit: true,
        },
        cancelled,
    )
    .map_err(|error| match error {
        CappedOutputError::ExceededLimit => ExecError::new(format!(
            "tool output exceeds {MAX_AGENT_TOOL_OUTPUT_BYTES} bytes"
        )),
        CappedOutputError::TimedOut => {
            ExecError::new(format!("tool timed out after {}s", timeout.as_secs()))
        }
        CappedOutputError::Cancelled => ExecError::new("tool cancelled"),
        CappedOutputError::DrainTimedOut => ExecError::new(format!(
            "tool output did not close within {}s",
            AGENT_TOOL_OUTPUT_DRAIN_TIMEOUT.as_secs()
        )),
        CappedOutputError::Wait(error) => ExecError::new(error.to_string()),
    })
}
