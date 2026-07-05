fn run_agent(name: &str, args: &[OsString]) -> Result<(), String> {
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

fn run_agent_tool_loop<W, M, T>(
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
                return write_tool_error(
                    stdout,
                    &config.run,
                    "ELOOP",
                    "agent repeated the same tool call",
                )
                .map_err(|error| format!("cannot write output: {error}"));
            }
            write_agent_frames_for_tool_iteration(
                stdout,
                &config.run,
                &outcome.frames,
                &tool_call,
            )?;
            let result = execute_tool_call(config, &tool_call)
                .unwrap_or_else(|error| format!("ERROR: {error}\n"));
            write_tool_result_event(stdout, &config.run, &tool_call, &result)?;
            stdout
                .flush()
                .map_err(|error| format!("cannot write output: {error}"))?;
            config.push_tool_result(&tool_call, &result);
            last_tool_signature = Some(signature);
            last_tool_result = Some((tool_call, result));
            if iteration == MAX_AGENT_TOOL_ITERATIONS {
                return write_tool_error(
                    stdout,
                    &config.run,
                    "ELOOP",
                    "agent tool loop limit exceeded",
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

struct AgentModelRunConfig {
    agent: String,
    source: PathBuf,
    ctx_root: PathBuf,
    run: String,
    model: String,
    model_path: PathBuf,
    system_prompt: String,
    prompt_template: String,
    rules: String,
    skills: String,
    current_time_unix: String,
    tool_context: String,
    history_messages: String,
    suppress_model_error_events: bool,
    debug_timing_start_unix_ms: Option<u128>,
}

impl AgentModelRunConfig {
    fn new(agent: &str) -> Result<Self, String> {
        let source =
            env::var_os("CTX_SOURCE").map_or_else(|| PathBuf::from(DEFAULT_SOURCE), PathBuf::from);
        let ctx_root =
            env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(DEFAULT_CTX_ROOT), PathBuf::from);
        Self::new_with_paths(agent, source, ctx_root)
    }

    fn new_with_paths(agent: &str, source: PathBuf, ctx_root: PathBuf) -> Result<Self, String> {
        let run = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
        let model_path = source
            .join("agent")
            .join(format!("{agent}.d"))
            .join("model");
        let configured_model = read_small_plain_text_file(
            &model_path,
            MAX_RUNNER_CONTROL_BYTES,
            "runner",
        )
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
        let selected = candidates
            .iter()
            .find(|candidate| is_regular_file_no_follow(&candidate.path))
            .or_else(|| candidates.first())
            .ok_or_else(|| format!("invalid model reference: {requested_model}"))?;
        let model_path = selected.path.clone();
        let model = selected.name.clone();
        let agent_dir = source.join("agent").join(format!("{agent}.d"));
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
        Ok(Self {
            agent: agent.to_owned(),
            source,
            ctx_root,
            run,
            model,
            model_path,
            system_prompt,
            prompt_template,
            rules: collect_agent_rules(),
            skills: collect_skill_metadata(skill_metadata_budget_from_env()),
            current_time_unix: current_time_unix().to_string(),
            tool_context: env::var("CTX_AGENT_TOOL_CONTEXT").unwrap_or_default(),
            history_messages: env::var("CTX_AGENT_HISTORY_MESSAGES")
                .unwrap_or_else(|_error| "(no historical messages injected)".to_owned()),
            suppress_model_error_events: false,
            debug_timing_start_unix_ms: agent_debug_timing_start_unix_ms(),
        })
    }

    fn push_tool_result(&mut self, tool_call: &AgentToolCall, result: &str) {
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
