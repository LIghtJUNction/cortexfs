use cortexfs_runtime_client::agent::{
    AGENT_ENVELOPE_ARG, AGENT_LAUNCH_ABI, AgentInvocationEnvelope, read_agent_invocation,
};

use super::*;

pub(crate) fn run_agent(name: &str, args: &[OsString]) -> Result<(), ExecError> {
    let Some(arg) = args.first().filter(|_arg| args.len() == 1) else {
        return Err(ExecError::new("invalid hosted agent invocation"));
    };
    if arg != OsStr::new(AGENT_ENVELOPE_ARG)
        || env::var("CTX_AGENT_LAUNCH").as_deref() != Ok(AGENT_LAUNCH_ABI)
    {
        return Err(ExecError::new("invalid hosted agent invocation"));
    }
    crate::runtime::control::ping_from_environment(name)
        .map_err(|error| ExecError::new(format!("run capability handshake failed: {error:?}")))?;
    let envelope = read_agent_invocation(io::stdin())
        .map_err(|_error| ExecError::new("invalid hosted agent invocation"))?;
    let step = env::var("CTX_AGENT_STEP")
        .ok()
        .and_then(|value| value.parse::<u8>().ok());
    if env::var("CTX_RUN_ID").as_deref() != Ok(envelope.run()) || step != Some(envelope.step()) {
        return Err(ExecError::new("hosted agent invocation mismatch"));
    }
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut config = AgentModelRunConfig::new(name)?;
    config.apply_invocation(&envelope);
    write_agent_debug_timing(&mut stdout, &config, "agent_runner_ready")?;
    if !is_regular_file_no_follow(&config.model_path) {
        return Err(ExecError::new(missing_model_message(
            &config.ctx_root,
            &config.model,
            &config.model_path,
        )));
    }
    config.suppress_model_error_events = true;
    let outcome = run_agent_model_once(&config, envelope.input(), &mut stdout)?;
    if !outcome.success || frames_have_error(&outcome.frames) {
        return Err(ExecError::new("agent model failed"));
    }
    if let Some(tool_call) = first_tool_call(&outcome.frames)? {
        write_agent_frames_for_tool_iteration(&mut stdout, &config.run, &outcome.frames, &tool_call)
    } else {
        write_hosted_agent_frames(&mut stdout, &config.run, &outcome.frames, outcome.streamed)
    }
}

pub(crate) struct AgentModelRunConfig {
    pub(crate) agent: String,
    pub(crate) source: PathBuf,
    pub(crate) ctx_root: PathBuf,
    pub(crate) run: String,
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
    fn new(agent: &str) -> Result<Self, ExecError> {
        let source = env::var_os("CTX_SOURCE")
            .map_or_else(cortexfs_paths::storage_current_path, PathBuf::from);
        let ctx_root = env::var_os("CTX_ROOT").map_or_else(cortexfs_paths::ctx_root, PathBuf::from);
        Self::new_with_paths(agent, source, ctx_root)
    }

    pub(crate) fn new_with_paths(
        agent: &str,
        source: PathBuf,
        ctx_root: PathBuf,
    ) -> Result<Self, ExecError> {
        let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
        let agent_dir = agent_model_control_dir(&source, agent);
        let model_path = cortexfs_paths::control_file_path(&agent_dir, "model");
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
            return Err(ExecError::new(format!(
                "invalid model reference: {requested_model}"
            )));
        };
        let primary_model = resolved_model_name(&ctx_root, &requested_model)?;
        let candidates = model_candidates(&ctx_root, &requested_model)?;
        let window_content = read_small_plain_text_file(
            &agent_dir.join("window"),
            MAX_RUNNER_CONTROL_BYTES,
            "runner",
        )
        .map_err(|error| ExecError::with_io("cannot read agent window control", &error))?;
        let window_setting = AgentWindowSetting::parse_control(&window_content)
            .ok_or_else(|| ExecError::new("invalid agent window control"))?;
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
            ExecError::new(format!(
                "no eligible model candidate for {requested_model}: {}",
                rejected.join("; ")
            ))
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
            model,
            model_path,
            system_prompt,
            prompt_template,
            rules,
            skills,
            current_time_unix: current_time_unix().to_string(),
            tool_context: String::new(),
            history_messages: "(no historical messages injected)".to_owned(),
            window_setting,
            context_budget,
            suppress_model_error_events: false,
            debug_timing_start_unix_ms: agent_debug_timing_start_unix_ms(),
        })
    }

    pub(crate) fn apply_invocation(&mut self, envelope: &AgentInvocationEnvelope) {
        self.history_messages.clear();
        self.history_messages.push_str(envelope.history_messages());
        self.tool_context.clear();
        self.tool_context.push_str(envelope.tool_context());
        let Some(observation) = envelope.observation() else {
            return;
        };
        if !self.tool_context.trim().is_empty() {
            self.tool_context.push_str("\n\n");
        }
        self.tool_context.push_str("Tool result ");
        self.tool_context.push_str(observation.tool_call_id());
        self.tool_context.push_str(" from ");
        self.tool_context.push_str(observation.name());
        self.tool_context.push_str(" status ");
        self.tool_context.push_str(observation.status());
        if observation.truncated() {
            self.tool_context.push_str(" (truncated)");
        }
        self.tool_context.push_str(":\n");
        self.tool_context.push_str(observation.content());
        trim_tool_context_to_limit(&mut self.tool_context);
    }
}

pub(crate) fn candidate_window_budget(
    ctx_root: &Path,
    model: &str,
    setting: AgentWindowSetting,
) -> Result<Option<AgentWindowBudget>, ExecError> {
    let (provider, name) = model
        .split_once('/')
        .ok_or_else(|| ExecError::new("invalid model candidate"))?;
    let content = read_small_plain_text_file(
        &cortexfs_paths::model_control_file_path(ctx_root, provider, name, "limit"),
        MAX_RUNNER_CONTROL_BYTES,
        "runner",
    )
    .map_err(|error| ExecError::with_io("cannot read model context limit", &error))?;
    let limit = ModelContextLimit::parse_control(&content)
        .ok_or_else(|| ExecError::new("invalid model context limit"))?;
    let effective = setting
        .resolve(limit)
        .map_err(|error| ExecError::new(format!("ineligible context limit: {error:?}")))?;
    Ok(budget_from_effective(effective))
}

pub(crate) fn parse_agent_context_budget(
    tokens: Option<&str>,
    chars: Option<&str>,
) -> Result<Option<AgentWindowBudget>, ExecError> {
    let (Some(tokens), Some(chars)) = (tokens, chars) else {
        return if tokens.is_none() && chars.is_none() {
            Ok(None)
        } else {
            Err(ExecError::new(
                "invalid context window environment: token and character values must be paired",
            ))
        };
    };
    let token_value = tokens.parse::<u32>().map_err(|_error| {
        ExecError::new("invalid context window environment: token value is not canonical decimal")
    })?;
    if token_value == 0 || tokens != token_value.to_string() {
        return Err(ExecError::new(
            "invalid context window environment: token value is not canonical decimal",
        ));
    }
    let window = ModelContextLimit::known(token_value)
        .and_then(budget_from_effective)
        .ok_or_else(|| ExecError::new("invalid context window environment: token value is zero"))?;
    let char_value = chars.parse::<usize>().map_err(|_error| {
        ExecError::new(
            "invalid context window environment: character value is not canonical decimal",
        )
    })?;
    if chars != char_value.to_string() || char_value != window.total_chars() {
        return Err(ExecError::new(
            "invalid context window environment: character value does not match tokens",
        ));
    }
    Ok(Some(window))
}

pub(crate) fn serialized_agent_messages(
    config: &AgentModelRunConfig,
    input: &str,
) -> Result<Vec<u8>, ExecError> {
    let context = AgentPromptContext {
        template: config.prompt_template.clone(),
        rules: config.rules.clone(),
        skills: config.skills.clone(),
        tool_injection: config.tool_context.clone(),
        history_messages: config.history_messages.clone(),
        current_time_unix: config.current_time_unix.clone(),
    };
    let messages = agent_provider_messages(input, &config.agent, &config.system_prompt, &context);
    serde_json::to_vec(&messages)
        .map_err(|error| ExecError::new(format!("cannot serialize agent prompt: {error}")))
}

pub(crate) fn admit_agent_prompt(
    config: &AgentModelRunConfig,
    input: &str,
) -> Result<bool, ExecError> {
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
    cortexfs_paths::agent_control_path(source, agent)
}

pub(crate) fn current_user_agent_model_control_dirs(source: &Path, agent: &str) -> Vec<PathBuf> {
    let mut controls = Vec::new();
    if let Some(ctx_home) = env::var_os("CTX_HOME").map(PathBuf::from)
        && ctx_home.starts_with(source)
    {
        controls.push(cortexfs_paths::agent_control_path(&ctx_home, agent));
    }
    let uid = nix::unistd::Uid::effective().as_raw().to_string();
    let uid_control =
        cortexfs_paths::agent_control_path(&cortexfs_paths::ctx_home_path(source, &uid), agent);
    if !controls.iter().any(|control| control == &uid_control) {
        controls.push(uid_control);
    }
    controls
}

pub(crate) fn is_plain_directory_no_follow(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_dir() && !metadata.file_type().is_symlink())
}
