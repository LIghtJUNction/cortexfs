use super::*;

pub(crate) fn run_agent(name: &str, args: &[OsString]) -> Result<(), String> {
    crate::runtime::control::ping_from_environment(name)
        .map_err(|error| format!("run capability handshake failed: {error:?}"))?;
    let input = collect_input(args).map_err(|error| format!("cannot read input: {error}"))?;
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut config = AgentModelRunConfig::new(name)?;
    write_agent_debug_timing(&mut stdout, &config, "agent_runner_ready")?;
    if !is_regular_file_no_follow(&config.model_path) {
        let message = missing_model_message(&config.ctx_root, &config.model, &config.model_path);
        return write_tool_start(&mut stdout, &config.run, name)
            .and_then(|()| write_tool_error(&mut stdout, &config.run, "ENOENT", &message))
            .map_err(|error| format!("cannot write output: {error}"));
    }
    run_agent_tool_loop(
        &mut config,
        &input,
        &mut stdout,
        run_agent_model_once,
        execute_agent_tool_call,
    )
}

pub(crate) fn run_agent_tool_loop<W, M, T>(
    config: &mut AgentModelRunConfig,
    input: &str,
    stdout: &mut W,
    mut run_model_once: M,
    mut execute_tool_call: T,
) -> Result<(), String>
where
    W: Write,
    M: FnMut(&AgentModelRunConfig, &str, &mut W) -> Result<AgentModelRunOutcome, String>,
    T: FnMut(&AgentModelRunConfig, &AgentToolCall) -> Result<String, String>,
{
    let mut last_tool_signature = None;
    let mut last_tool_result: Option<(AgentToolCall, String)> = None;
    for iteration in 0..=MAX_AGENT_TOOL_ITERATIONS {
        let suppress_model_error_events = config.suppress_model_error_events;
        if last_tool_result.is_some() {
            config.suppress_model_error_events = true;
        }
        let outcome = match run_model_once(config, input, stdout) {
            Ok(outcome) => outcome,
            Err(error) => {
                config.suppress_model_error_events = suppress_model_error_events;
                if let Some(pair) = last_tool_result.as_ref() {
                    return write_tool_result_fallback_response(
                        stdout,
                        &config.run,
                        &pair.0,
                        &pair.1,
                    );
                }
                return Err(error);
            }
        };
        config.suppress_model_error_events = suppress_model_error_events;
        if let Some(tool_call) = first_tool_call(&outcome.frames)? {
            let signature = tool_call_signature(&tool_call);
            if last_tool_signature.as_deref() == Some(signature.as_str()) {
                if let Some(pair) = last_tool_result.as_ref() {
                    return write_tool_result_fallback_response(
                        stdout,
                        &config.run,
                        &pair.0,
                        &pair.1,
                    );
                }
                return write_tool_loop_handoff_response(
                    stdout,
                    &config.run,
                    "agent repeated the same tool call",
                    Some(&tool_call),
                )
                .map_err(|error| format!("cannot write output: {error}"));
            }
            write_agent_frames_for_tool_iteration(
                stdout,
                &config.run,
                &outcome.frames,
                &tool_call,
            )?;
            emit_agent_terminal_tool_running(config, &tool_call);
            let (result, success) = match execute_tool_call(config, &tool_call) {
                Ok(result) => (result, true),
                Err(error) => (format!("ERROR: {error}\n"), false),
            };
            emit_agent_terminal_tool_done(config, &tool_call, &result, success);
            write_tool_result_event(stdout, &config.run, &tool_call, &result)?;
            stdout
                .flush()
                .map_err(|error| format!("cannot write output: {error}"))?;
            config.push_tool_result(&tool_call, &result);
            last_tool_signature = Some(signature);
            last_tool_result = Some((tool_call, result));
            if iteration == MAX_AGENT_TOOL_ITERATIONS {
                let tool_call = last_tool_result.as_ref().map(|pair| &pair.0);
                return write_tool_loop_handoff_response(
                    stdout,
                    &config.run,
                    "agent tool loop limit exceeded",
                    tool_call,
                )
                .map_err(|error| format!("cannot write output: {error}"));
            }
            continue;
        }

        if outcome.success
            && last_tool_result.is_some()
            && !frames_have_visible_assistant_response(&outcome.frames)
            && let Some(pair) = last_tool_result.as_ref()
        {
            return write_tool_result_fallback_response(stdout, &config.run, &pair.0, &pair.1);
        }
        if !outcome.success
            && last_tool_result.is_some()
            && frames_have_error(&outcome.frames)
            && let Some(pair) = last_tool_result.as_ref()
        {
            return write_tool_result_fallback_response(stdout, &config.run, &pair.0, &pair.1);
        }

        if outcome.streamed {
            write_done_frames(stdout, &outcome.frames)?;
            if outcome.success || frames_have_error(&outcome.frames) {
                if outcome.success && !frames_have_error(&outcome.frames) {
                    write_success_done_if_missing(stdout, &config.run, &outcome.frames)?;
                }
                return Ok(());
            }
            return Err("agent model failed".to_owned());
        }
        write_agent_frames(stdout, &config.run, &outcome.frames)?;
        if outcome.success || frames_have_error(&outcome.frames) {
            if outcome.success && !frames_have_error(&outcome.frames) {
                write_success_done_if_missing(stdout, &config.run, &outcome.frames)?;
            }
            return Ok(());
        }
        return Err("agent model failed".to_owned());
    }

    Ok(())
}

pub(crate) fn emit_agent_terminal_tool_running(
    config: &AgentModelRunConfig,
    tool_call: &AgentToolCall,
) {
    emit_agent_terminal_tool_line(config, &tool_terminal_running_line(tool_call));
}

pub(crate) fn emit_agent_terminal_tool_done(
    config: &AgentModelRunConfig,
    tool_call: &AgentToolCall,
    result: &str,
    success: bool,
) {
    emit_agent_terminal_tool_line(config, &tool_terminal_done_line(tool_call, result, success));
}

pub(crate) fn emit_agent_terminal_tool_line(config: &AgentModelRunConfig, line: &str) {
    for socket in agent_terminal_emit_sockets(config) {
        let Ok(mut stream) = UnixStream::connect(socket) else {
            continue;
        };
        let _ignored = stream.set_write_timeout(Some(Duration::from_secs(1)));
        if stream
            .write_all(b"emit\n")
            .and_then(|()| stream.write_all(line.as_bytes()))
            .and_then(|()| stream.flush())
            .is_ok()
        {
            return;
        }
    }
}

pub(crate) fn agent_terminal_emit_sockets(config: &AgentModelRunConfig) -> Vec<PathBuf> {
    let Some(session) = env::var("CTX_SESSION").ok() else {
        return Vec::new();
    };
    if !is_object_name(&session) {
        return Vec::new();
    }
    let Ok(view) = derive_agent_runtime_view(&config.ctx_root, &config.agent) else {
        return Vec::new();
    };
    let mut sockets = vec![agent_terminal_visible_socket(
        view.ctx_home(),
        &config.agent,
        &session,
    )];
    if let Some(runtime) = agent_terminal_runtime_socket(view.ctx_home(), &config.agent, &session) {
        sockets.push(runtime);
    }
    sockets.push(agent_terminal_legacy_runtime_socket(
        view.ctx_home(),
        &config.agent,
        &session,
    ));
    sockets
}

pub(crate) fn agent_terminal_visible_socket(
    ctx_home: &Path,
    agent: &str,
    session: &str,
) -> PathBuf {
    ctx_home
        .join("agent")
        .join(agent)
        .join("session")
        .join(session)
        .join("terminal")
        .join("main.sock")
}

pub(crate) fn agent_terminal_runtime_socket(
    ctx_home: &Path,
    agent: &str,
    session: &str,
) -> Option<PathBuf> {
    Some(
        agent_terminal_runtime_root(ctx_home)?
            .join("cortexfs")
            .join("terminal")
            .join(agent)
            .join(session)
            .join("main.sock"),
    )
}

pub(crate) fn agent_terminal_legacy_runtime_socket(
    ctx_home: &Path,
    agent: &str,
    session: &str,
) -> PathBuf {
    PathBuf::from("/run")
        .join("cortexfs")
        .join("terminal")
        .join(ctx_home_uid(ctx_home).unwrap_or_else(|| nix::unistd::Uid::effective().to_string()))
        .join(agent)
        .join(session)
        .join("main.sock")
}

pub(crate) fn agent_terminal_runtime_root(ctx_home: &Path) -> Option<PathBuf> {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            Some(
                PathBuf::from("/run")
                    .join("user")
                    .join(ctx_home_uid(ctx_home)?),
            )
        })
}

pub(crate) fn ctx_home_uid(ctx_home: &Path) -> Option<String> {
    ctx_home
        .file_name()
        .and_then(|uid| uid.to_str())
        .filter(|uid| uid.bytes().all(|byte| byte.is_ascii_digit()))
        .map(str::to_owned)
}

pub(crate) fn tool_terminal_running_line(tool_call: &AgentToolCall) -> String {
    let args = tool_terminal_args(tool_call);
    if args.is_empty() {
        format!(
            "\r\ntool {} running\r\n",
            terminal_safe_text(&tool_call.name)
        )
    } else {
        format!(
            "\r\ntool {} running {}\r\n",
            terminal_safe_text(&tool_call.name),
            args
        )
    }
}

pub(crate) fn tool_terminal_done_line(
    tool_call: &AgentToolCall,
    result: &str,
    success: bool,
) -> String {
    let status = if success { "done" } else { "error" };
    format!(
        "\r\ntool {} {} {} bytes\r\n",
        terminal_safe_text(&tool_call.name),
        status,
        result.len()
    )
}

pub(crate) fn tool_terminal_args(tool_call: &AgentToolCall) -> String {
    let mut parts = Vec::new();
    for (index, arg) in tool_call.args.iter().enumerate() {
        let text = arg.to_string_lossy();
        if index == 0 {
            parts.push(terminal_safe_text(&text));
        } else {
            parts.push(terminal_safe_text(&tool_terminal_quote(&text)));
        }
    }
    parts.join(" ")
}

pub(crate) fn tool_terminal_quote(value: &str) -> String {
    if value.bytes().all(|byte| {
        matches!(
            byte,
            b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'_'
                | b'-'
                | b'.'
                | b'/'
                | b':'
                | b'@'
                | b'%'
                | b'+'
                | b'='
                | b','
        )
    }) {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

pub(crate) fn terminal_safe_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() && character != '\n' && character != '\t' {
                ' '
            } else {
                character
            }
        })
        .collect()
}

pub(crate) struct AgentModelRunConfig {
    pub(crate) agent: String,
    pub(crate) source: PathBuf,
    pub(crate) ctx_root: PathBuf,
    pub(crate) run: String,
    pub(crate) session: String,
    pub(crate) model: String,
    pub(crate) model_path: PathBuf,
    pub(crate) system_prompt: String,
    pub(crate) prompt_template: String,
    pub(crate) rules: String,
    pub(crate) skills: String,
    pub(crate) current_time_unix: String,
    pub(crate) tool_context: String,
    pub(crate) history_messages: String,
    pub(crate) window_setting: AgentWindowSetting,
    pub(crate) context_budget: Option<AgentWindowBudget>,
    pub(crate) suppress_model_error_events: bool,
    pub(crate) debug_timing_start_unix_ms: Option<u128>,
}

impl AgentModelRunConfig {
    fn new(agent: &str) -> Result<Self, String> {
        let source =
            env::var_os("CTX_SOURCE").map_or_else(|| PathBuf::from(DEFAULT_SOURCE), PathBuf::from);
        let ctx_root =
            env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
        Self::new_with_paths(agent, source, ctx_root)
    }

    pub(crate) fn new_with_paths(
        agent: &str,
        source: PathBuf,
        ctx_root: PathBuf,
    ) -> Result<Self, String> {
        let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
        let session = env::var("CTX_SESSION").unwrap_or_else(|_error| "default".to_owned());
        let agent_dir = agent_model_control_dir(&source, agent);
        let model_path = agent_dir.join("model");
        let configured_model =
            read_small_plain_text_file(&model_path, MAX_RUNNER_CONTROL_BYTES, "runner")
                .map_or_else(
                    |_error| "main".to_owned(),
                    |content| content.trim().to_owned(),
                );
        let configured_model = if configured_model.is_empty() {
            "main".to_owned()
        } else {
            configured_model
        };
        let requested_model = env::var("CTX_AGENT_MODEL_OVERRIDE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(configured_model);
        let requested_model = if is_model_name(&requested_model) || is_model_alias(&requested_model)
        {
            requested_model
        } else {
            return Err(format!("invalid model reference: {requested_model}"));
        };
        let primary_model = resolved_model_name(&ctx_root, &requested_model)?;
        let candidates = model_candidates(&ctx_root, &requested_model)?;
        let window_content = read_small_plain_text_file(
            &agent_dir.join("window"),
            MAX_RUNNER_CONTROL_BYTES,
            "runner",
        )
        .map_err(|error| format!("cannot read agent window control: {error}"))?;
        let window_setting = AgentWindowSetting::parse_control(&window_content)
            .ok_or_else(|| "invalid agent window control".to_owned())?;
        let _inherited_budget = parse_agent_context_budget(
            env::var("CTX_CONTEXT_WINDOW_TOKENS").ok().as_deref(),
            env::var("CTX_CONTEXT_WINDOW_CHARS").ok().as_deref(),
        )?;
        let mut rejected = Vec::new();
        let mut selected = None;
        for candidate in &candidates {
            if !is_regular_file_no_follow(&candidate.path) {
                rejected.push(format!("{}: missing executable", candidate.name));
                continue;
            }
            match candidate_window_budget(&ctx_root, &candidate.name, window_setting) {
                Ok(budget) => {
                    selected = Some((candidate, budget));
                    break;
                }
                Err(error) => rejected.push(format!("{}: {error}", candidate.name)),
            }
        }
        let (selected, context_budget) = selected.ok_or_else(|| {
            format!(
                "no eligible model candidate for {requested_model}: {}",
                rejected.join("; ")
            )
        })?;
        let model_path = selected.path.clone();
        let model = selected.name.clone();
        authorize_agent_model_use(&agent_dir, &requested_model, &primary_model, &model)?;
        let system_prompt = read_small_plain_text_file(
            &agent_dir.join("system.md"),
            MAX_RUNNER_CONTROL_BYTES,
            "runner",
        )
        .unwrap_or_default();
        let prompt_template = read_small_plain_text_file(
            &agent_dir.join("prompt.template.md"),
            MAX_RUNNER_CONTROL_BYTES,
            "runner",
        )
        .unwrap_or_else(|_error| DEFAULT_AGENT_PROMPT_TEMPLATE.to_owned());
        let rules = collect_agent_rules();
        let skills = collect_skill_metadata(skill_metadata_budget_from_env());
        write_run_snapshot(&ctx_root, agent, &rules, &skills);
        Ok(Self {
            agent: agent.to_owned(),
            source,
            ctx_root,
            run,
            session,
            model,
            model_path,
            system_prompt,
            prompt_template,
            rules,
            skills,
            current_time_unix: current_time_unix().to_string(),
            tool_context: env::var("CTX_AGENT_TOOL_CONTEXT").unwrap_or_default(),
            history_messages: env::var("CTX_AGENT_HISTORY_MESSAGES")
                .unwrap_or_else(|_error| "(no historical messages injected)".to_owned()),
            window_setting,
            context_budget,
            suppress_model_error_events: false,
            debug_timing_start_unix_ms: agent_debug_timing_start_unix_ms(),
        })
    }

    pub(crate) fn push_tool_result(&mut self, tool_call: &AgentToolCall, result: &str) {
        if !self.tool_context.trim().is_empty() {
            self.tool_context.push_str("\n\n");
        }
        self.tool_context.push_str("Tool result ");
        self.tool_context.push_str(&tool_call.id);
        self.tool_context.push_str(" from ");
        self.tool_context.push_str(&tool_call.name);
        self.tool_context.push_str(" args ");
        self.tool_context.push_str(&tool_call_args_json(tool_call));
        self.tool_context.push_str(":\n");
        self.tool_context.push_str(result);
        trim_tool_context_to_limit(&mut self.tool_context);
    }
}

pub(crate) fn candidate_window_budget(
    ctx_root: &Path,
    model: &str,
    setting: AgentWindowSetting,
) -> Result<Option<AgentWindowBudget>, String> {
    let (provider, name) = model
        .split_once('/')
        .ok_or_else(|| "invalid model candidate".to_owned())?;
    let content = read_small_plain_text_file(
        &ctx_root
            .join("model")
            .join(provider)
            .join(format!("{name}.d/limit")),
        MAX_RUNNER_CONTROL_BYTES,
        "runner",
    )
    .map_err(|error| format!("cannot read model context limit: {error}"))?;
    let limit = ModelContextLimit::parse_control(&content)
        .ok_or_else(|| "invalid model context limit".to_owned())?;
    let effective = setting
        .resolve(limit)
        .map_err(|error| format!("ineligible context limit: {error:?}"))?;
    Ok(AgentWindowBudget::from_effective(effective))
}

pub(crate) fn parse_agent_context_budget(
    tokens: Option<&str>,
    chars: Option<&str>,
) -> Result<Option<AgentWindowBudget>, String> {
    let (Some(tokens), Some(chars)) = (tokens, chars) else {
        return if tokens.is_none() && chars.is_none() {
            Ok(None)
        } else {
            Err(
                "invalid context window environment: token and character values must be paired"
                    .to_owned(),
            )
        };
    };
    let token_value = tokens.parse::<u32>().map_err(|_error| {
        "invalid context window environment: token value is not canonical decimal".to_owned()
    })?;
    if token_value == 0 || tokens != token_value.to_string() {
        return Err(
            "invalid context window environment: token value is not canonical decimal".to_owned(),
        );
    }
    let window = ModelContextLimit::known(token_value)
        .and_then(AgentWindowBudget::from_effective)
        .ok_or_else(|| "invalid context window environment: token value is zero".to_owned())?;
    let char_value = chars.parse::<usize>().map_err(|_error| {
        "invalid context window environment: character value is not canonical decimal".to_owned()
    })?;
    if chars != char_value.to_string() || char_value != window.total_chars() {
        return Err(
            "invalid context window environment: character value does not match tokens".to_owned(),
        );
    }
    Ok(Some(window))
}

pub(crate) fn serialized_agent_messages(
    config: &AgentModelRunConfig,
    input: &str,
) -> Result<Vec<u8>, String> {
    let context = AgentPromptContext {
        template: config.prompt_template.clone(),
        rules: config.rules.clone(),
        skills: config.skills.clone(),
        tool_injection: config.tool_context.clone(),
        history_messages: config.history_messages.clone(),
        current_time_unix: config.current_time_unix.clone(),
    };
    let messages = agent_provider_messages(input, &config.agent, &config.system_prompt, &context);
    serde_json::to_vec(&messages).map_err(|error| format!("cannot serialize agent prompt: {error}"))
}

pub(crate) fn admit_agent_prompt(
    config: &AgentModelRunConfig,
    input: &str,
) -> Result<bool, String> {
    let Some(budget) = config.context_budget else {
        return Ok(true);
    };
    let messages = serialized_agent_messages(config, input)?;
    Ok(messages.len() <= budget.input_chars())
}

pub(crate) fn agent_model_control_dir(source: &Path, agent: &str) -> PathBuf {
    for control in current_user_agent_model_control_dirs(source, agent) {
        if is_plain_directory_no_follow(&control) {
            return control;
        }
    }
    source.join("agent").join(format!("{agent}.d"))
}

pub(crate) fn current_user_agent_model_control_dirs(source: &Path, agent: &str) -> Vec<PathBuf> {
    let mut controls = Vec::new();
    if let Some(ctx_home) = env::var_os("CTX_HOME").map(PathBuf::from)
        && ctx_home.starts_with(source)
    {
        controls.push(ctx_home.join("agent").join(format!("{agent}.d")));
    }
    let uid_control = source
        .join("home")
        .join(nix::unistd::Uid::effective().as_raw().to_string())
        .join("agent")
        .join(format!("{agent}.d"));
    if !controls.iter().any(|control| control == &uid_control) {
        controls.push(uid_control);
    }
    controls
}

pub(crate) fn is_plain_directory_no_follow(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_dir() && !metadata.file_type().is_symlink())
}
